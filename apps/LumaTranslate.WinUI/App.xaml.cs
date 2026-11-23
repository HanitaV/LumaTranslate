using LumaTranslate_WinUI.ViewModels;
using Microsoft.UI.Xaml;

namespace LumaTranslate_WinUI;

public partial class App : Application
{
    public static MainViewModel ViewModel { get; } = new();
    public static MainWindow? MainWindowInstance { get; private set; }

    public static bool ForceSetupWizard { get; private set; }
    public static bool AutoRunSetupOnLaunch { get; private set; }
    public static bool ExitAfterSetup { get; private set; }

    public App()
    {
        InitializeComponent();

        string[] cmdArgs = Environment.GetCommandLineArgs();
        foreach (string arg in cmdArgs.Skip(1))
        {
            switch (arg.Trim().ToLowerInvariant())
            {
                case "--setup":
                case "--install":
                case "--install-env":
                    ForceSetupWizard = true;
                    break;
                case "--auto":
                case "--auto-install":
                case "--accept-all":
                    ForceSetupWizard = true;
                    AutoRunSetupOnLaunch = true;
                    break;
                case "--exit-after-setup":
                    ExitAfterSetup = true;
                    break;
            }
        }

        UnhandledException += (_, e) =>
        {
            try
            {
                string dir = Path.Combine(
                    Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),
                    "LumaTranslate");
                Directory.CreateDirectory(dir);
                File.AppendAllText(
                    Path.Combine(dir, "ui-errors.log"),
                    $"[{DateTimeOffset.Now:O}] {e.Exception}\n");
            }
            catch
            {
            }
        };
    }

    protected override void OnLaunched(LaunchActivatedEventArgs args)
    {
        MainWindowInstance = new MainWindow();
        MainWindowInstance.Activate();
        MainWindowInstance.StartInitialFlow();
    }
}
