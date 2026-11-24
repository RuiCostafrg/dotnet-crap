using System.Runtime.CompilerServices;
using System.Text.Json;

// Explicit instrumentation keeps this fixture dependency free. This is not a
// Coverlet integration: zero-hit probe points are supplied as well as hits.
// Each `// point` probe counts its basic block, and every later `// block`
// line in that block reports the probe's count, so every executable line has
// a point, as dotnet-crap requires.
var source = File.ReadAllLines(args[0]);
for (var i = 0; i < source.Length; i++)
    if (source[i].Contains("// point")) Probe.Hits[i + 1] = 0;
if (Calculator.Choose(4) != 4) throw new Exception("positive assertion failed");
if (args[1] == "full" && Calculator.Choose(-1) != 0)
    throw new Exception("negative assertion failed");
var probe = 0;
for (var i = 0; i < source.Length; i++)
{
    if (source[i].Contains("// point")) probe = i + 1;
    else if (source[i].Contains("// block")) Probe.Hits[i + 1] = Probe.Hits[probe];
}
Console.WriteLine(JsonSerializer.Serialize(Probe.Hits));

public static class Probe
{
    public static readonly SortedDictionary<int, int> Hits = new();
    public static void Hit([CallerLineNumber] int line = 0) => Hits[line]++;
}
