using System.Diagnostics;
using System.Globalization;
using System.Runtime.Versioning;
using System.Text;
using DotnetCrap.Launcher;

namespace DotnetCrap.Launcher.Tests;

internal static class Program
{
    // A real POSIX shell executable: records $0 and argv NUL-separated, then exits with the requested code.
    private const string FakeNative = """
        #!/bin/sh
        printf '%s\0' "$0" "$@" > "$DOTNET_CRAP_TEST_RECORD" || exit 9
        echo stdout-marker
        echo stderr-marker >&2
        exit "${DOTNET_CRAP_TEST_EXIT:-0}"
        """;

    private static readonly string[] TrickyArguments =
    [
        "--root", "dir with spaces", "", "quote\"d", "-x", "--", "unicode-é", "back\\slash", "$HOME", "*", "line one\nline two",
    ];

    public static async Task<int> Main()
    {
        var tests = new (string Name, Func<Task> Run, bool UnixOnly)[]
        {
            ("runtime identifier is supported on this host", () => { RuntimeIdentifierIsSupported(); return Task.CompletedTask; }, false),
            ("Windows executable name is unchanged", () => { WindowsExecutableNameIsUnchanged(); return Task.CompletedTask; }, false),
            ("missing native binary reports an error and exits 2", MissingNativeReportsErrorAsync, false),
            ("executable native runs directly with exact argv and exit code", ExecutableNativeRunsDirectlyAsync, true),
            ("exit codes are forwarded exactly for direct runs", ExitCodesAreForwardedAsync, true),
            ("non-executable native runs from a private copy and leaves the install untouched", NonExecutableNativeUsesPrivateCopyAsync, true),
            ("private copy is removed after a non-zero child exit", PrivateCopyRemovedAfterFailureAsync, true),
            ("read-only installs need no write access (exec and non-exec files)", ReadOnlyInstallRunsWithoutWriteAccessAsync, true),
            ("owner without execute permission falls back to a private copy", OwnerWithoutExecuteFallsBackAsync, true),
            ("private copy preparation failure exits 2 without touching the install", PrivateCopyPreparationFailureAsync, true),
            ("in-process API forwards arguments and cleans its private copy", InProcessApiForwardsAndCleansUpAsync, true),
        };

        var passed = 0;
        var failed = 0;
        var skipped = 0;
        foreach (var (name, run, unixOnly) in tests)
        {
            if (unixOnly && OperatingSystem.IsWindows())
            {
                Console.WriteLine($"SKIP {name} (Unix permission semantics)");
                skipped++;
                continue;
            }

            try
            {
                await run();
                Console.WriteLine($"PASS {name}");
                passed++;
            }
            catch (Exception exception)
            {
                Console.WriteLine($"FAIL {name}: {exception.Message}");
                failed++;
            }
        }

        Console.WriteLine($"{passed} passed, {failed} failed, {skipped} skipped");
        return failed == 0 ? 0 : 1;
    }

    private static void RuntimeIdentifierIsSupported()
    {
        var rid = NativeLauncher.CurrentRuntimeIdentifier();
        Check(rid is "linux-x64" or "linux-arm64" or "osx-x64" or "osx-arm64" or "win-x64", $"unexpected RID {rid ?? "<null>"}");
    }

    private static void WindowsExecutableNameIsUnchanged()
    {
        Equal("dotnet-crap.exe", NativeLauncher.ExecutableName("win-x64"), "win-x64 executable");
        Equal("dotnet-crap", NativeLauncher.ExecutableName("linux-x64"), "linux-x64 executable");
        Equal(Path.Combine("base", "native", "osx-arm64", "dotnet-crap"), NativeLauncher.NativeExecutablePath("base", "osx-arm64"), "native path");
    }

    private static async Task MissingNativeReportsErrorAsync()
    {
        using var scratch = new Scratch();
        InstallLauncher(scratch);
        var run = await RunLauncherProcessAsync(scratch, ["--version"], 0);
        Equal(2, run.ExitCode, "missing native exit code");
        Check(run.Stderr.Contains("missing from this installation", StringComparison.Ordinal), $"stderr was: {run.Stderr}");
    }

    private static async Task ExecutableNativeRunsDirectlyAsync()
    {
        using var scratch = new Scratch();
        InstallLauncher(scratch);
        WriteNative(scratch, ExecutableMode);

        var run = await RunLauncherProcessAsync(scratch, TrickyArguments, 42);
        Equal(42, run.ExitCode, "forwarded exit code");
        Check(run.Stdout.Contains("stdout-marker", StringComparison.Ordinal), "child stdout was not forwarded");
        Check(run.Stderr.Contains("stderr-marker", StringComparison.Ordinal), "child stderr was not forwarded");

        var argv = ReadRecord(scratch.Record);
        Check(argv[0].EndsWith(Path.Combine("native", NativeRid(), NativeLauncher.ExecutableName(NativeRid())), StringComparison.Ordinal),
            $"executable path was {argv[0]}");
        Check(!argv[0].StartsWith(scratch.Temp, StringComparison.Ordinal), "an executable native should not be copied");
        SameArguments(TrickyArguments, argv.Skip(1).ToArray(), "forwarded argv");
        Equal(0, Directory.EnumerateFileSystemEntries(scratch.Temp).Count(), "private directories after direct run");
    }

