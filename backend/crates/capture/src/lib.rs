//! Windows Graphics Capture + Direct3D 11 multi-monitor region cropper.
//!
//! Safety & efficiency properties:
//! - Enumerates all Win32 monitors with physical pixel bounds and per-monitor effective DPI.
//! - Uses `Direct3D11CaptureFramePool::CreateFreeThreaded` per active monitor.
//! - Crops ONLY the requested region on the GPU (`ID3D11DeviceContext::CopySubresourceRegion`)
//!   into a reusable `D3D11_USAGE_STAGING` texture sized to the region, avoiding full-frame
//!   4K/1440p CPU readbacks.
//! - Releases capture sessions, frame pools, and Direct3D11 COM objects deterministically.

use luma_core::{CaptureRegion, MonitorInfo};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CaptureError {
    #[error("Monitor '{0}' not found")]
    MonitorNotFound(String),
    #[error("Windows Graphics Capture error: {0}")]
    Wgc(String),
    #[error("No recent frame available yet for monitor '{0}'")]
    FramePending(String),
}

#[derive(Debug, Clone)]
pub struct CroppedFrame {
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub bgra8: Vec<u8>,
    pub capture_latency_ms: u64,
}

pub trait ScreenCapturer: Send + Sync {
    fn list_monitors(&self) -> Result<Vec<MonitorInfo>, CaptureError>;
    fn capture_region(&self, region: &CaptureRegion) -> Result<CroppedFrame, CaptureError>;
    fn release_unused_monitors(&self, active_monitor_ids: &[String]);
    fn shutdown(&self);
}

