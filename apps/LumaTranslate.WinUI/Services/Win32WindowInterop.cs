using System.Runtime.InteropServices;

namespace LumaTranslate_WinUI.Services;

internal static partial class Win32WindowInterop
{
    public const int GWL_STYLE = -16;
    public const int GWL_EXSTYLE = -20;

    public static readonly nint WS_CAPTION = 0x00C00000;
    public static readonly nint WS_THICKFRAME = 0x00040000;
    public static readonly nint WS_SYSMENU = 0x00080000;
    public static readonly nint WS_POPUP = unchecked((nint)0x80000000L);

    public static readonly nint WS_EX_TRANSPARENT = 0x00000020;
    public static readonly nint WS_EX_TOOLWINDOW = 0x00000080;
    public static readonly nint WS_EX_TOPMOST = 0x00000008;
    public static readonly nint WS_EX_LAYERED = 0x00080000;
    public static readonly nint WS_EX_NOACTIVATE = 0x08000000;

    public const uint WDA_NONE = 0x00000000;
    public const uint WDA_EXCLUDEFROMCAPTURE = 0x00000011;

    public static readonly nint HWND_TOPMOST = -1;
    public const uint SWP_NOMOVE = 0x0002;
    public const uint SWP_NOSIZE = 0x0001;
    public const uint SWP_NOACTIVATE = 0x0010;
    public const uint SWP_FRAMECHANGED = 0x0020;
    public const uint SWP_SHOWWINDOW = 0x0040;

    public const int SW_SHOWNOACTIVATE = 4;
    public const int SW_HIDE = 0;

    public const uint LWA_ALPHA = 0x00000002;

    public const int SM_XVIRTUALSCREEN = 76;
    public const int SM_YVIRTUALSCREEN = 77;
    public const int SM_CXVIRTUALSCREEN = 78;
    public const int SM_CYVIRTUALSCREEN = 79;

    [LibraryImport("user32.dll", EntryPoint = "GetWindowLongPtrW", SetLastError = true)]
    public static partial nint GetWindowLongPtr(nint hWnd, int nIndex);

    [LibraryImport("user32.dll", EntryPoint = "SetWindowLongPtrW", SetLastError = true)]
    public static partial nint SetWindowLongPtr(nint hWnd, int nIndex, nint dwNewLong);

    [LibraryImport("user32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    public static partial bool SetWindowDisplayAffinity(nint hWnd, uint dwAffinity);

    [LibraryImport("user32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    public static partial bool SetWindowPos(
        nint hWnd,
        nint hWndInsertAfter,
        int x,
        int y,
        int cx,
        int cy,
        uint uFlags);

    [LibraryImport("user32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    public static partial bool ShowWindow(nint hWnd, int nCmdShow);

    [LibraryImport("user32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    public static partial bool SetLayeredWindowAttributes(nint hWnd, uint crKey, byte bAlpha, uint dwFlags);

    [LibraryImport("user32.dll")]
    public static partial int GetSystemMetrics(int nIndex);

    [LibraryImport("user32.dll")]
    public static partial uint GetDpiForWindow(nint hwnd);

    [LibraryImport("user32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    public static partial bool ReleaseCapture();

    [LibraryImport("user32.dll", EntryPoint = "SendMessageW")]
    public static partial nint SendMessage(nint hWnd, uint msg, nint wParam, nint lParam);

    public const uint WM_NCLBUTTONDOWN = 0x00A1;
    public const nint HTCAPTION = 2;

    [StructLayout(LayoutKind.Sequential)]
    public struct POINT
    {
        public int X;
        public int Y;
    }

    [LibraryImport("user32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    public static partial bool GetCursorPos(out POINT lpPoint);

    public static bool TryExcludeFromCapture(nint hwnd)
    {
        if (hwnd == 0)
        {
            return false;
        }
        return SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE);
    }

    public static void ConfigureSubtitleOverlayStyles(nint hwnd, bool clickThrough, double opacity)
    {
        if (hwnd == 0)
        {
            return;
        }

        nint style = GetWindowLongPtr(hwnd, GWL_STYLE);
        style &= ~(WS_CAPTION | WS_THICKFRAME | WS_SYSMENU);
        style |= WS_POPUP;
        SetWindowLongPtr(hwnd, GWL_STYLE, style);

        nint exStyle = GetWindowLongPtr(hwnd, GWL_EXSTYLE);
        exStyle |= WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_LAYERED;
        if (clickThrough)
        {
            exStyle |= WS_EX_TRANSPARENT | WS_EX_NOACTIVATE;
        }
        else
        {
            exStyle &= ~(WS_EX_TRANSPARENT | WS_EX_NOACTIVATE);
        }
        SetWindowLongPtr(hwnd, GWL_EXSTYLE, exStyle);

        byte alpha = (byte)Math.Clamp(Math.Round(opacity * 255.0), 75.0, 255.0);
        SetLayeredWindowAttributes(hwnd, 0, alpha, LWA_ALPHA);

        SetWindowPos(
            hwnd,
            HWND_TOPMOST,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_FRAMECHANGED);
    }

    public static (int X, int Y, int Width, int Height) GetVirtualScreenBounds()
    {
        int x = GetSystemMetrics(SM_XVIRTUALSCREEN);
        int y = GetSystemMetrics(SM_YVIRTUALSCREEN);
        int w = Math.Max(GetSystemMetrics(SM_CXVIRTUALSCREEN), 800);
        int h = Math.Max(GetSystemMetrics(SM_CYVIRTUALSCREEN), 600);
        return (x, y, w, h);
    }
}
