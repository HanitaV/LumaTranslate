using LumaTranslate_WinUI.Services;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;

namespace LumaTranslate_WinUI.Controls;

public static class UninstallDialog
{
    public static async Task<UninstallOptions?> ShowAsync(
        XamlRoot xamlRoot,
        bool defaultRemoveAppFolder = false)
    {
        UninstallInspection quick = UninstallService.InspectQuick();
        Brush? secondaryBrush = Application.Current.Resources.TryGetValue("TextFillColorSecondaryBrush", out object? res)
            ? res as Brush
            : null;

        var introText = new TextBlock
        {
            Text = "Chọn các thành phần bạn muốn gỡ bỏ khỏi máy tính. LumaTranslate sẽ tự động dừng tiến trình nền, đóng các tác vụ AI và thoát ứng dụng sau khi dọn dẹp xong.",
            TextWrapping = TextWrapping.Wrap,
            FontSize = 13,
        };
        if (secondaryBrush is not null)
        {
            introText.Foreground = secondaryBrush;
        }

        static string FormatSizeTag(long bytes) =>
            bytes < 0 ? "Đang tính dung lượng..." : UninstallService.FormatBytesVi(bytes);

        var aiCheck = new CheckBox
        {
            Content = $"Xóa môi trường Python 3.12 & mô hình AI cục bộ ({FormatSizeTag(quick.AiEnvironmentBytes)})",
            IsChecked = true,
        };
        AutomationProperties.SetAutomationId(aiCheck, "UninstallAiEnvCheckBox");

        var aiCaption = CreateCaption(
            $"Bao gồm PaddleOCR PP-OCRv5, VieNeu-TTS v3 Turbo và công cụ uv trong {quick.AppDataRoot}",
            secondaryBrush);

        var dataCheck = new CheckBox
        {
            Content = $"Xóa cài đặt, khóa API, bộ nhớ đệm & lịch sử hội thoại ({FormatSizeTag(quick.UserDataBytes)})",
            IsChecked = true,
        };
        AutomationProperties.SetAutomationId(dataCheck, "UninstallUserDataCheckBox");

        var dataCaption = CreateCaption(
            $"Thư mục dữ liệu cá nhân: {Path.Combine(quick.AppDataRoot, "data")}",
            secondaryBrush);

        var shortcutsCheck = new CheckBox
        {
            Content = "Xóa lối tắt (Shortcut) & thông tin đăng ký ứng dụng trong Windows",
            IsChecked = true,
        };
        AutomationProperties.SetAutomationId(shortcutsCheck, "UninstallShortcutsCheckBox");

        var appFolderCheck = new CheckBox
        {
            Content = "Xóa luôn thư mục bộ cài ứng dụng hiện tại sau khi thoát",
            IsEnabled = quick.CanDeleteAppFolder,
            IsChecked = quick.CanDeleteAppFolder && defaultRemoveAppFolder,
        };
        AutomationProperties.SetAutomationId(appFolderCheck, "UninstallAppFolderCheckBox");

        var appFolderCaption = CreateCaption(
            quick.CanDeleteAppFolder
                ? $"Thư mục ứng dụng sẽ được xóa: {quick.AppBaseDirectory}"
                : quick.CannotDeleteAppFolderReasonVi ?? "Không thể xóa thư mục ứng dụng hiện tại.",
            secondaryBrush);

        var panel = new StackPanel
        {
            Spacing = 10,
            MinWidth = 440,
            MaxWidth = 520,
        };

        panel.Children.Add(introText);
        panel.Children.Add(BuildOptionGroup(aiCheck, aiCaption));
        panel.Children.Add(BuildOptionGroup(dataCheck, dataCaption));
        panel.Children.Add(shortcutsCheck);
        panel.Children.Add(BuildOptionGroup(appFolderCheck, appFolderCaption));

        var dialog = new ContentDialog
        {
            XamlRoot = xamlRoot,
            Title = "Gỡ cài đặt LumaTranslate",
            Content = panel,
            PrimaryButtonText = "Gỡ cài đặt & Thoát",
            CloseButtonText = "Hủy",
            DefaultButton = ContentDialogButton.Close,
        };

        void UpdatePrimaryButtonState(object sender, RoutedEventArgs e)
        {
            dialog.IsPrimaryButtonEnabled =
                aiCheck.IsChecked == true ||
                dataCheck.IsChecked == true ||
                shortcutsCheck.IsChecked == true ||
                (appFolderCheck.IsEnabled && appFolderCheck.IsChecked == true);
        }

        aiCheck.Checked += UpdatePrimaryButtonState;
        aiCheck.Unchecked += UpdatePrimaryButtonState;
        dataCheck.Checked += UpdatePrimaryButtonState;
        dataCheck.Unchecked += UpdatePrimaryButtonState;
        shortcutsCheck.Checked += UpdatePrimaryButtonState;
        shortcutsCheck.Unchecked += UpdatePrimaryButtonState;
        appFolderCheck.Checked += UpdatePrimaryButtonState;
        appFolderCheck.Unchecked += UpdatePrimaryButtonState;

        using var sizeCts = new CancellationTokenSource();
        if (quick.AiEnvironmentBytes < 0 || quick.UserDataBytes < 0)
        {
            var dispatcher = dialog.DispatcherQueue;
            _ = Task.Run(async () =>
            {
                try
                {
                    UninstallInspection full = await UninstallService.InspectAsync(ct: sizeCts.Token).ConfigureAwait(false);
                    dispatcher.TryEnqueue(() =>
                    {
                        aiCheck.Content = $"Xóa môi trường Python 3.12 & mô hình AI cục bộ ({UninstallService.FormatBytesVi(full.AiEnvironmentBytes)})";
                        dataCheck.Content = $"Xóa cài đặt, khóa API, bộ nhớ đệm & lịch sử hội thoại ({UninstallService.FormatBytesVi(full.UserDataBytes)})";
                    });
                }
                catch (OperationCanceledException)
                {
                }
            });
        }

        ContentDialogResult result;
        try
        {
            result = await dialog.ShowAsync();
        }
        finally
        {
            sizeCts.Cancel();
        }

        if (result != ContentDialogResult.Primary)
        {
            return null;
        }

        return new UninstallOptions(
            RemoveAiEnvironment: aiCheck.IsChecked == true,
            RemoveUserData: dataCheck.IsChecked == true,
            RemoveShortcutsAndRegistry: shortcutsCheck.IsChecked == true,
            RemoveAppFolder: quick.CanDeleteAppFolder && appFolderCheck.IsChecked == true);
    }

    private static StackPanel BuildOptionGroup(CheckBox checkBox, TextBlock caption)
    {
        var group = new StackPanel { Spacing = 2 };
        group.Children.Add(checkBox);
        group.Children.Add(caption);
        return group;
    }

    private static TextBlock CreateCaption(string text, Brush? foreground)
    {
        var block = new TextBlock
        {
            Text = text,
            FontSize = 12,
            Margin = new Thickness(28, 0, 0, 0),
            TextWrapping = TextWrapping.Wrap,
        };
        if (foreground is not null)
        {
            block.Foreground = foreground;
        }
        return block;
    }
}
