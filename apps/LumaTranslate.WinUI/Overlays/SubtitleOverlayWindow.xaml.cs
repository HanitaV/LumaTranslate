using LumaTranslate_WinUI.Services;
using Microsoft.UI;
using Microsoft.UI.Windowing;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Input;
using Microsoft.UI.Xaml.Media;
using Windows.Graphics;
using Windows.UI;
using WinRT.Interop;

namespace LumaTranslate_WinUI.Overlays;

public sealed partial class SubtitleOverlayWindow : Window
{
    private static readonly SolidColorBrush LockedBorderBrush = new(Color.FromArgb(0x55, 0xFF, 0xFF, 0xFF));
    private static readonly SolidColorBrush UnlockedBorderBrush = new(Color.FromArgb(0xFF, 0x38, 0xBD, 0xF8));

    private readonly nint _hwnd;
    private bool _lockedClickThrough = true;
    private bool _captureExcluded;
    private bool _isApplyingSettings;

    private bool _isDraggingMove;
    private bool _isDraggingResize;
    private Win32WindowInterop.POINT _dragStartCursor;
    private PointInt32 _dragStartPos;
    private SizeInt32 _dragStartSize;

    public ulong HwndValue => (ulong)_hwnd;
    public bool IsCaptureExcluded => _captureExcluded;

    public event Action<int, int, uint, uint>? BoundsChangedByUser;
    public event Action? LockRequestedByUser;

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

            DragToolbar.Visibility = _lockedClickThrough ? Visibility.Collapsed : Visibility.Visible;
            ResizeGrip.Visibility = _lockedClickThrough ? Visibility.Collapsed : Visibility.Visible;
            OverlayRoot.BorderBrush = _lockedClickThrough ? LockedBorderBrush : UnlockedBorderBrush;
            OverlayRoot.BorderThickness = _lockedClickThrough ? new Thickness(1) : new Thickness(2);

            int targetW = (int)Math.Clamp(settings.Width, 320u, 3840u);
            int targetH = (int)Math.Clamp(settings.Height, 80u, 900u);
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

    private void LockOverlayBtn_Click(object sender, RoutedEventArgs e)
    {
        LockRequestedByUser?.Invoke();
    }

    private void OverlayRoot_PointerPressed(object sender, PointerRoutedEventArgs e)
    {
        if (_lockedClickThrough || _isDraggingResize)
        {
            return;
        }
        if (!e.GetCurrentPoint(OverlayRoot).Properties.IsLeftButtonPressed)
        {
            return;
        }
        if (!Win32WindowInterop.GetCursorPos(out _dragStartCursor))
        {
            return;
        }
        _dragStartPos = AppWindow.Position;
        _isDraggingMove = true;
        OverlayRoot.CapturePointer(e.Pointer);
        e.Handled = true;
    }

    private void OverlayRoot_PointerMoved(object sender, PointerRoutedEventArgs e)
    {
        if (!_isDraggingMove || _lockedClickThrough)
        {
            return;
        }
        if (!Win32WindowInterop.GetCursorPos(out var cur))
        {
            return;
        }
        int newX = _dragStartPos.X + (cur.X - _dragStartCursor.X);
        int newY = _dragStartPos.Y + (cur.Y - _dragStartCursor.Y);
        AppWindow.Move(new PointInt32(newX, newY));
        e.Handled = true;
    }

    private void OverlayRoot_PointerReleased(object sender, PointerRoutedEventArgs e)
    {
        if (!_isDraggingMove)
        {
            return;
        }
        _isDraggingMove = false;
        OverlayRoot.ReleasePointerCapture(e.Pointer);
        NotifyCurrentBounds();
        e.Handled = true;
    }

    private void ResizeGrip_PointerPressed(object sender, PointerRoutedEventArgs e)
    {
        if (_lockedClickThrough)
        {
            return;
        }
        if (!e.GetCurrentPoint(ResizeGrip).Properties.IsLeftButtonPressed)
        {
            return;
        }
        if (!Win32WindowInterop.GetCursorPos(out _dragStartCursor))
        {
            return;
        }
        _dragStartPos = AppWindow.Position;
        _dragStartSize = AppWindow.Size;
        _isDraggingResize = true;
        ResizeGrip.CapturePointer(e.Pointer);
        e.Handled = true;
    }

    private void ResizeGrip_PointerMoved(object sender, PointerRoutedEventArgs e)
    {
        if (!_isDraggingResize || _lockedClickThrough)
        {
            return;
        }
        if (!Win32WindowInterop.GetCursorPos(out var cur))
        {
            return;
        }
        int newW = Math.Clamp(_dragStartSize.Width + (cur.X - _dragStartCursor.X), 320, 3840);
        int newH = Math.Clamp(_dragStartSize.Height + (cur.Y - _dragStartCursor.Y), 80, 900);
        AppWindow.MoveAndResize(new RectInt32(_dragStartPos.X, _dragStartPos.Y, newW, newH));
        e.Handled = true;
    }

    private void ResizeGrip_PointerReleased(object sender, PointerRoutedEventArgs e)
    {
        if (!_isDraggingResize)
        {
            return;
        }
        _isDraggingResize = false;
        ResizeGrip.ReleasePointerCapture(e.Pointer);
        NotifyCurrentBounds();
        e.Handled = true;
    }

    private void NotifyCurrentBounds()
    {
        PointInt32 pos = AppWindow.Position;
        SizeInt32 sz = AppWindow.Size;
        BoundsChangedByUser?.Invoke(
            pos.X,
            pos.Y,
            (uint)Math.Max(sz.Width, 320),
            (uint)Math.Max(sz.Height, 80));
    }

    private void OnAppWindowChanged(AppWindow sender, AppWindowChangedEventArgs args)
    {
        if (!_isApplyingSettings && !_lockedClickThrough && (args.DidPositionChange || args.DidSizeChange))
        {
            NotifyCurrentBounds();
        }
    }
}
