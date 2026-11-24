using System.Text;
using LumaTranslate_WinUI.Services;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;

namespace LumaTranslate_WinUI.Views;

public sealed partial class SetupWizardPage : Page
{
    private readonly StringBuilder _logBuffer = new();
    private CancellationTokenSource? _installCts;
    private bool _autoTriggered;

    public SetupWizardPage()
    {
        InitializeComponent();
    }

    private async void Page_Loaded(object sender, RoutedEventArgs e)
    {
        ResolvedEnvironmentPaths env = EnvironmentBootstrapService.InspectEnvironment();
        EnvironmentStateText.Text = env.MissingReasonVi;
        SkipToAppBtn.Visibility = env.IsReady ? Visibility.Visible : Visibility.Collapsed;

        DetectedGpuInfo gpu = await EnvironmentBootstrapService.DetectGpuAsync();
        HardwareSummaryText.Text = gpu.HasNvidiaGpu
            ? $"GPU phát hiện: {gpu.GpuName} • VRAM trống {gpu.VramFreeMb}/{gpu.VramTotalMb} MB • Driver {gpu.DriverVersion}"
            : "Không phát hiện GPU NVIDIA (Trình cài đặt sẽ cấu hình chế độ CPU ONNX tự động)";

        if (!_autoTriggered && App.AutoRunSetupOnLaunch)
        {
            _autoTriggered = true;
            await RunInstallationFlowAsync();
        }
    }

    private async void StartInstallBtn_Click(object sender, RoutedEventArgs e)
    {
        await RunInstallationFlowAsync();
    }

    private async Task RunInstallationFlowAsync()
    {
        StartInstallBtn.IsEnabled = false;
        SkipToAppBtn.IsEnabled = false;
        UninstallFromSetupBtn.IsEnabled = false;
        DownloadModelsCheckBox.IsEnabled = false;
        ForceReinstallCheckBox.IsEnabled = false;
        CancelSetupBtn.IsEnabled = true;

        _logBuffer.Clear();
        InstallLogTextBlock.Text = string.Empty;
        _installCts = new CancellationTokenSource();

        bool downloadModels = DownloadModelsCheckBox.IsChecked == true;
        bool forceReinstall = ForceReinstallCheckBox.IsChecked == true;

        var progress = new Progress<SetupProgressUpdate>(update =>
        {
            InstallProgressBar.Value = update.Percent;
            PercentText.Text = $"{Math.Round(update.Percent)}%";
            CurrentStepText.Text = update.StepTitleVi;

            if (!string.IsNullOrWhiteSpace(update.LogLine))
            {
                if (_logBuffer.Length > 28000)
                {
                    _logBuffer.Remove(0, 8000);
                }
                _logBuffer.AppendLine(update.LogLine);
                InstallLogTextBlock.Text = _logBuffer.ToString();
                LogScrollViewer.ChangeView(null, LogScrollViewer.ScrollableHeight, null, disableAnimation: true);
            }
        });

        try
        {
            await EnvironmentBootstrapService.ProvisionEnvironmentAsync(
                downloadModels,
                forceReinstall,
                progress,
                _installCts.Token);

            if (App.ExitAfterSetup)
            {
                App.MainWindowInstance?.Close();
                return;
            }

            App.MainWindowInstance?.CompleteSetupAndLaunchMainShell();
        }
        catch (OperationCanceledException)
        {
            CurrentStepText.Text = "Đã hủy quá trình cài đặt.";
        }
        catch (Exception ex)
        {
            CurrentStepText.Text = $"Lỗi cài đặt: {ex.Message}";
            _logBuffer.AppendLine($"[ERROR] {ex}");
            InstallLogTextBlock.Text = _logBuffer.ToString();
        }
        finally
        {
            _installCts?.Dispose();
            _installCts = null;
            StartInstallBtn.IsEnabled = true;
            SkipToAppBtn.IsEnabled = true;
            UninstallFromSetupBtn.IsEnabled = true;
            DownloadModelsCheckBox.IsEnabled = true;
            ForceReinstallCheckBox.IsEnabled = true;
            CancelSetupBtn.IsEnabled = false;

            ResolvedEnvironmentPaths refreshed = EnvironmentBootstrapService.InspectEnvironment();
            EnvironmentStateText.Text = refreshed.MissingReasonVi;
            SkipToAppBtn.Visibility = refreshed.IsReady ? Visibility.Visible : Visibility.Collapsed;
        }
    }

    private void CancelSetupBtn_Click(object sender, RoutedEventArgs e)
    {
        _installCts?.Cancel();
    }

    private void SkipToAppBtn_Click(object sender, RoutedEventArgs e)
    {
        App.MainWindowInstance?.CompleteSetupAndLaunchMainShell();
    }

    private async void UninstallFromSetupBtn_Click(object sender, RoutedEventArgs e)
    {
        if (App.MainWindowInstance is not null)
        {
            await App.MainWindowInstance.TriggerUninstallFlowAsync();
        }
    }
}
