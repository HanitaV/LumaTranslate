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

        var seed = new CaptureRegionDto
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

        // Let the user confirm/choose the region's source language & name right after dragging!
        CaptureRegionDto? confirmed = await RegionEditDialog.ShowAsync(
            XamlRoot,
            seed,
            ViewModel.Monitors,
            isNew: true);
        if (confirmed is not null)
        {
            ViewModel.DefaultNewRegionLanguage = confirmed.SourceLanguage;
            await ViewModel.AddOrUpdateRegionAsync(confirmed);
        }
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
            ViewModel.DefaultNewRegionLanguage = edited.SourceLanguage;
            await ViewModel.AddOrUpdateRegionAsync(edited);
        }
    }

    private async void StartStopBtn_Click(object sender, RoutedEventArgs e)
    {
        await ViewModel.ToggleMonitoringAsync();
    }

    private void ShowOverlayToggle_Click(object sender, RoutedEventArgs e)
    {
        App.MainWindowInstance?.SetSubtitleOverlayVisibility(ShowOverlayToggle.IsChecked == true);
    }

    private async void MoveOverlayToggle_Click(object sender, RoutedEventArgs e)
    {
        bool nextAllowMove = !ViewModel.IsOverlayMoveMode;
        await ViewModel.SetOverlayMoveModeAsync(nextAllowMove);
    }

    private async void MoveOverlayQuickBtn_Click(object sender, RoutedEventArgs e)
    {
        await ViewModel.SetOverlayMoveModeAsync(allowMove: true);
    }

    private async void QuickTtsToggle_Click(object sender, RoutedEventArgs e)
    {
        await ViewModel.SetTtsEnabledQuickAsync(!ViewModel.IsTtsEnabled);
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

    private async void RegionLanguageCombo_SelectionChanged(object sender, SelectionChangedEventArgs e)
    {
        if (sender is ComboBox combo
            && combo.Tag is string regionId
            && combo.SelectedItem is LanguageOption selected)
        {
            await ViewModel.SetRegionLanguageAsync(regionId, selected.Code);
        }
    }

    private async void RepickRegionBtn_Click(object sender, RoutedEventArgs e)
    {
        if (sender is Button btn && btn.Tag is string regionId)
        {
            RegionItemViewModel? item = ViewModel.Regions.FirstOrDefault(r => r.Id == regionId);
            if (item is null)
            {
                return;
            }

            var selector = new RegionSelectorWindow();
            RectInt32? picked = await selector.PickRegionAsync();
            if (picked is null)
            {
                return;
            }

            CaptureRegionDto prev = item.Dto;
            var updated = new CaptureRegionDto
            {
                Id = prev.Id,
                Name = prev.Name,
                MonitorId = prev.MonitorId,
                X = picked.Value.X,
                Y = picked.Value.Y,
                Width = (uint)picked.Value.Width,
                Height = (uint)picked.Value.Height,
                DpiScale = prev.DpiScale,
                SourceLanguage = prev.SourceLanguage,
                Enabled = prev.Enabled,
            };
            await ViewModel.AddOrUpdateRegionAsync(updated);
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
