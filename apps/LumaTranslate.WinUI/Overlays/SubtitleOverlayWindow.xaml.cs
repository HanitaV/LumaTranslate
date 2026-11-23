using LumaTranslate_WinUI.Services;
using Microsoft.UI.Windowing;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Input;
using Windows.Graphics;
using WinRT.Interop;

namespace LumaTranslate_WinUI.Overlays;

public sealed partial class SubtitleOverlayWindow : Window
{
    private readonly nint _hwnd;
    private bool _lockedClickThrough = true;
    private bool _captureExcluded;
    private bool _isApplyingSettings;

    public ulong HwndValue => (ulong)_hwnd;
    public bool IsCaptureExcluded => _captureExcluded;

    public event Action<int, int, uint, uint>? BoundsChangedByUser;

    public SubtitleOverlayWindow()
    {
        InitializeComponent();

        _hwnd = WindowNative.GetWindowHandle(this);
        if (AppWindow.Presenter is OverlappedPresenter presenter)
        {
            presenter.IsResizable = false;
            presenter.IsMaximizable = false;
            presenter.IsMinimizable = false;
            presenter.IsAlwaysOnTop = true;
            presenter.SetBorderAndTitleBar(true, false);
        }
        AppWindow.Changed += OnAppWindowChanged;

        _captureExcluded = Win32WindowInterop.TryExcludeFromCapture(_hwnd);
        Win32WindowInterop.ConfigureSubtitleOverlayStyles(_hwnd, clickThrough: true, opacity: 0.88);
    }

    public void ApplySettings(OverlaySettingsDto settings)
    {
        _isApplyingSettings = true;
        try
        {
            _lockedClickThrough = settings.LockedClickThrough;
            VietnameseSubtitleText.FontSize = Math.Clamp(settings.FontSize, 14.0, 56.0);
            DragHintText.Visibility = _lockedClickThrough ? Visibility.Collapsed : Visibility.Visible;

            int targetW = (int)Math.Clamp(settings.Width, 320u, 3840u);
            int targetH = (int)Math.Clamp(settings.Height, 80u, 800u);
            PointInt32 curPos = AppWindow.Position;
            SizeInt32 curSize = AppWindow.Size;
            if (curPos.X != settings.X || curPos.Y != settings.Y || curSize.Width != targetW || curSize.Height != targetH)
            {
                AppWindow.MoveAndResize(new RectInt32(settings.X, settings.Y, targetW, targetH));
            }

            _captureExcluded = Win32WindowInterop.TryExcludeFromCapture(_hwnd);
            Win32WindowInterop.ConfigureSubtitleOverlayStyles(
                _hwnd,
                clickThrough: _lockedClickThrough,
                opacity: Math.Clamp(settings.Opacity, 0.30, 1.0));
        }
        finally
        {
            _isApplyingSettings = false;
        }
    }

    public void ShowWithoutStealingFocus()
    {
        Win32WindowInterop.ShowWindow(_hwnd, Win32WindowInterop.SW_SHOWNOACTIVATE);
        Win32WindowInterop.SetWindowPos(
            _hwnd,
            Win32WindowInterop.HWND_TOPMOST,
            0,
            0,
            0,
            0,
            Win32WindowInterop.SWP_NOMOVE
            | Win32WindowInterop.SWP_NOSIZE
            | Win32WindowInterop.SWP_NOACTIVATE
            | Win32WindowInterop.SWP_SHOWWINDOW);
    }

    public void HideOverlay()
    {
        Win32WindowInterop.ShowWindow(_hwnd, Win32WindowInterop.SW_HIDE);
    }

    public void UpdateSubtitleText(string vietnameseText)
    {
        VietnameseSubtitleText.Text = string.IsNullOrWhiteSpace(vietnameseText)
            ? string.Empty
            : vietnameseText;
    }

    private void OverlayRoot_PointerPressed(object sender, PointerRoutedEventArgs e)
    {
        if (_lockedClickThrough)
        {
            return;
        }
        Win32WindowInterop.ReleaseCapture();
        Win32WindowInterop.SendMessage(
            _hwnd,
            Win32WindowInterop.WM_NCLBUTTONDOWN,
            Win32WindowInterop.HTCAPTION,
            0);
    }

    private void OnAppWindowChanged(AppWindow sender, AppWindowChangedEventArgs args)
    {
        if (!_isApplyingSettings && !_lockedClickThrough && (args.DidPositionChange || args.DidSizeChange))
        {
            PointInt32 pos = sender.Position;
            SizeInt32 sz = sender.Size;
            BoundsChangedByUser?.Invoke(
                pos.X,
                pos.Y,
                (uint)Math.Max(sz.Width, 320),
                (uint)Math.Max(sz.Height, 80));
        }
    }
}
