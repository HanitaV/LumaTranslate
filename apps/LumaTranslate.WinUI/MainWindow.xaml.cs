using LumaTranslate_WinUI.Controls;
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
    private (ulong Hwnd, bool CaptureExcluded, int X, int Y, uint Width, uint Height)? _lastRegisteredOverlay;
    private CancellationTokenSource? _overlayBoundsDebounceCts;
    private bool _mainShellStarted;
    private bool _shuttingDown;
    private bool _uninstalling;

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
        var (_, _, vw, vh) = Win32WindowInterop.GetVirtualScreenBounds();
        int initW = Math.Min(1100, Math.Max(vw - 80, 860));
        int initH = Math.Min(780, Math.Max(vh - 80, 640));
        AppWindow.Resize(new SizeInt32(initW, initH));

        Closed += MainWindow_Closed;
    }

    public void StartInitialFlow()
    {
        if (App.LaunchUninstallMode)
        {
            _ = LaunchDirectUninstallModeAsync();
            return;
        }

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

    private async Task LaunchDirectUninstallModeAsync()
    {
        if (RootShellGrid.XamlRoot is null)
        {
            var tcs = new TaskCompletionSource();
            void OnLoaded(object sender, RoutedEventArgs e)
            {
                RootShellGrid.Loaded -= OnLoaded;
                tcs.TrySetResult();
            }
            RootShellGrid.Loaded += OnLoaded;
            await tcs.Task;
        }

        await TriggerUninstallFlowAsync(exitIfCancelled: true, defaultRemoveAppFolder: true);
    }

    public async Task TriggerUninstallFlowAsync(
        bool exitIfCancelled = false,
        bool defaultRemoveAppFolder = false)
    {
        if (_uninstalling || RootShellGrid.XamlRoot is null)
        {
            return;
        }

        UninstallOptions? options = await UninstallDialog.ShowAsync(
            RootShellGrid.XamlRoot,
            defaultRemoveAppFolder);

        if (options is null)
        {
            if (exitIfCancelled)
            {
                _shuttingDown = true;
                Close();
            }
            return;
        }

        _uninstalling = true;
        _overlayWindow?.HideOverlay();
        ShellNav.IsEnabled = false;
        SetupHostFrame.IsEnabled = false;
        UninstallBusyOverlay.Visibility = Visibility.Visible;

        var statusProgress = new Progress<string>(msg =>
        {
            DispatcherQueue.TryEnqueue(() => UninstallBusyStatusText.Text = msg);
        });

        try
        {
            await UninstallService.ExecuteUninstallAsync(
                options,
                gracefulShutdownAsync: async () =>
                {
                    _overlayBoundsDebounceCts?.Cancel();
                    _overlayBoundsDebounceCts?.Dispose();
                    _overlayBoundsDebounceCts = null;

                    DispatcherQueue.TryEnqueue(() =>
                    {
                        _overlayWindow?.Close();
                        _overlayWindow = null;
                    });

                    if (_mainShellStarted)
                    {
                        await App.ViewModel.ShutdownAsync();
                    }
                },
                statusProgress: statusProgress);
        }
        finally
        {
            _shuttingDown = true;
            Close();
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

            _overlayWindow.BoundsChangedByUser += (x, y, w, h) =>
            {
                App.ViewModel.UpdateOverlayBoundsFromUser(x, y, w, h);
                ScheduleDebouncedOverlayRegistration();
            };

            _overlayWindow.LockRequestedByUser += () =>
            {
                _ = App.ViewModel.SetOverlayMoveModeAsync(allowMove: false);
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
                    DispatcherQueue.TryEnqueue(() => _ = RegisterOverlayWithBackendAsync(force: true));
                }
                else
                {
                    DispatcherQueue.TryEnqueue(() => _lastRegisteredOverlay = null);
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

    private void ScheduleDebouncedOverlayRegistration()
    {
        _overlayBoundsDebounceCts?.Cancel();
        _overlayBoundsDebounceCts?.Dispose();
        var cts = new CancellationTokenSource();
        _overlayBoundsDebounceCts = cts;
        _ = Task.Run(async () =>
        {
            try
            {
                await Task.Delay(180, cts.Token).ConfigureAwait(false);
                DispatcherQueue.TryEnqueue(() => _ = RegisterOverlayWithBackendAsync());
            }
            catch (OperationCanceledException)
            {
            }
        });
    }

    private async Task RegisterOverlayWithBackendAsync(bool force = false)
    {
        if (_overlayWindow is null || !App.ViewModel.Ipc.IsConnected)
        {
            return;
        }
        try
        {
            OverlaySettingsDto o = App.ViewModel.Settings.Overlay;
            var registration = (
                Hwnd: _overlayWindow.HwndValue,
                CaptureExcluded: _overlayWindow.IsCaptureExcluded,
                o.X,
                o.Y,
                o.Width,
                o.Height);
            if (!force && _lastRegisteredOverlay == registration)
            {
                return;
            }
            _lastRegisteredOverlay = registration;
            await App.ViewModel.Ipc.RegisterOverlayWindowAsync(
                registration.Hwnd,
                registration.CaptureExcluded,
                registration.X,
                registration.Y,
                registration.Width,
                registration.Height);
        }
        catch
        {
            _lastRegisteredOverlay = null;
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
            _overlayBoundsDebounceCts?.Cancel();
            _overlayBoundsDebounceCts?.Dispose();
            _overlayBoundsDebounceCts = null;
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
