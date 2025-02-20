using DotMake.CommandLine;

namespace FFXIVDownloader;

[CliCommand]
public class MainCommand
{
    private static async Task<int> Main(string[] args)
    {
#if DEBUG
        Log.IsVerboseEnabled = Log.IsDebugEnabled = true;
#endif

        try
        {
            return await Cli.RunAsync<MainCommand>(args).ConfigureAwait(false);
        }
        catch (Exception e)
        {
            Log.Error(e);
            return 1;
        }
    }
}