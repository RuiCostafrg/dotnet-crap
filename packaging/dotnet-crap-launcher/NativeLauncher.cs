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

    /// <summary>Returns the bundled runtime identifier supported by the current OS and architecture.</summary>
    /// <returns>The supported runtime identifier, or <see langword="null"/> when this launcher has no matching binary.</returns>
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

    /// <summary>Gets the native executable file name for a runtime identifier.</summary>
    /// <param name="rid">Runtime identifier used to select the platform-specific file name.</param>
    /// <returns><c>dotnet-crap.exe</c> for <c>win-x64</c>; otherwise <c>dotnet-crap</c>.</returns>
    public static string ExecutableName(string rid) => rid == "win-x64" ? "dotnet-crap.exe" : "dotnet-crap";

    /// <summary>Builds the path to the native executable bundled with the launcher.</summary>
    /// <param name="appBaseDirectory">Directory containing the launcher application.</param>
    /// <param name="rid">Runtime identifier subdirectory under <c>native</c>.</param>
    /// <returns>The combined path to the native executable.</returns>
    public static string NativeExecutablePath(string appBaseDirectory, string rid) =>
        Path.Combine(appBaseDirectory, "native", rid, ExecutableName(rid));

    /// <summary>
    /// Runs the native executable with <paramref name="arguments"/> passed through unchanged and
    /// returns its exit code. Launcher-side failures return 2 and are written to <paramref name="error"/>.
    /// </summary>
    /// <param name="appBaseDirectory">Directory containing the launcher application and bundled native files.</param>
    /// <param name="rid">Runtime identifier selecting the native executable.</param>
    /// <param name="arguments">Arguments passed to the native executable in order.</param>
    /// <param name="error">Writer for launcher-side error messages.</param>
    /// <param name="temporaryRoot">Parent for the private copy; defaults to <see cref="Path.GetTempPath"/>.</param>
    /// <returns>The native executable's exit code, or 2 when the launcher cannot start it.</returns>
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

    /// <summary>Locates the bundled executable and chooses the direct or private-copy launch path.</summary>
    /// <param name="appBaseDirectory">Directory containing the launcher application.</param>
    /// <param name="rid">Runtime identifier selecting the native executable.</param>
    /// <param name="arguments">Arguments passed to the native executable in order.</param>
    /// <param name="error">Writer for launcher-side error messages.</param>
    /// <param name="temporaryRoot">Parent directory for a private executable copy, when needed.</param>
    /// <returns>The child exit code or 2 when the executable is missing or cannot be launched.</returns>
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

    /// <summary>Copies the native file into a private directory, launches it, then removes that directory.</summary>
    /// <param name="installed">Path to the bundled native file.</param>
    /// <param name="fileName">File name to use for the private copy.</param>
    /// <param name="arguments">Arguments passed to the copied executable in order.</param>
    /// <param name="error">Writer for launcher-side error messages.</param>
    /// <param name="temporaryRoot">Parent directory for the private directory.</param>
    /// <returns>The child exit code or 2 when preparation or launch fails.</returns>
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

    /// <summary>Starts a process with the supplied argument list and waits for it to exit.</summary>
    /// <param name="executable">Path to the executable.</param>
    /// <param name="arguments">Arguments passed to the executable in order.</param>
    /// <param name="error">Writer for launch errors other than Unix permission denial.</param>
    /// <returns>The child exit code and whether launch failed with Unix permission denial.</returns>
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

    /// <summary>Creates a uniquely named directory with owner-only permissions.</summary>
    /// <param name="root">Parent directory in which to create the private directory.</param>
    /// <returns>The path of the created directory.</returns>
    /// <exception cref="IOException">No unique directory could be allocated after the retry limit.</exception>
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

    /// <summary>Attempts to remove a private directory and reports cleanup failures.</summary>
    /// <param name="path">Directory to remove recursively.</param>
    /// <param name="error">Writer for a warning if removal fails.</param>
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
