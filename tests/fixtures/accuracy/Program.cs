using System;
using System.Reflection;
class Program {
    static object Invoke(object target, string method, params object[] args) => target.GetType().GetMethod(method, BindingFlags.Instance | BindingFlags.NonPublic)!.Invoke(target, args)!;
    static void Main() {
        if ((int)Invoke(new SwitchFixture(), "Pick", 2) != 1 || (int)Invoke(new SwitchFixture(), "Pick", 9) != 0) throw new Exception("switch");
        if ((int)Invoke(new BranchesFixture(), "Run", "abc", true, true) != 3) throw new Exception("branches");
        if ((int)Invoke(new NestedFixture(), "Outer", 7) != 7) throw new Exception("nested");
        Console.WriteLine("fixture execution passed");
    }
}
