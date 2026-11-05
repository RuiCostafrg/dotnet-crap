using System;
using System.Collections.Generic;
using System.IO;
using System.Reflection;
using System.Text.Json;

static class Hits {
    public static readonly Dictionary<string, Dictionary<int, ulong>> Files = new() {
        ["Switch.cs"] = new() { [5] = 0, [6] = 0 },
        ["Branches.cs"] = new() { [4] = 0, [5] = 0 },
        ["Nested.cs"] = new() { [3] = 0, [4] = 0, [5] = 0 }
    };
    public static void Mark(string file, int line) => Files[file][line]++;
    public static int Value(string file, int line, int value) { Mark(file, line); return value; }
}
class Program {
    static object Invoke(object target, string method, params object[] args) => target.GetType().GetMethod(method, BindingFlags.Instance | BindingFlags.NonPublic)!.Invoke(target, args)!;
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
