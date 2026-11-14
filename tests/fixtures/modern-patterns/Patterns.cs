public sealed class PatternValue
{
    public int Number { get; init; }
    public string Text { get; init; } = "";
}

public static class Patterns
{
    public static bool Logical(int x) => x is > 0 and < 10 or 20;

    public static bool Negated(int x) => x is not (> 0 and < 10);

    public static bool Simple(int x) => x is not 0;

    public static bool Property(PatternValue x) =>
        x is { Number: > 0 and < 10, Text: "a" or "b" };

    public static bool List(int[] x) => x is [> 0 and < 10, ..] or [];

    public static int Expression(int x) => x switch
    {
        > 0 and < 10 => 1,
        20 or 30 => 2,
        _ => 0
    };

    public static int Statement(int x)
    {
        switch (x)
        {
            case > 0 and < 10 when x != 5: return 1;
            case 20 or 30: return 2;
            default: return 0;
        }
    }

    public static int Guard(int x, bool enabled) => x switch
    {
        > 0 and < 10 when enabled && x != 5 => 1,
        _ => 0
    };

    public static int Owned(int x)
    {
        int Local(int y) => y switch { > 0 and < 10 => 1, _ => 0 };
        Func<int, bool> f = y => y is 1 or 2;
        return x switch { 0 => Local(x), _ => f(x) ? 1 : 0 };
    }

    public static string Text() => "and or not && ||";
}