    private static async Task ExitCodesAreForwardedAsync()
    {
        using var scratch = new Scratch();
        InstallLauncher(scratch);
        WriteNative(scratch, ExecutableMode);
        foreach (var code in new[] { 0, 1, 2, 42, 255 })
        {
            var run = await RunLauncherProcessAsync(scratch, ["--threshold", code.ToString(CultureInfo.InvariantCulture)], code);
            Equal(code, run.ExitCode, $"exit code {code}");
        }
    }

    private static async Task NonExecutableNativeUsesPrivateCopyAsync()
    {
        using var scratch = new Scratch();
        InstallLauncher(scratch);
        var installed = WriteNative(scratch, NonExecutableMode);
        var originalBytes = File.ReadAllBytes(installed);

        var run = await RunLauncherProcessAsync(scratch, TrickyArguments, 0);
        Equal(0, run.ExitCode, "exit code after private copy");
        Check(run.Stdout.Contains("stdout-marker", StringComparison.Ordinal), "child stdout was not forwarded from the copy");

        var argv = ReadRecord(scratch.Record);
        Check(argv[0].StartsWith(scratch.Temp, StringComparison.Ordinal), $"copy was not run from TMPDIR: {argv[0]}");
        Check(!argv[0].Equals(installed, StringComparison.Ordinal), "installed file was executed in place");
        SameArguments(TrickyArguments, argv.Skip(1).ToArray(), "forwarded argv from copy");

        Equal(NonExecutableMode, File.GetUnixFileMode(installed), "installed file mode");
        Check(File.ReadAllBytes(installed).AsSpan().SequenceEqual(originalBytes), "installed file bytes changed");
        Equal(0, Directory.EnumerateFileSystemEntries(scratch.Temp).Count(), "private copy leftovers");
    }

    private static async Task PrivateCopyRemovedAfterFailureAsync()
    {
        using var scratch = new Scratch();
        InstallLauncher(scratch);
        WriteNative(scratch, NonExecutableMode);
        var run = await RunLauncherProcessAsync(scratch, ["fail"], 7);
        Equal(7, run.ExitCode, "non-zero child exit through copy");
        Equal(0, Directory.EnumerateFileSystemEntries(scratch.Temp).Count(), "private copy leftovers after failure");
    }

    private static async Task ReadOnlyInstallRunsWithoutWriteAccessAsync()
    {
        if (Environment.UserName == "root")
        {
            Console.WriteLine("NOTE: running as root; root bypasses permission bits. Run as a non-root user to enforce read-only.");
        }

        foreach (var mode in new[] { ExecutableMode & ~UnixFileMode.UserWrite, NonExecutableMode & ~UnixFileMode.UserWrite })
        {
            using var scratch = new Scratch();
            InstallLauncher(scratch);
            var installed = WriteNative(scratch, mode);
            var nativeDirectory = Path.GetDirectoryName(installed)!;
            var before = Directory.EnumerateFileSystemEntries(nativeDirectory).Order(StringComparer.Ordinal).ToArray();

            SetUnixMode(nativeDirectory, ReadOnlyDirectoryMode);
            SetUnixMode(scratch.App, ReadOnlyDirectoryMode);
            SetUnixMode(installed, mode);
            Equal(mode, File.GetUnixFileMode(installed), "initial read-only native mode");

            var run = await RunLauncherProcessAsync(scratch, ["--read-only"], 0);
            Equal(0, run.ExitCode, $"read-only install (mode {Convert.ToString((int)mode, 8)}) exit code");
            Equal(mode, File.GetUnixFileMode(installed), $"read-only install mode {Convert.ToString((int)mode, 8)}");
            SameArguments(before, Directory.EnumerateFileSystemEntries(nativeDirectory).Order(StringComparer.Ordinal).ToArray(), "install directory entries");
        }
    }

