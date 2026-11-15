using System.Runtime.CompilerServices;
using System.Text.Json;

static class Probe
{
    public static readonly SortedDictionary<int, ulong> Hits = new();

    public static void Initialize(string source)
    {
        var lines = File.ReadAllLines(source);
        for (int i = 0; i < lines.Length; i++)
            if (lines[i].Contains("return Probe.Value(")) Hits.Add(i + 1, 0);
        if (Hits.Count != 2) throw new Exception("expected two explicit return-line probes");
    }

    public static int Value(int value, [CallerLineNumber] int line = 0)
    {
        Hits[line]++;
        return value;
    }
}

static class Program
{
    static void Check(bool value, string message)
    {
        if (!value) throw new Exception(message);
    }

    static void Main(string[] args)
    {
        string source = args[Array.IndexOf(args, "--source") + 1];
        string output = args[Array.IndexOf(args, "--coverage-output") + 1];
        var tfm = Environment.GetEnvironmentVariable("DOTNET_CRAP_TEST_TFM") ?? "net10.0";
        Check(Environment.Version.Major == int.Parse(tfm[3..].Split('.')[0]), "runtime/target mismatch");
        Probe.Initialize(source);
        Check(Patterns.Logical(3) && Patterns.Logical(20) && !Patterns.Logical(10), "and/or precedence");
        Check(!Patterns.Negated(3) && Patterns.Negated(10), "not and parentheses");
        Check(Patterns.Simple(1) && !Patterns.Simple(0), "simple negation");
        Check(Patterns.Property(new() { Number = 3, Text = "b" }), "property pattern");
        Check(!Patterns.Property(new() { Number = 10, Text = "b" }), "property miss");
        Check(Patterns.List([3, 99]) && Patterns.List([]) && !Patterns.List([10]), "list patterns");
        Check(Patterns.Expression(3) == 1 && Patterns.Expression(20) == 2 && Patterns.Expression(99) == 0, "switch expression");
        Check(Patterns.Statement(3) == 1 && Patterns.Statement(30) == 2 && Patterns.Statement(5) == 0, "switch guard");
        Check(Patterns.Guard(3, true) == 1 && Patterns.Guard(3, false) == 0, "boolean guard");
        Check(Patterns.Owned(0) == 0 && Patterns.Owned(1) == 1 && Patterns.Owned(9) == 0, "nested callables");
        Check(Patterns.Text() == "and or not && ||", "pattern words in text");
        Check(PatternBranch.Run(3) == 1, "covered pattern branch");
        if (Array.IndexOf(args, "--full") >= 0)
            Check(PatternBranch.Run(-1) == 0, "covered fallback branch");
        File.WriteAllText(output, JsonSerializer.Serialize(Probe.Hits));
        Console.WriteLine("modern pattern fixture execution passed");
    }
}
