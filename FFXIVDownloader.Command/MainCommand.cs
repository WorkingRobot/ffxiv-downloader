using DotMake.CommandLine;

namespace FFXIVDownloader.Command;

[CliCommand]
public class MainCommand
{
    public static bool IsGHA;

    [CliOption(Required = false, Hidden = true, Name = "--gha")]
    public bool IsGithubActions { get; set; } = false;

    [CliOption(Required = false, Description = "Enables verbose logging.")]
    public bool Verbose { get; set; }

    [CliOption(Required = false, Description = "Enables debug logging. Implies verbose logging.")]
    public bool Debug { get; set; }

    [CliOption(Required = false, Description = "The file path from where to access patch data from.")]
    public string? PatchOverridePath { get; set; }

    public CancellationToken Init()
    {
#if DEBUG
        Log.IsVerboseEnabled = Log.IsDebugEnabled = true;
#else
        Log.IsVerboseEnabled = Debug || Verbose;
        Log.IsDebugEnabled = Debug;
#endif
        Log.Info($"Verbose: {Log.IsVerboseEnabled}; Debug: {Log.IsDebugEnabled}");

        PatchClient.OverridePath = PatchOverridePath;
        IsGHA = IsGithubActions;
        if (IsGHA)
            Log.Info("Running in CI/CD mode. o/");

        var cts = new CancellationTokenSource();
        Console.CancelKeyPress += (sender, eventArgs) =>
        {
            cts.Cancel();
            eventArgs.Cancel = true;
        };
        return cts.Token;
    }

    private static Task<int> Main(string[] args) =>
        Cli.RunAsync<MainCommand>(args, new CliSettings
        {
            //EnableDefaultExceptionHandler = true
        });
}
