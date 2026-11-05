using System;
using System.Collections.Generic;
using System.IO;
using System.Reflection;
using System.Text.Json;

static class Hits {
    public static readonly Dictionary<string, Dictionary<int, ulong>> Files = new() {
        ["Switch.cs"] = new() { [6] = 0, [7] = 0 },
        ["Branches.cs"] = new() { [5] = 0, [6] = 0 },
        ["Nested.cs"] = new() { [5] = 0, [6] = 0, [7] = 0 }
    };
    /// <summary>Increment the explicitly registered source-line probe.</summary>
    public static void Mark(string file, int line) => Files[file][line]++;
    /// <summary>Record local callable execution and preserve its return value.</summary>
    public static int Value(string file, int line, int value) { Mark(file, line); return value; }
}
class Program {
    /// <summary>Invoke a fixture's private callable without changing its scanner declaration.</summary>
    static object Invoke(object target, string method, params object[] args) => target.GetType().GetMethod(method, BindingFlags.Instance | BindingFlags.NonPublic)!.Invoke(target, args)!;
    /// <summary>Run partial or full probe paths, assert runtime outcomes and write hit counts.</summary>
    static void Main(string[] args) {
        if ((int)Invoke(new SwitchFixture(), "Pick", 2) != 1) throw new Exception("switch");
        if ((int)Invoke(new BranchesFixture(), "Run", "abc", true, true) != 3) throw new Exception("branches");
        if ((int)Invoke(new NestedFixture(), "Outer", 7) != 7) throw new Exception("nested");
        if (Array.IndexOf(args, "--full") >= 0) {
            if ((int)Invoke(new SwitchFixture(), "Pick", 9) != 0) throw new Exception("switch default");
            if ((int)Invoke(new BranchesFixture(), "Run", null!, true, true) != 0) throw new Exception("catch");
            if ((int)Invoke(new NestedFixture(), "Outer", 0) != 0) throw new Exception("nested default");
        }
        var output = Array.IndexOf(args, "--coverage-output");
        if (output < 0 || output + 1 >= args.Length) throw new Exception("coverage output path required");
        File.WriteAllText(args[output + 1], JsonSerializer.Serialize(Hits.Files));
        Console.WriteLine("fixture execution passed");
    }
}