#[cfg(windows)]
mod win_impl {
    use super::*;
    use windows::core::{Interface, HSTRING};
    use windows::Graphics::Capture::{
        Direct3D11CaptureFramePool, GraphicsCaptureItem, GraphicsCaptureSession,
    };
    use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
    use windows::Graphics::DirectX::DirectXPixelFormat;
    use windows::Win32::Foundation::{BOOL, LPARAM, RECT};
    use windows::Win32::Graphics::Direct3D::{
        D3D_DRIVER_TYPE_HARDWARE, D3D_FEATURE_LEVEL_11_0,
    };
    use windows::Win32::Graphics::Direct3D11::{
        D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D, D3D11_BIND_SHADER_RESOURCE,
        D3D11_BOX, D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_MAPPED_SUBRESOURCE,
        D3D11_MAP_READ, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT,
        D3D11_USAGE_STAGING,
    };
    use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};
    use windows::Win32::Graphics::Dxgi::IDXGIDevice;
    use windows::Win32::Graphics::Gdi::{
        EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFOEXW,
    };
    use windows::Win32::System::WinRT::Direct3D11::{
        CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess,
    };
    use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;
    use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};
    use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};

    const MONITORINFOF_PRIMARY_FLAG: u32 = 1;

    #[derive(Clone)]
    struct RawMonitorEntry {
        hmonitor: isize,
        info: MonitorInfo,
    }

    unsafe extern "system" fn enum_monitors_proc(
        hmon: HMONITOR,
        _hdc: HDC,
        _rect: *mut RECT,
        lparam: LPARAM,
    ) -> BOOL {
        let vec = &mut *(lparam.0 as *mut Vec<RawMonitorEntry>);
        let mut mi = MONITORINFOEXW::default();
        mi.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
        if GetMonitorInfoW(hmon, &mut mi.monitorInfo as *mut _).as_bool() {
            let mut dpi_x: u32 = 96;
            let mut dpi_y: u32 = 96;
            let _ = GetDpiForMonitor(hmon, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y);
            let scale = (dpi_x as f64 / 96.0).clamp(0.5, 4.0);

            let nul_pos = mi
                .szDevice
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(mi.szDevice.len());
            let dev_name = String::from_utf16_lossy(&mi.szDevice[..nul_pos]);
            let rc = mi.monitorInfo.rcMonitor;
            let width = (rc.right - rc.left).max(1) as u32;
            let height = (rc.bottom - rc.top).max(1) as u32;
            let id = if dev_name.is_empty() {
                format!("monitor-{}", vec.len() + 1)
            } else {
                dev_name.clone()
            };
            let is_primary = (mi.monitorInfo.dwFlags & MONITORINFOF_PRIMARY_FLAG) != 0;

            vec.push(RawMonitorEntry {
                hmonitor: hmon.0 as isize,
                info: MonitorInfo {
                    id,
                    name: format!(
                        "{} ({}x{} @ {:.0}%)",
                        dev_name,
                        width,
                        height,
                        scale * 100.0
                    ),
                    x: rc.left,
                    y: rc.top,
                    width,
                    height,
                    dpi_scale: scale,
                    is_primary,
                },
            });
        }
        BOOL(1)
    }

    fn enumerate_raw_monitors() -> Vec<RawMonitorEntry> {
        let mut list: Vec<RawMonitorEntry> = Vec::new();
        // SAFETY: `list` lives for the entire synchronous `EnumDisplayMonitors` call.
        unsafe {
            let _ = EnumDisplayMonitors(
                HDC::default(),
                None,
                Some(enum_monitors_proc),
                LPARAM(&mut list as *mut Vec<RawMonitorEntry> as isize),
            );
        }
        list
    }

    struct ActiveMonitorSession {
        monitor: MonitorInfo,
        frame_pool: Direct3D11CaptureFramePool,
        session: GraphicsCaptureSession,
        latest_gpu_tex: Option<ID3D11Texture2D>,
    }

    impl Drop for ActiveMonitorSession {
        fn drop(&mut self) {
            let _ = self.session.Close();
            let _ = self.frame_pool.Close();
            self.latest_gpu_tex = None;
        }
    }

    enum CaptureCmd {
        ListMonitors(mpsc::SyncSender<Result<Vec<MonitorInfo>, CaptureError>>),
        CaptureCrop {
            region: CaptureRegion,
            reply: mpsc::SyncSender<Result<CroppedFrame, CaptureError>>,
        },
        ReleaseUnused(Vec<String>),
        Shutdown,
    }

    pub struct WgcD3d11Capturer {
        tx: Mutex<Option<mpsc::Sender<CaptureCmd>>>,
    }

    impl WgcD3d11Capturer {
        pub fn new() -> Result<Self, CaptureError> {
            let (tx, rx) = mpsc::channel::<CaptureCmd>();
            let (ready_tx, ready_rx) = mpsc::sync_channel::<Result<(), CaptureError>>(1);

            thread::Builder::new()
                .name("luma-wgc-d3d11".to_string())
                .spawn(move || {
                    // SAFETY: Initialize WinRT MTA apartment for this dedicated capture worker thread.
                    unsafe {
                        let _ = RoInitialize(RO_INIT_MULTITHREADED);
                    }

                    let init_res = create_d3d11_and_winrt_device();
                    let (d3d_device, d3d_ctx, winrt_device) = match init_res {
                        Ok(t) => {
                            let _ = ready_tx.send(Ok(()));
                            t
                        }
                        Err(e) => {
                            let _ = ready_tx.send(Err(e));
                            return;
                        }
                    };

                    let mut sessions: HashMap<String, ActiveMonitorSession> = HashMap::new();
                    let mut staging_cache: HashMap<(u32, u32), ID3D11Texture2D> = HashMap::new();

                    while let Ok(cmd) = rx.recv() {
                        match cmd {
                            CaptureCmd::ListMonitors(reply) => {
                                let mons = enumerate_raw_monitors()
                                    .into_iter()
                                    .map(|e| e.info)
                                    .collect();
                                let _ = reply.send(Ok(mons));
                            }
                            CaptureCmd::CaptureCrop { region, reply } => {
                                let res = capture_region_on_worker(
                                    &d3d_device,
                                    &d3d_ctx,
                                    &winrt_device,
                                    &mut sessions,
                                    &mut staging_cache,
                                    &region,
                                );
                                let _ = reply.send(res);
                            }
                            CaptureCmd::ReleaseUnused(active_ids) => {
                                sessions.retain(|k, _| active_ids.iter().any(|id| id == k));
                                if sessions.is_empty() {
                                    staging_cache.clear();
                                }
                            }
                            CaptureCmd::Shutdown => {
                                sessions.clear();
                                staging_cache.clear();
                                break;
                            }
                        }
                    }
                })
                .map_err(|e| CaptureError::Wgc(format!("Failed to spawn capture thread: {e}")))?;

            ready_rx
                .recv()
                .map_err(|e| CaptureError::Wgc(e.to_string()))??;

            Ok(Self {
                tx: Mutex::new(Some(tx)),
            })
        }
    }

    fn create_d3d11_and_winrt_device(
    ) -> Result<(ID3D11Device, ID3D11DeviceContext, IDirect3DDevice), CaptureError> {
        // SAFETY: Standard D3D11 hardware device creation with BGRA support required by WGC.
        unsafe {
            let mut d3d_device: Option<ID3D11Device> = None;
            let mut d3d_ctx: Option<ID3D11DeviceContext> = None;
            let feature_levels = [D3D_FEATURE_LEVEL_11_0];
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                None,
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                Some(&feature_levels),
                D3D11_SDK_VERSION,
                Some(&mut d3d_device),
                None,
                Some(&mut d3d_ctx),
            )
            .map_err(|e| CaptureError::Wgc(format!("D3D11CreateDevice failed: {e}")))?;

            let d3d_device =
                d3d_device.ok_or_else(|| CaptureError::Wgc("Null ID3D11Device".into()))?;
            let d3d_ctx =
                d3d_ctx.ok_or_else(|| CaptureError::Wgc("Null ID3D11DeviceContext".into()))?;

            let dxgi_device: IDXGIDevice = d3d_device
                .cast()
                .map_err(|e| CaptureError::Wgc(format!("Cast IDXGIDevice failed: {e}")))?;
            let inspectable = CreateDirect3D11DeviceFromDXGIDevice(&dxgi_device)
                .map_err(|e| CaptureError::Wgc(format!("CreateDirect3D11Device failed: {e}")))?;
            let winrt_device: IDirect3DDevice = inspectable
                .cast()
                .map_err(|e| CaptureError::Wgc(format!("Cast IDirect3DDevice failed: {e}")))?;

            Ok((d3d_device, d3d_ctx, winrt_device))
        }
    }

    fn create_monitor_capture_item(hmonitor: isize) -> Result<GraphicsCaptureItem, CaptureError> {
        // SAFETY: Uses official WinRT `IGraphicsCaptureItemInterop::CreateForMonitor` activation factory.
        unsafe {
            let interop: IGraphicsCaptureItemInterop = windows::core::factory::<
                GraphicsCaptureItem,
                IGraphicsCaptureItemInterop,
            >()
            .map_err(|e| CaptureError::Wgc(format!("IGraphicsCaptureItemInterop failed: {e}")))?;
            interop
                .CreateForMonitor(HMONITOR(hmonitor as *mut core::ffi::c_void))
                .map_err(|e| CaptureError::Wgc(format!("CreateForMonitor failed: {e}")))
        }
    }

    fn capture_region_on_worker(
        d3d_device: &ID3D11Device,
        d3d_ctx: &ID3D11DeviceContext,
        winrt_device: &IDirect3DDevice,
        sessions: &mut HashMap<String, ActiveMonitorSession>,
        staging_cache: &mut HashMap<(u32, u32), ID3D11Texture2D>,
        region: &CaptureRegion,
    ) -> Result<CroppedFrame, CaptureError> {
        let t0 = Instant::now();

        if !sessions.contains_key(&region.monitor_id) {
            let raw_mons = enumerate_raw_monitors();
            let entry = raw_mons
                .iter()
                .find(|m| m.info.id == region.monitor_id)
                .or_else(|| raw_mons.iter().find(|m| m.info.is_primary))
                .or_else(|| raw_mons.first())
                .ok_or_else(|| CaptureError::MonitorNotFound(region.monitor_id.clone()))?;

            let item = create_monitor_capture_item(entry.hmonitor)?;
            let size = item
                .Size()
                .map_err(|e| CaptureError::Wgc(format!("CaptureItem.Size failed: {e}")))?;
            let frame_pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
                winrt_device,
                DirectXPixelFormat::B8G8R8A8UIntNormalized,
                2,
                size,
            )
            .map_err(|e| CaptureError::Wgc(format!("CreateFreeThreaded failed: {e}")))?;

            let session = frame_pool
                .CreateCaptureSession(&item)
                .map_err(|e| CaptureError::Wgc(format!("CreateCaptureSession failed: {e}")))?;
            let _ = session.SetIsCursorCaptureEnabled(false);
            let _ = session.SetIsBorderRequired(false);
            session
                .StartCapture()
                .map_err(|e| CaptureError::Wgc(format!("StartCapture failed: {e}")))?;

            sessions.insert(
                region.monitor_id.clone(),
                ActiveMonitorSession {
                    monitor: entry.info.clone(),
                    frame_pool,
                    session,
                    latest_gpu_tex: None,
                },
            );
        }

        let mon_session = sessions.get_mut(&region.monitor_id).unwrap();

        // Drain frame pool: copy the newest WGC surface into our own private `ID3D11Texture2D`
        // and immediately release the WGC surface + frame so the 2-buffer `Direct3D11CaptureFramePool`
        // is never starved across continuous subtitle updates!
        let mut waited_ms = 0u64;
        loop {
            let mut got_any = false;
            while let Ok(frame) = mon_session.frame_pool.TryGetNextFrame() {
                if let Ok(surface) = frame.Surface() {
                    if let Ok(access) = surface.cast::<IDirect3DDxgiInterfaceAccess>() {
                        // SAFETY: Extract WGC frame texture, copy GPU->GPU to owned texture, and drop immediately.
                        unsafe {
                            if let Ok(wgc_tex) = access.GetInterface::<ID3D11Texture2D>() {
                                let mut desc = D3D11_TEXTURE2D_DESC::default();
                                wgc_tex.GetDesc(&mut desc);
                                let needs_alloc = match mon_session.latest_gpu_tex {
                                    Some(ref existing) => {
                                        let mut cur = D3D11_TEXTURE2D_DESC::default();
                                        existing.GetDesc(&mut cur);
                                        cur.Width != desc.Width || cur.Height != desc.Height
                                    }
                                    None => true,
                                };
                                if needs_alloc {
                                    let owned_desc = D3D11_TEXTURE2D_DESC {
                                        Width: desc.Width,
                                        Height: desc.Height,
                                        MipLevels: 1,
                                        ArraySize: 1,
                                        Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                                        SampleDesc: DXGI_SAMPLE_DESC {
                                            Count: 1,
                                            Quality: 0,
                                        },
                                        Usage: D3D11_USAGE_DEFAULT,
                                        BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
                                        CPUAccessFlags: 0,
                                        MiscFlags: 0,
                                    };
                                    let mut new_owned: Option<ID3D11Texture2D> = None;
                                    if d3d_device
                                        .CreateTexture2D(&owned_desc, None, Some(&mut new_owned))
                                        .is_ok()
                                    {
                                        mon_session.latest_gpu_tex = new_owned;
                                    }
                                }
                                if let Some(ref owned_tex) = mon_session.latest_gpu_tex {
                                    d3d_ctx.CopyResource(owned_tex, &wgc_tex);
                                    got_any = true;
                                }
                            }
                        }
                    }
                }
                let _ = frame.Close();
            }
            let max_wait_ms = if mon_session.latest_gpu_tex.is_none() { 160 } else { 80 };
            if got_any || mon_session.latest_gpu_tex.is_some() || waited_ms >= max_wait_ms {
                break;
            }
            thread::sleep(Duration::from_millis(8));
            waited_ms += 8;
        }

        let Some(ref src_tex) = mon_session.latest_gpu_tex else {
            return Err(CaptureError::FramePending(region.monitor_id.clone()));
        };

        // Compute monitor-local coordinates for `D3D11_BOX` crop
        let mut src_desc = D3D11_TEXTURE2D_DESC::default();
        // SAFETY: Valid COM pointer on capture worker thread.
        unsafe {
            src_tex.GetDesc(&mut src_desc);
        }

        let rel_x = (region.x - mon_session.monitor.x).max(0) as u32;
        let rel_y = (region.y - mon_session.monitor.y).max(0) as u32;
        if rel_x >= src_desc.Width || rel_y >= src_desc.Height {
            return Err(CaptureError::Wgc("Region lies outside captured surface".into()));
        }
        let crop_w = region.width.min(src_desc.Width - rel_x).max(1);
        let crop_h = region.height.min(src_desc.Height - rel_y).max(1);

        let staging_tex = if let Some(existing) = staging_cache.get(&(crop_w, crop_h)) {
            existing.clone()
        } else {
            let desc = D3D11_TEXTURE2D_DESC {
                Width: crop_w,
                Height: crop_h,
                MipLevels: 1,
                ArraySize: 1,
                Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                SampleDesc: DXGI_SAMPLE_DESC {
                    Count: 1,
                    Quality: 0,
                },
                Usage: D3D11_USAGE_STAGING,
                BindFlags: 0,
                CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
                MiscFlags: 0,
            };
            let mut tex_out: Option<ID3D11Texture2D> = None;
            // SAFETY: Valid staging texture descriptor on our D3D11 device.
            unsafe {
                d3d_device
                    .CreateTexture2D(&desc, None, Some(&mut tex_out))
                    .map_err(|e| CaptureError::Wgc(format!("CreateTexture2D staging failed: {e}")))?;
            }
            let created = tex_out.unwrap();
            staging_cache.insert((crop_w, crop_h), created.clone());
            created
        };

        let crop_box = D3D11_BOX {
            left: rel_x,
            top: rel_y,
            front: 0,
            right: rel_x + crop_w,
            bottom: rel_y + crop_h,
            back: 1,
        };

        let row_bytes = (crop_w * 4) as usize;
        let mut packed_bgra = vec![0u8; row_bytes * (crop_h as usize)];

        // SAFETY: Copy only the cropped `crop_box` region on GPU to `staging_tex`, then map for CPU read.
        unsafe {
            d3d_ctx.CopySubresourceRegion(
                &staging_tex,
                0,
                0,
                0,
                0,
                src_tex,
                0,
                Some(&crop_box),
            );

            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            d3d_ctx
                .Map(&staging_tex, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
                .map_err(|e| CaptureError::Wgc(format!("D3D11 Map failed: {e}")))?;

            let src_ptr = mapped.pData as *const u8;
            let src_pitch = mapped.RowPitch as usize;
            for y in 0..(crop_h as usize) {
                let src_row = std::slice::from_raw_parts(src_ptr.add(y * src_pitch), row_bytes);
                let dst_offset = y * row_bytes;
                packed_bgra[dst_offset..dst_offset + row_bytes].copy_from_slice(src_row);
            }

            d3d_ctx.Unmap(&staging_tex, 0);
        }

        let _ = HSTRING::new(); // keep windows core linked cleanly

        Ok(CroppedFrame {
            width: crop_w,
            height: crop_h,
            stride: crop_w * 4,
            bgra8: packed_bgra,
            capture_latency_ms: t0.elapsed().as_millis() as u64,
        })
    }

    impl ScreenCapturer for WgcD3d11Capturer {
        fn list_monitors(&self) -> Result<Vec<MonitorInfo>, CaptureError> {
            let guard = self.tx.lock();
            let Some(ref tx) = *guard else {
                return Err(CaptureError::Wgc("Capturer shut down".into()));
            };
            let (reply_tx, reply_rx) = mpsc::sync_channel(1);
            tx.send(CaptureCmd::ListMonitors(reply_tx))
                .map_err(|_| CaptureError::Wgc("Capture thread closed".into()))?;
            reply_rx
                .recv()
                .map_err(|e| CaptureError::Wgc(e.to_string()))?
        }

        fn capture_region(&self, region: &CaptureRegion) -> Result<CroppedFrame, CaptureError> {
            let guard = self.tx.lock();
            let Some(ref tx) = *guard else {
                return Err(CaptureError::Wgc("Capturer shut down".into()));
            };
            let (reply_tx, reply_rx) = mpsc::sync_channel(1);
            tx.send(CaptureCmd::CaptureCrop {
                region: region.clone(),
                reply: reply_tx,
            })
            .map_err(|_| CaptureError::Wgc("Capture thread closed".into()))?;
            reply_rx
                .recv()
                .map_err(|e| CaptureError::Wgc(e.to_string()))?
        }

        fn release_unused_monitors(&self, active_monitor_ids: &[String]) {
            if let Some(ref tx) = *self.tx.lock() {
                let _ = tx.send(CaptureCmd::ReleaseUnused(active_monitor_ids.to_vec()));
            }
        }

        fn shutdown(&self) {
            if let Some(tx) = self.tx.lock().take() {
                let _ = tx.send(CaptureCmd::Shutdown);
            }
        }
    }

    impl Drop for WgcD3d11Capturer {
        fn drop(&mut self) {
            self.shutdown();
        }
    }
}

#[cfg(windows)]
pub use win_impl::WgcD3d11Capturer;
