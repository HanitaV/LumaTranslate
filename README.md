# LumaTranslate

🌐 **Ngôn ngữ / Language**: **Tiếng Việt** | [English](README.en.md)

**LumaTranslate** là ứng dụng dịch phụ đề vùng màn hình thời gian thực và đọc thuyết minh tiếng Việt cục bộ dành cho Windows 11, lấy cảm hứng từ LunaTranslator. Ứng dụng kết hợp giao diện **WinUI 3 (C#)** chuẩn Fluent bằng tiếng Việt, nhân xử lý thời gian thực **Rust + Tokio**, nhận diện chữ **PaddleOCR PP-OCRv5 Mobile ưu tiên CUDA**, **dịch đám mây sang tiếng Việt (`vi`)** và tổng hợp giọng nói cục bộ **VieNeu-TTS v3 Turbo (`48 kHz`)**.

## Tính năng nổi bật

- **Chụp nhiều vùng & đa màn hình**: Kéo chọn một hoặc nhiều vùng phụ đề trên các màn hình có tỷ lệ phóng đại DPI khác nhau, vận hành trên `Windows.Graphics.Capture` kết hợp cắt vùng trực tiếp trên GPU qua `Direct3D 11` (`CopySubresourceRegion`).
- **OCR cục bộ ưu tiên CUDA**: Tiến trình **PaddleOCR PP-OCRv5 Mobile** chạy nền liên tục (`paddlepaddle-gpu 3.3.1 cu126`), hỗ trợ tiếng Nhật, tiếng Anh, tiếng Trung giản thể & phồn thể, tiếng Hàn và các ngôn ngữ chữ Latinh (`~36–77 ms` trên GPU RTX 3060 Laptop), tự động chuyển về CPU khi không có GPU rời hoặc khi thiếu VRAM (OOM).
- **Ổn định phụ đề & lọc nhiễu**: `FrameDiffDetector` bỏ qua các khung hình tĩnh để giảm tải; `OcrStabilizer` chờ hiệu ứng chữ chạy (typewriter) hiển thị trọn câu và lọc hiện tượng nhảy ký tự nhỏ của OCR.
- **Dịch đám mây chuyên biệt sang tiếng Việt**: Hỗ trợ **MyMemory** (mặc định, không cần API key), **Microsoft Azure Translator** (REST v3.0 chính thức, tương thích gói miễn phí F0) và **Google Cloud Translation** (REST v2 chính thức) kèm xác nhận quyền riêng tư minh bạch, kiểm tra kết nối, gộp yêu cầu trùng đang xử lý và chặn bản dịch cũ trả về muộn.
- **Bộ nhớ đệm hai tầng & lịch sử hội thoại**: Bộ nhớ đệm LRU giới hạn dung lượng trên RAM kết hợp cơ sở dữ liệu **SQLite WAL** (`%LOCALAPPDATA%\LumaTranslate\data\luma_translate.db`), đi kèm lịch sử bản dịch tiếng Việt hỗ trợ tìm kiếm nhanh.
- **Lớp phủ phụ đề luôn nổi (Topmost Overlay)**: Cửa sổ phụ đề không viền, trong suốt, cho phép nhấp xuyên qua (click-through) và được loại trừ khỏi luồng chụp màn hình qua `SetWindowDisplayAffinity(WDA_EXCLUDEFROMCAPTURE)` cùng bộ lọc chống lặp ngược cấp hai.
- **Đọc giọng nói tiếng Việt cục bộ tự động (TTS)**: Phát trực tiếp luồng âm thanh `48 kHz` từ **VieNeu-TTS v3 Turbo** (độ trễ đoạn đầu `~186 ms` trên RTX 3060 CUDA, tự động chuyển sang ONNX CPU khi thiếu VRAM) thẳng ra Windows WASAPI với cơ chế ưu tiên phụ đề mới nhất (`latest-subtitle-wins`).

## Yêu cầu hệ thống

- **Hệ điều hành**: Windows 11 x64 (hoặc Windows 10 phiên bản 2004 / build 19041 trở lên)
- **GPU**: Khuyến nghị NVIDIA RTX 3060 (6 GB Laptop hoặc 12 GB Desktop) với Driver `>= 550.54` để tăng tốc CUDA (hỗ trợ tự động chạy trên CPU nếu không có GPU tương thích)
- **Môi trường phụ thuộc**: **Không cần cài sẵn** Python, .NET hay CUDA Toolkit — ứng dụng sẽ tự động tải môi trường cô lập và mô hình trong lần chạy đầu tiên.

## Bắt đầu nhanh (Bộ cài đặt / Prebuilt)

### 1. Cài đặt LumaTranslate

1. Truy cập trang [GitHub Releases](https://github.com/HanitaV/LumaTranslate/releases), kéo xuống cuối bài viết của bản phát hành mới nhất và **bấm vào mục `▸ Assets`** để mở danh sách tệp đính kèm, sau đó tải bộ cài **`LumaTranslate-Setup-*-x64.exe`** *(hoặc gói `*-portable.zip` nếu muốn dùng dạng giải nén di động)*.
2. Chạy tệp **`LumaTranslate-Setup-*-x64.exe`**: trình cài đặt tiếng Việt sẽ tự động cài ứng dụng vào `%LOCALAPPDATA%\Programs\LumaTranslate` (**không yêu cầu quyền Quản trị viên / Admin**), tạo lối tắt trong **Start Menu** (và **Desktop** nếu chọn), rồi mở thẳng LumaTranslate.

### 2. Thiết lập AI tự động lần đầu & Sử dụng

`LumaTranslate.exe` đóng vai trò **vừa là trình cài đặt môi trường AI lần đầu, vừa là trình khởi chạy ứng dụng**:

- **Lần chạy đầu tiên (máy chưa thiết lập)**: Tự động mở trình hướng dẫn **Thiết lập môi trường LumaTranslate**, nhận diện GPU/VRAM NVIDIA và độ tương thích driver, tải `uv` độc lập cùng môi trường Python 3.12 cô lập vào `%LOCALAPPDATA%\LumaTranslate\runtimes\`, cài đặt các gói OCR & VieNeu-TTS bản CUDA/CPU, tải và xác thực các mô hình chuẩn Apache-2.0 tại `%LOCALAPPDATA%\LumaTranslate\models\`, sau đó chuyển thẳng vào ứng dụng chính.
- **Các lần chạy tiếp theo**: Mở **LumaTranslate** từ Start Menu / Desktop để khởi động ngay `LumaTranslate.Backend.exe`, cửa sổ phụ đề nổi `SubtitleOverlayWindow` và màn hình dịch chính.
- **Cập nhật phiên bản mới**: Chỉ cần chạy đè bộ cài `LumaTranslate-Setup-*-x64.exe` mới — các mô hình AI và lịch sử dịch trong `%LOCALAPPDATA%\LumaTranslate` sẽ được giữ nguyên.

**Tham số dòng lệnh tùy chọn (CLI)**:
- `LumaTranslate.exe --setup` — Buộc mở lại trình hướng dẫn cài đặt / sửa chữa môi trường AI tích hợp.
- `LumaTranslate.exe --auto-install --exit-after-setup` — Tự động thiết lập môi trường AI không cần tương tác và thoát khi hoàn tất.
- `LumaTranslate.exe --uninstall` — Mở hộp thoại gỡ cài đặt giao diện tiếng Việt của ứng dụng.

### 3. Gỡ cài đặt LumaTranslate

Bạn có thể gỡ cài đặt ứng dụng (`%LOCALAPPDATA%\Programs\LumaTranslate`) cùng môi trường AI / dữ liệu (`%LOCALAPPDATA%\LumaTranslate`) từ bất kỳ nơi nào sau đây:

- **Ứng dụng đã cài đặt của Windows**: Mở **Settings → Apps → Installed apps → LumaTranslate → Uninstall** (trình gỡ cài đặt sẽ hỏi rõ bạn có muốn xóa luôn mô hình AI vài GB và lịch sử dịch hay không; chọn **Modify** để mở trình sửa chữa môi trường AI).
- **Trong Cài đặt ứng dụng**: Mở **Cài đặt → Quản lý dữ liệu, môi trường AI & Gỡ cài đặt phần mềm → Gỡ cài đặt phần mềm...** (hoặc nhấn **Gỡ cài đặt / Dọn dẹp...** ngay trên trình hướng dẫn Thiết lập môi trường).
- **PowerShell (tự động / bản portable)**: Chạy `powershell -ExecutionPolicy Bypass -File .\uninstall.ps1` trong thư mục cài đặt (thêm `-Unattended -RemoveAppFolder` để gỡ ngầm hoàn toàn).

---

## Dành cho nhà phát triển (Chạy từ mã nguồn)

### Công cụ phát triển cần thiết

- Rust `1.80+` (`rustc` / `cargo`)
- .NET `10.0` SDK + Windows App SDK `2.7+`
- Python `3.12` x64 + [`uv`](https://github.com/astral-sh/uv)
- [Inno Setup 6](https://jrsoftware.org/isinfo.php) (`winget install JRSoftware.InnoSetup` — tùy chọn khi muốn xuất bộ cài `LumaTranslate-Setup-x64.exe`)

### 1. Thiết lập môi trường suy luận & tải mô hình

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\setup.ps1 -AcceptModelDownload
```

### 2. Kiểm tra thực thi CUDA cho OCR & VieNeu-TTS

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\verify-cuda.ps1
```

### 3. Biên dịch & chạy từ mã nguồn

```powershell
# Biên dịch Rust backend (Release) và giao diện WinUI 3 x64
powershell -ExecutionPolicy Bypass -File .\scripts\build.ps1 -Configuration Release

# Khởi chạy bản biên dịch cục bộ
.\apps\LumaTranslate.WinUI\bin\x64\Release\net10.0-windows10.0.26100.0\win-x64\LumaTranslate.exe
```

### 4. Chạy kiểm thử tự động

```powershell
# Toàn bộ kiểm thử Rust unit/integration + Python worker + kiểm thử GPU CUDA thực tế
powershell -ExecutionPolicy Bypass -File .\scripts\test.ps1

# Chỉ chạy kiểm thử không phụ thuộc phần cứng (bỏ qua kiểm thử mô hình trực tiếp trên GPU)
powershell -ExecutionPolicy Bypass -File .\scripts\test.ps1 -SkipGpu
```

### 5. Đóng gói bộ cài `LumaTranslate-Setup-x64.exe` & bản phân phối

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\package.ps1
# Tạo thư mục phân phối tại dist\LumaTranslate-win-x64\
# và biên dịch bộ cài AppData tại dist\LumaTranslate-Setup-x64.exe
```

## Tài liệu chi tiết

- [Kiến trúc & Xử lý đồng thời (EN)](docs/architecture.md)
- [Thiết lập CUDA & Phân bổ VRAM trên RTX 3060 (EN)](docs/cuda-setup.md)
- [Dịch đám mây & Quyền riêng tư (EN)](docs/cloud-translation.md)
- [Hiệu năng & Kết quả đo kiểm thực tế (EN)](docs/performance.md)
- [Xử lý sự cố thường gặp (EN)](docs/troubleshooting.md)

## Giấy phép

Mã nguồn trong kho lưu trữ này được cấp phép theo [Giấy phép MIT](LICENSE). Trọng số các mô hình tải về (`PP-OCRv5 Mobile`, `VieNeu-TTS v3 Turbo`, `MOSS-Audio-Tokenizer-Nano`) được cấp phép theo chuẩn **Apache-2.0** bởi các tác giả tương ứng và được lưu trữ cục bộ tại `%LOCALAPPDATA%\LumaTranslate\models\`.