    private static async Task OwnerWithoutExecuteFallsBackAsync()
    {
        // Only the group execute bit is set and the test user owns the file, so the kernel refuses
        // the in-place launch with EACCES and the launcher must use its private copy instead.
        using var scratch = new Scratch();
        InstallLauncher(scratch);
        var installed = WriteNative(scratch, GroupExecuteOnly);

        var run = await RunLauncherProcessAsync(scratch, ["fallback"], 0);
        Equal(0, run.ExitCode, "exit code after EACCES fallback");
        var argv = ReadRecord(scratch.Record);
        SameArguments(["fallback"], argv.Skip(1).ToArray(), "fallback argv");
        Equal(GroupExecuteOnly, File.GetUnixFileMode(installed), "installed mode after fallback");
        if (Environment.UserName == "root")
        {
            Console.WriteLine("NOTE: running as root; root may execute with any execute bit, so the fallback was not exercised.");
        }
        else
        {
            Check(argv[0].StartsWith(scratch.Temp, StringComparison.Ordinal), $"EACCES did not fall back to a private copy: {argv[0]}");
        }
        Equal(0, Directory.EnumerateFileSystemEntries(scratch.Temp).Count(), "private copy leftovers after fallback");
    }

    private static async Task PrivateCopyPreparationFailureAsync()
    {
        using var scratch = new Scratch();
        InstallLauncher(scratch);
        var installed = WriteNative(scratch, NonExecutableMode);
        var blocker = Path.Combine(scratch.Root, "not-a-directory");
        File.WriteAllText(blocker, "blocks CreateDirectory", new UTF8Encoding(false));

        using var stderr = new StringWriter();
        var code = await NativeLauncher.RunAsync(scratch.App, NativeRid(), ["--version"], stderr, blocker);
        Equal(2, code, "preparation failure exit code");
        Check(stderr.ToString().Contains("could not prepare", StringComparison.Ordinal), $"stderr was: {stderr}");
        Equal(NonExecutableMode, File.GetUnixFileMode(installed), "installed mode after failed preparation");
    }

    private static async Task InProcessApiForwardsAndCleansUpAsync()
    {
        using var scratch = new Scratch();
        InstallLauncher(scratch);
        WriteNative(scratch, NonExecutableMode);
        using var env = new EnvironmentOverride(scratch);
        env.Set("DOTNET_CRAP_TEST_EXIT", "3");

        using var stderr = new StringWriter();
        var code = await NativeLauncher.RunAsync(scratch.App, NativeRid(), TrickyArguments, stderr, scratch.Temp);
        Equal(3, code, "in-process exit code");

        var argv = ReadRecord(scratch.Record);
        Check(argv[0].StartsWith(scratch.Temp, StringComparison.Ordinal), $"in-process copy was not under the requested root: {argv[0]}");
        SameArguments(TrickyArguments, argv.Skip(1).ToArray(), "in-process argv");
        Equal(0, Directory.EnumerateFileSystemEntries(scratch.Temp).Count(), "in-process private copy leftovers");
    }

    private const UnixFileMode ExecutableMode =
        UnixFileMode.UserRead | UnixFileMode.UserWrite | UnixFileMode.UserExecute |
        UnixFileMode.GroupRead | UnixFileMode.GroupExecute | UnixFileMode.OtherRead | UnixFileMode.OtherExecute;

    private const UnixFileMode GroupExecuteOnly = UnixFileMode.UserRead | UnixFileMode.UserWrite | UnixFileMode.GroupExecute;

    // Matches a NuGet extraction, which keeps read bits but drops execute bits.
    private const UnixFileMode NonExecutableMode =
        UnixFileMode.UserRead | UnixFileMode.UserWrite | UnixFileMode.GroupRead | UnixFileMode.OtherRead;

    private const UnixFileMode ReadOnlyDirectoryMode =
        UnixFileMode.UserRead | UnixFileMode.UserExecute | UnixFileMode.GroupRead | UnixFileMode.GroupExecute |
        UnixFileMode.OtherRead | UnixFileMode.OtherExecute;

    private static string NativeRid() => NativeLauncher.CurrentRuntimeIdentifier()
        ?? throw new InvalidOperationException("No supported runtime identifier on this host.");

    private static string WriteNative(Scratch scratch, UnixFileMode mode)
    {
        var directory = Path.Combine(scratch.App, "native", NativeRid());
        Directory.CreateDirectory(directory);
        var path = Path.Combine(directory, NativeLauncher.ExecutableName(NativeRid()));
        File.WriteAllText(path, FakeNative + "\n", new UTF8Encoding(false));
        SetUnixMode(path, mode);
        return path;
    }

    private static void InstallLauncher(Scratch scratch)
    {
        var copied = 0;
        foreach (var file in Directory.EnumerateFiles(AppContext.BaseDirectory, "dotnet-crap-launcher.*"))
        {
            File.Copy(file, Path.Combine(scratch.App, Path.GetFileName(file)), overwrite: true);
            copied++;
        }
        Check(copied > 0, "launcher build output was not found next to the test assembly");
    }

