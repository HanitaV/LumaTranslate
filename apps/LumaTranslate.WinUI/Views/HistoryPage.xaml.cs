using LumaTranslate_WinUI.ViewModels;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;

namespace LumaTranslate_WinUI.Views;

public sealed partial class HistoryPage : Page
{
    public MainViewModel ViewModel => App.ViewModel;

    public HistoryPage()
    {
        InitializeComponent();
    }

    private async void Page_Loaded(object sender, RoutedEventArgs e)
    {
        await ViewModel.LoadHistoryAsync(SearchBox.Text);
    }

    private async void SearchBox_TextChanged(object sender, TextChangedEventArgs e)
    {
        await ViewModel.LoadHistoryAsync(SearchBox.Text);
    }

    private async void RefreshBtn_Click(object sender, RoutedEventArgs e)
    {
        await ViewModel.LoadHistoryAsync(SearchBox.Text);
    }

    private async void ClearHistoryBtn_Click(object sender, RoutedEventArgs e)
    {
        await ViewModel.ClearHistoryAsync();
    }
}
