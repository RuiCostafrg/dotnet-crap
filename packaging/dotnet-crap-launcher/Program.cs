using System.Runtime.InteropServices;
using DotnetCrap.Launcher;

var rid = NativeLauncher.CurrentRuntimeIdentifier();
if (rid is null)
{
    Console.Error.WriteLine($"dotnet-crap does not support this platform ({RuntimeInformation.OSDescription}, {RuntimeInformation.ProcessArchitecture}).");
    return 2;
}

return await NativeLauncher.RunAsync(AppContext.BaseDirectory, rid, args, Console.Error);
