using System.Diagnostics.CodeAnalysis;
using System.Runtime.CompilerServices;
using System.Text.Json;
using FFXIVDownloader.Thaliak;

namespace FFXIVDownloader;

public static class Log
{
    public static void Error(Exception e)
    {
        Console.Error.WriteLine($"[ERROR] {e}");
    }

    public static void Warn(string message)
    {
        Console.WriteLine($"[WARN] {message}");
    }

    public static void Info(string message)
    {
        Console.WriteLine($"[INFO] {message}");
    }

    //

    public static bool IsVerboseEnabled;
    public static void Verbose(DefaultInterpolatedStringHandler message)
    {
        if (IsVerboseEnabled)
            Console.WriteLine($"[VERBOSE] {message.ToStringAndClear()}");
    }

    public static void Verbose(string message)
    {
        if (IsVerboseEnabled)
            Console.WriteLine($"[VERBOSE] {message}");
    }

    public static void Verbose()
    {
        Verbose(string.Empty);
    }

    //

    public static bool IsDebugEnabled;
    public static void Debug(DefaultInterpolatedStringHandler handler)
    {
        if (IsDebugEnabled)
            Console.WriteLine($"[DEBUG] {handler.ToStringAndClear()}");
    }

    public static void Debug(string message)
    {
        if (IsDebugEnabled)
            Console.WriteLine($"[DEBUG] {message}");
    }

    private static JsonSerializerOptions JsonOptions { get; } = new() { WriteIndented = true, IncludeFields = true };

    [RequiresDynamicCode("JSON serialization and deserialization might require types that cannot be statically analyzed and might need runtime code generation. Use System.Text.Json source generation for native AOT applications.")]
    [RequiresUnreferencedCode("JSON serialization and deserialization might require types that cannot be statically analyzed. Use the overload that takes a JsonTypeInfo or JsonSerializerContext, or make sure all of the required types are preserved.")]
    public static void DebugObject<T>(T value)
    {
        if (IsDebugEnabled)
            Console.WriteLine($"[DEBUG] {JsonSerializer.Serialize(value, JsonOptions)}");
    }

    //

    public static void Output(string message)
    {
        Console.WriteLine($"[OUTPUT] {message}");
    }

    public static void Output(object obj)
    {
        Output(obj.ToString() ?? "null");
    }

    //

    public static void CIOutput(string key, string value) =>
        WriteToEnv("GITHUB_OUTPUT", $"{key}={value}");

    private static void WriteToEnv(string variable, string text)
    {
        if (Environment.GetEnvironmentVariable(variable) is { } file)
        {
            using var writer = new StreamWriter(file);
            writer.WriteLine(text);
        }
        else
            Output($"{variable} => {text}");
    }
}
