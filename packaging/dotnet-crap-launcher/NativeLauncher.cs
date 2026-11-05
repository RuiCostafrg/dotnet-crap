using System.ComponentModel;
using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Runtime.Versioning;

namespace DotnetCrap.Launcher;

/// <summary>
/// Starts the native dotnet-crap executable bundled under <c>native/&lt;rid&gt;/</c>.
/// The installed file is never modified. On Unix, a native file without any execute
/// bit (NuGet extraction does not preserve Unix modes) is copied to a private,
/// owner-only directory for this invocation, executed from there, and removed.
/// </summary>
public static class NativeLauncher
{
    private const UnixFileMode AnyExecute =
        UnixFileMode.UserExecute | UnixFileMode.GroupExecute | UnixFileMode.OtherExecute;
    private const UnixFileMode PrivateFileMode =
        UnixFileMode.UserRead | UnixFileMode.UserWrite | UnixFileMode.UserExecute;
    private const UnixFileMode PrivateDirectoryMode =
        UnixFileMode.UserRead | UnixFileMode.UserWrite | UnixFileMode.UserExecute;

    public static string? CurrentRuntimeIdentifier()
    {
        if (OperatingSystem.IsWindows())
        {
            return RuntimeInformation.ProcessArchitecture == Architecture.X64 ? "win-x64" : null;
        }
        if (OperatingSystem.IsMacOS())
        {
            return RuntimeInformation.ProcessArchitecture switch
            {
                Architecture.X64 => "osx-x64",
                Architecture.Arm64 => "osx-arm64",
                _ => null,
            };
        }
        if (OperatingSystem.IsLinux())
        {
            return RuntimeInformation.ProcessArchitecture switch
            {
                Architecture.X64 => "linux-x64",
                Architecture.Arm64 => "linux-arm64",
                _ => null,
            };
        }
        return null;
    }

    public static string ExecutableName(string rid) => rid == "win-x64" ? "dotnet-crap.exe" : "dotnet-crap";

    public static string NativeExecutablePath(string appBaseDirectory, string rid) =>
        Path.Combine(appBaseDirectory, "native", rid, ExecutableName(rid));

    /// <summary>
    /// Runs the native executable with <paramref name="arguments"/> passed through unchanged and
    /// returns its exit code. Launcher-side failures return 2 and are written to <paramref name="error"/>.
    /// </summary>
    /// <param name="temporaryRoot">Parent for the private copy; defaults to <see cref="Path.GetTempPath"/>.</param>
    public static async Task<int> RunAsync(
        string appBaseDirectory,
        string rid,
        IReadOnlyList<string> arguments,
        TextWriter error,
        string? temporaryRoot = null)
    {
        try
        {
            return await RunCoreAsync(appBaseDirectory, rid, arguments, error, temporaryRoot);
        }
        catch (Exception exception) when (exception is IOException or UnauthorizedAccessException or Win32Exception)
        {
            error.WriteLine($"dotnet-crap could not start its native binary: {exception.Message}");
            return 2;
        }
    }

    private static async Task<int> RunCoreAsync(string appBaseDirectory, string rid,
        IReadOnlyList<string> arguments, TextWriter error, string? temporaryRoot)
    {
        var installed = NativeExecutablePath(appBaseDirectory, rid);
        if (!File.Exists(installed))
        {
            error.WriteLine($"dotnet-crap native binary for {rid} is missing from this installation.");
            return 2;
        }

        // Windows executables are launched as-is, unchanged from earlier releases.
        if (OperatingSystem.IsWindows())
        {
            return (await StartAsync(installed, arguments, error)).ExitCode;
        }

        // On Unix, try the installed file first when any execute bit is set. Whether this user may
        // execute it depends on ownership, so the kernel decides: EACCES falls through to a private copy.
        if ((File.GetUnixFileMode(installed) & AnyExecute) != 0)
        {
            var inPlace = await StartAsync(installed, arguments, error);
            if (!inPlace.PermissionDenied)
            {
                return inPlace.ExitCode;
            }
        }

        return await RunPrivateCopyAsync(installed, ExecutableName(rid), arguments, error, temporaryRoot);
    }

