using System;
class Program
{
    static int Choose(int x)
    {
        if (x > 0 && x < 5) return 1;
        return 0;
    }
    static void Main()
    {
        if (Choose(2) != 1 || Choose(8) != 0) throw new Exception("assertion failed");
        Console.WriteLine("fixture passed");
    }
}
