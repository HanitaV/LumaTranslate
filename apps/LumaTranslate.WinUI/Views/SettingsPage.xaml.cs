using LumaTranslate_WinUI.Services;
using LumaTranslate_WinUI.ViewModels;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;

namespace LumaTranslate_WinUI.Views;

public sealed partial class SettingsPage : Page
{
    public MainViewModel ViewModel => App.ViewModel;

    public SettingsPage()
    {
        InitializeComponent();
        DefaultLangCombo.ItemsSource = MainViewModel.SupportedLanguages;
        ProviderCombo.ItemsSource = MainViewModel.SupportedProviders;
        TtsModeCombo.ItemsSource = MainViewModel.SupportedTtsModes;
    }

    private void Page_Loaded(object sender, RoutedEventArgs e)
    {
        AppSettingsDto s = ViewModel.Settings;

        DefaultLangCombo.SelectedItem = MainViewModel.SupportedLanguages
            .FirstOrDefault(l => l.Code == ViewModel.DefaultNewRegionLanguage)
            ?? MainViewModel.SupportedLanguages[1];

        IntervalNumberBox.Value = s.MonitoringIntervalMs;
        CloudConsentCheckBox.IsChecked = s.CloudConsentGranted;

        ProviderCombo.SelectedItem = MainViewModel.SupportedProviders
            .FirstOrDefault(p => p.Id == s.ActiveProvider)
            ?? MainViewModel.SupportedProviders[0];

        MyMemoryEmailBox.Text = s.Credentials.MyMemoryEmail ?? string.Empty;
        AzureKeyBox.Password = s.Credentials.AzureApiKey ?? string.Empty;
        AzureRegionBox.Text = s.Credentials.AzureRegion ?? string.Empty;
        GoogleKeyBox.Password = s.Credentials.GoogleApiKey ?? string.Empty;

        FallbackMyMemoryCheck.IsChecked = s.FallbackProviders.Contains("mymemory");
        FallbackAzureCheck.IsChecked = s.FallbackProviders.Contains("azure");
        FallbackGoogleCheck.IsChecked = s.FallbackProviders.Contains("google");

        TtsAutoSwitch.IsOn = s.TtsEnabled;
        VolumeSlider.Value = Math.Round(s.TtsVolume * 100.0);
        TtsModeCombo.SelectedItem = MainViewModel.SupportedTtsModes
            .FirstOrDefault(m => m.Id == s.TtsExecutionMode)
            ?? MainViewModel.SupportedTtsModes[0];

        OverlayFontSizeBox.Value = s.Overlay.FontSize;
        OverlayOpacityBox.Value = Math.Round(s.Overlay.Opacity * 100.0);
        OverlayXBox.Value = s.Overlay.X;
        OverlayYBox.Value = s.Overlay.Y;
        ClickThroughSwitch.IsOn = s.Overlay.LockedClickThrough;
    }

    private AppSettingsDto CollectSettingsFromForm()
    {
        var fallbacks = new List<string>();
        if (FallbackMyMemoryCheck.IsChecked == true) fallbacks.Add("mymemory");
        if (FallbackAzureCheck.IsChecked == true) fallbacks.Add("azure");
        if (FallbackGoogleCheck.IsChecked == true) fallbacks.Add("google");

        string providerId = (ProviderCombo.SelectedItem as ProviderOption)?.Id ?? "mymemory";
        string ttsMode = (TtsModeCombo.SelectedItem as TtsModeOption)?.Id ?? "auto";

        return new AppSettingsDto
        {
            MonitoringIntervalMs = double.IsNaN(IntervalNumberBox.Value)
                ? 200ul
                : (ulong)Math.Clamp(Math.Round(IntervalNumberBox.Value), 100.0, 2000.0),
            ActiveProvider = providerId,
            FallbackProviders = fallbacks,
            CloudConsentGranted = CloudConsentCheckBox.IsChecked == true,
            Credentials = new ProviderCredentialsDto
            {
                MyMemoryEmail = NullIfBlank(MyMemoryEmailBox.Text),
                AzureApiKey = NullIfBlank(AzureKeyBox.Password),
                AzureRegion = NullIfBlank(AzureRegionBox.Text),
                AzureEndpoint = ViewModel.Settings.Credentials.AzureEndpoint,
                GoogleApiKey = NullIfBlank(GoogleKeyBox.Password),
            },
            TtsEnabled = TtsAutoSwitch.IsOn,
            TtsVolume = (float)Math.Clamp(VolumeSlider.Value / 100.0, 0.0, 1.5),
            TtsExecutionMode = ttsMode,
            Overlay = new OverlaySettingsDto
            {
                FontSize = double.IsNaN(OverlayFontSizeBox.Value) ? 24.0 : Math.Clamp(OverlayFontSizeBox.Value, 14.0, 56.0),
                Opacity = double.IsNaN(OverlayOpacityBox.Value) ? 0.85 : Math.Clamp(OverlayOpacityBox.Value / 100.0, 0.30, 1.0),
                X = double.IsNaN(OverlayXBox.Value) ? ViewModel.Settings.Overlay.X : (int)Math.Round(OverlayXBox.Value),
                Y = double.IsNaN(OverlayYBox.Value) ? ViewModel.Settings.Overlay.Y : (int)Math.Round(OverlayYBox.Value),
                Width = ViewModel.Settings.Overlay.Width,
                Height = ViewModel.Settings.Overlay.Height,
                LockedClickThrough = ClickThroughSwitch.IsOn,
            },
        };
    }

    private static string? NullIfBlank(string? s) =>
        string.IsNullOrWhiteSpace(s) ? null : s.Trim();

    private async void TestProviderBtn_Click(object sender, RoutedEventArgs e)
    {
        TestProviderBtn.IsEnabled = false;
        TestResultText.Text = "Đang kiểm tra kết nối...";
        try
        {
            AppSettingsDto draft = CollectSettingsFromForm();
            var (_, msgVi) = await ViewModel.TestProviderConnectionAsync(draft.ActiveProvider, draft);
            TestResultText.Text = msgVi;
        }
        finally
        {
            TestProviderBtn.IsEnabled = true;
        }
    }

    private async void SaveSettingsBtn_Click(object sender, RoutedEventArgs e)
    {
        if (DefaultLangCombo.SelectedItem is LanguageOption lang)
        {
            ViewModel.DefaultNewRegionLanguage = lang.Code;
        }
        AppSettingsDto updated = CollectSettingsFromForm();
        await ViewModel.SaveSettingsAsync(updated, "Đã lưu cài đặt thành công.");
    }

    private async void ClearCacheBtn_Click(object sender, RoutedEventArgs e)
    {
        await ViewModel.ClearCacheAsync();
    }

    private async void ClearHistoryBtn_Click(object sender, RoutedEventArgs e)
    {
        await ViewModel.ClearHistoryAsync();
    }

    private void OpenSetupWizardBtn_Click(object sender, RoutedEventArgs e)
    {
        App.MainWindowInstance?.OpenSetupWizard();
    }
}