    [UnsupportedOSPlatform("windows")]
    private static async Task<int> RunPrivateCopyAsync(
        string installed,
        string fileName,
        IReadOnlyList<string> arguments,
        TextWriter error,
        string? temporaryRoot)
    {
        string? privateDirectory = null;
        try
        {
            privateDirectory = CreatePrivateDirectory(temporaryRoot ?? Path.GetTempPath());
            var copy = Path.Combine(privateDirectory, fileName);
            File.Copy(installed, copy, overwrite: false);
            File.SetUnixFileMode(copy, PrivateFileMode);

            // Ctrl+C reaches the child through the terminal. Keep the launcher alive until the child
            // exits so the private copy is removed and the child's exit code is returned.
            ConsoleCancelEventHandler keepRunning = (_, eventArgs) => eventArgs.Cancel = true;
            Console.CancelKeyPress += keepRunning;
            StartOutcome outcome;
            try
            {
                outcome = await StartAsync(copy, arguments, error);
            }
            finally
            {
                Console.CancelKeyPress -= keepRunning;
            }

            if (outcome.PermissionDenied)
            {
                error.WriteLine("dotnet-crap could not execute its private native copy: permission denied.");
                return 2;
            }
            return outcome.ExitCode;
        }
        catch (Exception exception) when (exception is IOException or UnauthorizedAccessException)
        {
            error.WriteLine($"dotnet-crap could not prepare its native binary for execution: {exception.Message}");
            return 2;
        }
        finally
        {
            if (privateDirectory is not null)
            {
                RemovePrivateDirectory(privateDirectory, error);
            }
        }
    }

    private const int UnixPermissionDenied = 13; // EACCES

    private readonly record struct StartOutcome(int ExitCode, bool PermissionDenied);

    private static async Task<StartOutcome> StartAsync(string executable, IReadOnlyList<string> arguments, TextWriter error)
    {
        var start = new ProcessStartInfo(executable) { UseShellExecute = false };
        foreach (var argument in arguments)
        {
            start.ArgumentList.Add(argument);
        }

        try
        {
            using var process = Process.Start(start);
            if (process is null)
            {
                error.WriteLine("dotnet-crap could not start its native binary.");
                return new StartOutcome(2, false);
            }

            await process.WaitForExitAsync();
            return new StartOutcome(process.ExitCode, false);
        }
        catch (Win32Exception exception) when (!OperatingSystem.IsWindows() && exception.NativeErrorCode == UnixPermissionDenied)
        {
            return new StartOutcome(2, true);
        }
        catch (Exception exception) when (exception is IOException or UnauthorizedAccessException or Win32Exception)
        {
            error.WriteLine($"dotnet-crap could not start its native binary: {exception.Message}");
            return new StartOutcome(2, false);
        }
    }

    [UnsupportedOSPlatform("windows")]
    private static string CreatePrivateDirectory(string root)
    {
        for (var attempt = 0; attempt < 16; attempt++)
        {
            var path = Path.Combine(root, "dotnet-crap-" + Guid.NewGuid().ToString("N"));
            if (Directory.Exists(path) || File.Exists(path))
            {
                continue;
            }

            Directory.CreateDirectory(path, PrivateDirectoryMode);
            return path;
        }

        throw new IOException("Could not allocate a unique private launcher directory.");
    }

    private static void RemovePrivateDirectory(string path, TextWriter error)
    {
        try
        {
            Directory.Delete(path, recursive: true);
        }
        catch (Exception exception) when (exception is IOException or UnauthorizedAccessException)
        {
            error.WriteLine($"dotnet-crap warning: could not remove private launcher directory {path}: {exception.Message}");
        }
    }
}