    private static async Task<LauncherRun> RunLauncherProcessAsync(Scratch scratch, IReadOnlyList<string> arguments, int exitCode)
    {
        var start = new ProcessStartInfo(DotnetHost()) { UseShellExecute = false, RedirectStandardOutput = true, RedirectStandardError = true };
        start.ArgumentList.Add(Path.Combine(scratch.App, "dotnet-crap-launcher.dll"));
        foreach (var argument in arguments)
        {
            start.ArgumentList.Add(argument);
        }
        start.Environment["TMPDIR"] = scratch.Temp;
        start.Environment["DOTNET_CRAP_TEST_RECORD"] = scratch.Record;
        start.Environment["DOTNET_CRAP_TEST_EXIT"] = exitCode.ToString(CultureInfo.InvariantCulture);

        using var process = Process.Start(start) ?? throw new InvalidOperationException("could not start the launcher process");
        var stdout = process.StandardOutput.ReadToEndAsync();
        var stderr = process.StandardError.ReadToEndAsync();
        await process.WaitForExitAsync();
        return new LauncherRun(process.ExitCode, await stdout, await stderr);
    }

    private static string DotnetHost() => Environment.GetEnvironmentVariable("DOTNET_HOST_PATH") is { Length: > 0 } host ? host : "dotnet";

    private static IReadOnlyList<string> ReadRecord(string path)
    {
        Check(File.Exists(path), "the fake native executable did not write its argv record");
        var text = Encoding.UTF8.GetString(File.ReadAllBytes(path));
        var parts = text.Split('\0');
        Check(parts[^1].Length == 0, "argv record was not NUL-terminated");
        return parts[..^1];
    }

    [UnsupportedOSPlatform("windows")]
    private static void SetUnixMode(string path, UnixFileMode mode) => File.SetUnixFileMode(path, mode);

    private static void Check(bool condition, string message)
    {
        if (!condition)
        {
            throw new InvalidOperationException(message);
        }
    }

    private static void Equal<T>(T expected, T actual, string label)
    {
        if (!EqualityComparer<T>.Default.Equals(expected, actual))
        {
            throw new InvalidOperationException($"{label}: expected {expected} but was {actual}");
        }
    }

    private static void SameArguments(IReadOnlyList<string> expected, IReadOnlyList<string> actual, string label)
    {
        Check(expected.Count == actual.Count, $"{label}: expected {expected.Count} arguments but got {actual.Count}");
        for (var index = 0; index < expected.Count; index++)
        {
            Check(string.Equals(expected[index], actual[index], StringComparison.Ordinal),
                $"{label}: argument {index} expected [{expected[index]}] but was [{actual[index]}]");
        }
    }

    private sealed record LauncherRun(int ExitCode, string Stdout, string Stderr);

    private sealed class Scratch : IDisposable
    {
        public Scratch()
        {
            Root = Path.Combine(Path.GetTempPath(), "dotnet-crap-launcher-tests-" + Guid.NewGuid().ToString("N"));
            App = Path.Combine(Root, "app");
            Temp = Path.Combine(Root, "tmp");
            Directory.CreateDirectory(App);
            Directory.CreateDirectory(Temp);
        }

        public string Root { get; }
        public string App { get; }
        public string Temp { get; }
        public string Record => Path.Combine(Root, "record.bin");

        public void Dispose()
        {
            try
            {
                if (!OperatingSystem.IsWindows())
                {
                    MakeWritable(Root);
                }
                Directory.Delete(Root, recursive: true);
            }
            catch (Exception exception) when (exception is IOException or UnauthorizedAccessException)
            {
                Console.WriteLine($"NOTE: could not remove test directory {Root}: {exception.Message}");
            }
        }

        [UnsupportedOSPlatform("windows")]
        private static void MakeWritable(string root)
        {
            foreach (var directory in Directory.EnumerateDirectories(root, "*", SearchOption.AllDirectories).Append(root))
            {
                File.SetUnixFileMode(directory, File.GetUnixFileMode(directory) | ExecutableWritable);
            }
            foreach (var file in Directory.EnumerateFiles(root, "*", SearchOption.AllDirectories))
            {
                File.SetUnixFileMode(file, File.GetUnixFileMode(file) | UnixFileMode.UserWrite);
            }
        }

        private const UnixFileMode ExecutableWritable = UnixFileMode.UserRead | UnixFileMode.UserWrite | UnixFileMode.UserExecute;
    }

    private sealed class EnvironmentOverride : IDisposable
    {
        private readonly Dictionary<string, string?> _previous = new(StringComparer.Ordinal);

        public EnvironmentOverride(Scratch scratch)
        {
            Set("DOTNET_CRAP_TEST_RECORD", scratch.Record);
        }

        public void Set(string name, string value)
        {
            _previous.TryAdd(name, Environment.GetEnvironmentVariable(name));
            Environment.SetEnvironmentVariable(name, value);
        }

        public void Dispose()
        {
            foreach (var (name, value) in _previous)
            {
                Environment.SetEnvironmentVariable(name, value);
            }
        }
    }
}
