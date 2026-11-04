using System.Runtime.CompilerServices;
using System.Text.Json;

// Explicit instrumentation keeps this fixture dependency free. This is not a
// Coverlet integration: zero-hit probe points are supplied as well as hits.
var source = File.ReadAllLines(args[0]);
for (var i = 0; i < source.Length; i++)
    if (source[i].Contains("// point")) Probe.Hits[i + 1] = 0;
if (Calculator.Choose(4) != 4) throw new Exception("positive assertion failed");
if (args[1] == "full" && Calculator.Choose(-1) != 0)
    throw new Exception("negative assertion failed");
Console.WriteLine(JsonSerializer.Serialize(Probe.Hits));

public static class Probe
{
    public static readonly SortedDictionary<int, int> Hits = new();
    public static void Hit([CallerLineNumber] int line = 0) => Hits[line]++;
}
