using System.Diagnostics;
using System.Runtime.InteropServices;

var rid = RuntimeInformation.IsOSPlatform(OSPlatform.Windows)
    ? RuntimeInformation.ProcessArchitecture == Architecture.X64 ? "win-x64" : "unsupported"
    : RuntimeInformation.IsOSPlatform(OSPlatform.OSX)
        ? RuntimeInformation.ProcessArchitecture switch { Architecture.X64 => "osx-x64", Architecture.Arm64 => "osx-arm64", _ => "unsupported" }
        : RuntimeInformation.IsOSPlatform(OSPlatform.Linux)
            ? RuntimeInformation.ProcessArchitecture switch { Architecture.X64 => "linux-x64", Architecture.Arm64 => "linux-arm64", _ => "unsupported" }
            : "unsupported";
if (rid == "unsupported") { Console.Error.WriteLine($"dotnet-crap does not support this platform ({RuntimeInformation.OSDescription}, {RuntimeInformation.ProcessArchitecture})."); return 2; }
var executable = Path.Combine(AppContext.BaseDirectory, "native", rid, OperatingSystem.IsWindows() ? "dotnet-crap.exe" : "dotnet-crap");
if (!File.Exists(executable)) { Console.Error.WriteLine($"dotnet-crap native binary for {rid} is missing from this installation."); return 2; }
try
{
// NuGet extraction does not preserve Unix executable bits.
if (!OperatingSystem.IsWindows())
    File.SetUnixFileMode(executable, File.GetUnixFileMode(executable) | UnixFileMode.UserExecute);
var start = new ProcessStartInfo(executable) { UseShellExecute = false };
foreach (var argument in args) start.ArgumentList.Add(argument);
using var process = Process.Start(start);
if (process is null) return 2;
await process.WaitForExitAsync();
return process.ExitCode;

}
catch (Exception error) when (error is IOException or UnauthorizedAccessException or System.ComponentModel.Win32Exception)
{
    Console.Error.WriteLine($"dotnet-crap could not start its native binary: {error.Message}");
    return 2;
}
