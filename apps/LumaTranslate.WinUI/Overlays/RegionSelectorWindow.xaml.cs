using LumaTranslate_WinUI.Services;
using Microsoft.UI.Input;
using Microsoft.UI.Windowing;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;
using Windows.Foundation;
using Windows.Graphics;
using Windows.System;
using WinRT.Interop;

namespace LumaTranslate_WinUI.Overlays;

public sealed partial class RegionSelectorWindow : Window
{
    private readonly nint _hwnd;
    private readonly TaskCompletionSource<RectInt32?> _tcs = new();
    private readonly int _virtualX;
    private readonly int _virtualY;
    private bool _dragging;
    private Point _startDip;

    public RegionSelectorWindow()
    {
        InitializeComponent();

        _hwnd = WindowNative.GetWindowHandle(this);
        (_virtualX, _virtualY, int vw, int vh) = Win32WindowInterop.GetVirtualScreenBounds();

        if (AppWindow.Presenter is OverlappedPresenter presenter)
        {
            presenter.IsResizable = false;
            presenter.IsMaximizable = false;
            presenter.IsMinimizable = false;
            presenter.IsAlwaysOnTop = true;
            presenter.SetBorderAndTitleBar(true, false);
        }

        nint style = Win32WindowInterop.GetWindowLongPtr(_hwnd, Win32WindowInterop.GWL_STYLE);
        style &= ~(Win32WindowInterop.WS_CAPTION | Win32WindowInterop.WS_THICKFRAME | Win32WindowInterop.WS_SYSMENU);
        style |= Win32WindowInterop.WS_POPUP;
        Win32WindowInterop.SetWindowLongPtr(_hwnd, Win32WindowInterop.GWL_STYLE, style);

        nint exStyle = Win32WindowInterop.GetWindowLongPtr(_hwnd, Win32WindowInterop.GWL_EXSTYLE);
        exStyle |= Win32WindowInterop.WS_EX_TOPMOST | Win32WindowInterop.WS_EX_TOOLWINDOW | Win32WindowInterop.WS_EX_LAYERED;
        Win32WindowInterop.SetWindowLongPtr(_hwnd, Win32WindowInterop.GWL_EXSTYLE, exStyle);
        Win32WindowInterop.SetLayeredWindowAttributes(_hwnd, 0, 215, Win32WindowInterop.LWA_ALPHA);
        Win32WindowInterop.TryExcludeFromCapture(_hwnd);

        AppWindow.MoveAndResize(new RectInt32(_virtualX, _virtualY, vw, vh));
        Closed += (_, _) => _tcs.TrySetResult(null);
    }

    public Task<RectInt32?> PickRegionAsync()
    {
        Activate();
        SelectorRoot.Focus(FocusState.Programmatic);
        return _tcs.Task;
    }

    private void SelectorRoot_KeyDown(object sender, KeyRoutedEventArgs e)
    {
        if (e.Key == VirtualKey.Escape)
        {
            _tcs.TrySetResult(null);
            Close();
        }
    }

    private void SelectorRoot_PointerPressed(object sender, PointerRoutedEventArgs e)
    {
        PointerPoint pt = e.GetCurrentPoint(SelectionCanvas);
        if (pt.Properties.IsRightButtonPressed)
        {
            _tcs.TrySetResult(null);
            Close();
            return;
        }
        if (!pt.Properties.IsLeftButtonPressed)
        {
            return;
        }

        _dragging = true;
        _startDip = pt.Position;
        SelectorRoot.CapturePointer(e.Pointer);

        Canvas.SetLeft(SelectionBox, _startDip.X);
        Canvas.SetTop(SelectionBox, _startDip.Y);
        SelectionBox.Width = 1;
        SelectionBox.Height = 1;
        SelectionBox.Visibility = Visibility.Visible;
    }

    private void SelectorRoot_PointerMoved(object sender, PointerRoutedEventArgs e)
    {
        if (!_dragging)
        {
            return;
        }

        Point cur = e.GetCurrentPoint(SelectionCanvas).Position;
        double left = Math.Min(_startDip.X, cur.X);
        double top = Math.Min(_startDip.Y, cur.Y);
        double width = Math.Max(Math.Abs(cur.X - _startDip.X), 1.0);
        double height = Math.Max(Math.Abs(cur.Y - _startDip.Y), 1.0);

        Canvas.SetLeft(SelectionBox, left);
        Canvas.SetTop(SelectionBox, top);
        SelectionBox.Width = width;
        SelectionBox.Height = height;

        RectInt32 phys = ConvertDipRectToPhysical(left, top, width, height);
        CoordinatesBadge.Text = $"Vùng chọn: ({phys.X}, {phys.Y}) • Kích thước: {phys.Width} × {phys.Height} px";
    }

    private void SelectorRoot_PointerReleased(object sender, PointerRoutedEventArgs e)
    {
        if (!_dragging)
        {
            return;
        }
        _dragging = false;
        SelectorRoot.ReleasePointerCapture(e.Pointer);

        Point cur = e.GetCurrentPoint(SelectionCanvas).Position;
        double left = Math.Min(_startDip.X, cur.X);
        double top = Math.Min(_startDip.Y, cur.Y);
        double width = Math.Abs(cur.X - _startDip.X);
        double height = Math.Abs(cur.Y - _startDip.Y);

        RectInt32 phys = ConvertDipRectToPhysical(left, top, width, height);
        if (phys.Width >= 20 && phys.Height >= 20)
        {
            _tcs.TrySetResult(phys);
        }
        else
        {
            _tcs.TrySetResult(null);
        }
        Close();
    }

    private RectInt32 ConvertDipRectToPhysical(double leftDip, double topDip, double widthDip, double heightDip)
    {
        double scale = SelectorRoot.XamlRoot?.RasterizationScale
            ?? (Win32WindowInterop.GetDpiForWindow(_hwnd) / 96.0);
        if (scale <= 0.1)
        {
            scale = 1.0;
        }

        int pxX = _virtualX + (int)Math.Round(leftDip * scale);
        int pxY = _virtualY + (int)Math.Round(topDip * scale);
        int pxW = Math.Max((int)Math.Round(widthDip * scale), 1);
        int pxH = Math.Max((int)Math.Round(heightDip * scale), 1);
        return new RectInt32(pxX, pxY, pxW, pxH);
    }
}
