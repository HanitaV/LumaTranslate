using LumaTranslate_WinUI.Overlays;
using LumaTranslate_WinUI.Services;
using LumaTranslate_WinUI.Views;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Windows.Graphics;

namespace LumaTranslate_WinUI;

public sealed partial class MainWindow : Window
{
    private SubtitleOverlayWindow? _overlayWindow;
    private bool _mainShellStarted;
    private bool _shuttingDown;

    public MainWindow()
    {
        InitializeComponent();

        ExtendsContentIntoTitleBar = true;
        SetTitleBar(AppTitleBar);
        string iconPath = Path.Combine(AppContext.BaseDirectory, "Assets", "AppIcon.ico");
        if (File.Exists(iconPath))
        {
            AppWindow.SetIcon(iconPath);
        }
        AppWindow.Resize(new SizeInt32(980, 740));

        Closed += MainWindow_Closed;

        ResolvedEnvironmentPaths env = EnvironmentBootstrapService.InspectEnvironment();
        if (App.ForceSetupWizard || !env.IsReady)
        {
            OpenSetupWizard();
        }
        else
        {
            CompleteSetupAndLaunchMainShell();
        }
    }

    public void OpenSetupWizard()
    {
        _overlayWindow?.HideOverlay();
        ShellNav.Visibility = Visibility.Collapsed;
        SetupHostFrame.Visibility = Visibility.Visible;
        SetupHostFrame.Navigate(typeof(SetupWizardPage));
    }

    public void CompleteSetupAndLaunchMainShell()
    {
        SetupHostFrame.Visibility = Visibility.Collapsed;
        ShellNav.Visibility = Visibility.Visible;

        if (!_mainShellStarted)
        {
            _mainShellStarted = true;
            App.ViewModel.Initialize(DispatcherQueue);

            _overlayWindow = new SubtitleOverlayWindow();
            _overlayWindow.ApplySettings(App.ViewModel.Settings.Overlay);
            _overlayWindow.ShowWithoutStealingFocus();

            _overlayWindow.BoundsChangedByUser += async (x, y, w, h) =>
            {
                App.ViewModel.Settings.Overlay.X = x;
                App.ViewModel.Settings.Overlay.Y = y;
                App.ViewModel.Settings.Overlay.Width = w;
                App.ViewModel.Settings.Overlay.Height = h;
                await RegisterOverlayWithBackendAsync();
            };

            App.ViewModel.SubtitleChangedForOverlay += sub =>
            {
                _overlayWindow?.UpdateSubtitleText(sub.VietnameseText);
                if (App.ViewModel.OverlayVisible)
                {
                    _overlayWindow?.ShowWithoutStealingFocus();
                }
            };

            App.ViewModel.OverlaySettingsChanged += settings =>
            {
                _overlayWindow?.ApplySettings(settings);
                _ = RegisterOverlayWithBackendAsync();
            };

            App.ViewModel.Ipc.ConnectionStateChanged += connected =>
            {
                if (connected)
                {
                    DispatcherQueue.TryEnqueue(() => _ = RegisterOverlayWithBackendAsync());
                }
            };
        }
        else if (App.ViewModel.OverlayVisible)
        {
            _overlayWindow?.ShowWithoutStealingFocus();
        }

        ShellNav.SelectedItem = ShellNav.MenuItems[0];
        ContentFrame.Navigate(typeof(TranslatePage));
    }

    public void SetSubtitleOverlayVisibility(bool visible)
    {
        App.ViewModel.OverlayVisible = visible;
        if (visible)
        {
            _overlayWindow?.ShowWithoutStealingFocus();
        }
        else
        {
            _overlayWindow?.HideOverlay();
        }
    }

    private async Task RegisterOverlayWithBackendAsync()
    {
        if (_overlayWindow is null || !App.ViewModel.Ipc.IsConnected)
        {
            return;
        }
        try
        {
            OverlaySettingsDto o = App.ViewModel.Settings.Overlay;
            await App.ViewModel.Ipc.RegisterOverlayWindowAsync(
                _overlayWindow.HwndValue,
                _overlayWindow.IsCaptureExcluded,
                o.X,
                o.Y,
                o.Width,
                o.Height);
        }
        catch
        {
        }
    }

    private void ShellNav_SelectionChanged(NavigationView sender, NavigationViewSelectionChangedEventArgs args)
    {
        if (args.SelectedItem is NavigationViewItem item && item.Tag is string tag)
        {
            Type targetPage = tag switch
            {
                "history" => typeof(HistoryPage),
                "settings" => typeof(SettingsPage),
                _ => typeof(TranslatePage),
            };
            if (ContentFrame.CurrentSourcePageType != targetPage)
            {
                ContentFrame.Navigate(targetPage);
            }
        }
    }

    private async void MainWindow_Closed(object sender, WindowEventArgs args)
    {
        if (_shuttingDown)
        {
            return;
        }
        _shuttingDown = true;
        args.Handled = true;

        try
        {
            _overlayWindow?.Close();
            _overlayWindow = null;
            if (_mainShellStarted)
            {
                await App.ViewModel.ShutdownAsync();
            }
        }
        finally
        {
            Close();
        }
    }
}
