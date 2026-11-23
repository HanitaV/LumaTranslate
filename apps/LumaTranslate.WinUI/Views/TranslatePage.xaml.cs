using LumaTranslate_WinUI.Controls;
using LumaTranslate_WinUI.Overlays;
using LumaTranslate_WinUI.Services;
using LumaTranslate_WinUI.ViewModels;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Windows.Graphics;

namespace LumaTranslate_WinUI.Views;

public sealed partial class TranslatePage : Page
{
    public MainViewModel ViewModel => App.ViewModel;

    public TranslatePage()
    {
        InitializeComponent();
    }

    private async void SelectRegionBtn_Click(object sender, RoutedEventArgs e)
    {
        var selector = new RegionSelectorWindow();
        RectInt32? picked = await selector.PickRegionAsync();
        if (picked is null)
        {
            return;
        }

        int nextIndex = ViewModel.Regions.Count + 1;
        string monId = ViewModel.Monitors.FirstOrDefault(m => m.IsPrimary)?.Id
            ?? ViewModel.Monitors.FirstOrDefault()?.Id
            ?? string.Empty;

        var newRegion = new CaptureRegionDto
        {
            Id = $"region-{Guid.NewGuid():N}"[..15],
            Name = $"Vùng {nextIndex}",
            MonitorId = monId,
            X = picked.Value.X,
            Y = picked.Value.Y,
            Width = (uint)picked.Value.Width,
            Height = (uint)picked.Value.Height,
            DpiScale = 1.0,
            SourceLanguage = ViewModel.DefaultNewRegionLanguage,
            Enabled = true,
        };

        await ViewModel.AddOrUpdateRegionAsync(newRegion);
    }

    private async void AddManualRegionBtn_Click(object sender, RoutedEventArgs e)
    {
        int nextIndex = ViewModel.Regions.Count + 1;
        var seed = new CaptureRegionDto
        {
            Id = $"region-{Guid.NewGuid():N}"[..15],
            Name = $"Vùng {nextIndex}",
            X = 200,
            Y = 720,
            Width = 900,
            Height = 140,
            SourceLanguage = ViewModel.DefaultNewRegionLanguage,
            Enabled = true,
        };

        CaptureRegionDto? edited = await RegionEditDialog.ShowAsync(
            XamlRoot,
            seed,
            ViewModel.Monitors,
            isNew: true);
        if (edited is not null)
        {
            await ViewModel.AddOrUpdateRegionAsync(edited);
        }
    }

    private async void StartStopBtn_Click(object sender, RoutedEventArgs e)
    {
        await ViewModel.ToggleMonitoringAsync();
    }

    private void ShowOverlayToggle_Toggled(object sender, RoutedEventArgs e)
    {
        App.MainWindowInstance?.SetSubtitleOverlayVisibility(ShowOverlayToggle.IsChecked == true);
    }

    private async void GrantConsentBtn_Click(object sender, RoutedEventArgs e)
    {
        await ViewModel.GrantCloudConsentAsync();
    }

    private void StatusBanner_Closed(InfoBar sender, InfoBarClosedEventArgs args)
    {
        ViewModel.ClearBanner();
    }

    private async void RegionEnabledCheckBox_Click(object sender, RoutedEventArgs e)
    {
        if (sender is CheckBox cb && cb.Tag is string regionId)
        {
            await ViewModel.SetRegionEnabledAsync(regionId, cb.IsChecked == true);
        }
    }

    private async void EditRegionBtn_Click(object sender, RoutedEventArgs e)
    {
        if (sender is Button btn && btn.Tag is string regionId)
        {
            RegionItemViewModel? item = ViewModel.Regions.FirstOrDefault(r => r.Id == regionId);
            if (item is null)
            {
                return;
            }
            CaptureRegionDto? updated = await RegionEditDialog.ShowAsync(
                XamlRoot,
                item.Dto,
                ViewModel.Monitors,
                isNew: false);
            if (updated is not null)
            {
                await ViewModel.AddOrUpdateRegionAsync(updated);
            }
        }
    }

    private async void DeleteRegionBtn_Click(object sender, RoutedEventArgs e)
    {
        if (sender is Button btn && btn.Tag is string regionId)
        {
            await ViewModel.DeleteRegionAsync(regionId);
        }
    }
}
