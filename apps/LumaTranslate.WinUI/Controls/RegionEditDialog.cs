using LumaTranslate_WinUI.Services;
using LumaTranslate_WinUI.ViewModels;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;

namespace LumaTranslate_WinUI.Controls;

public static class RegionEditDialog
{
    public static async Task<CaptureRegionDto?> ShowAsync(
        XamlRoot xamlRoot,
        CaptureRegionDto initial,
        IReadOnlyList<MonitorInfoDto> monitors,
        bool isNew)
    {
        var nameBox = new TextBox
        {
            Header = "Tên vùng theo dõi",
            Text = initial.Name,
            PlaceholderText = "Ví dụ: Hộp thoại nhân vật",
        };
        AutomationProperties.SetAutomationId(nameBox, "RegionDialogNameBox");

        var langCombo = new ComboBox
        {
            Header = "Ngôn ngữ nguồn cần nhận dạng & dịch sang tiếng Việt",
            HorizontalAlignment = HorizontalAlignment.Stretch,
            ItemsSource = MainViewModel.SupportedLanguages,
        };
        AutomationProperties.SetAutomationId(langCombo, "RegionDialogLanguageCombo");
        langCombo.SelectedItem = MainViewModel.SupportedLanguages
            .FirstOrDefault(l => string.Equals(l.Code, initial.SourceLanguage, StringComparison.OrdinalIgnoreCase))
            ?? MainViewModel.SupportedLanguages[0];

        var xBox = new NumberBox { Header = "Tọa độ X (px)", Value = initial.X, SpinButtonPlacementMode = NumberBoxSpinButtonPlacementMode.Compact };
        var yBox = new NumberBox { Header = "Tọa độ Y (px)", Value = initial.Y, SpinButtonPlacementMode = NumberBoxSpinButtonPlacementMode.Compact };
        var wBox = new NumberBox { Header = "Chiều rộng (px)", Value = initial.Width, Minimum = 16, SpinButtonPlacementMode = NumberBoxSpinButtonPlacementMode.Compact };
        var hBox = new NumberBox { Header = "Chiều cao (px)", Value = initial.Height, Minimum = 16, SpinButtonPlacementMode = NumberBoxSpinButtonPlacementMode.Compact };

        AutomationProperties.SetAutomationId(xBox, "RegionDialogXBox");
        AutomationProperties.SetAutomationId(yBox, "RegionDialogYBox");
        AutomationProperties.SetAutomationId(wBox, "RegionDialogWidthBox");
        AutomationProperties.SetAutomationId(hBox, "RegionDialogHeightBox");

        var coordsGrid = new Grid { ColumnSpacing = 12, RowSpacing = 12 };
        coordsGrid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        coordsGrid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        coordsGrid.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        coordsGrid.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });

        Grid.SetRow(xBox, 0); Grid.SetColumn(xBox, 0);
        Grid.SetRow(yBox, 0); Grid.SetColumn(yBox, 1);
        Grid.SetRow(wBox, 1); Grid.SetColumn(wBox, 0);
        Grid.SetRow(hBox, 1); Grid.SetColumn(hBox, 1);
        coordsGrid.Children.Add(xBox);
        coordsGrid.Children.Add(yBox);
        coordsGrid.Children.Add(wBox);
        coordsGrid.Children.Add(hBox);

        var panel = new StackPanel { Spacing = 14, MinWidth = 380 };
        panel.Children.Add(nameBox);
        panel.Children.Add(langCombo);
        panel.Children.Add(coordsGrid);

        var dialog = new ContentDialog
        {
            XamlRoot = xamlRoot,
            Title = isNew ? "Thêm vùng theo dõi mới" : "Chỉnh sửa vùng theo dõi",
            Content = panel,
            PrimaryButtonText = "Lưu vùng",
            CloseButtonText = "Hủy",
            DefaultButton = ContentDialogButton.Primary,
        };

        ContentDialogResult res = await dialog.ShowAsync();
        if (res != ContentDialogResult.Primary)
        {
            return null;
        }

        string selectedLang = (langCombo.SelectedItem as LanguageOption)?.Code ?? "auto";
        int rx = double.IsNaN(xBox.Value) ? initial.X : (int)Math.Round(xBox.Value);
        int ry = double.IsNaN(yBox.Value) ? initial.Y : (int)Math.Round(yBox.Value);
        uint rw = double.IsNaN(wBox.Value) ? initial.Width : (uint)Math.Max(16, Math.Round(wBox.Value));
        uint rh = double.IsNaN(hBox.Value) ? initial.Height : (uint)Math.Max(16, Math.Round(hBox.Value));

        string monId = initial.MonitorId;
        if (string.IsNullOrWhiteSpace(monId) && monitors.Count > 0)
        {
            monId = monitors.FirstOrDefault(m => m.IsPrimary)?.Id ?? monitors[0].Id;
        }

        return new CaptureRegionDto
        {
            Id = string.IsNullOrWhiteSpace(initial.Id) ? $"region-{Guid.NewGuid():N}"[..15] : initial.Id,
            Name = string.IsNullOrWhiteSpace(nameBox.Text) ? "Vùng phụ đề" : nameBox.Text.Trim(),
            MonitorId = monId,
            X = rx,
            Y = ry,
            Width = rw,
            Height = rh,
            DpiScale = initial.DpiScale,
            SourceLanguage = selectedLang,
            Enabled = initial.Enabled,
        };
    }
}
