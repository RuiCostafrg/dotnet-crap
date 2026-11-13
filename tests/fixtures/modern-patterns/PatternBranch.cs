public static class PatternBranch
{
    public static int Run(int x)
    {
        if (x is > 0 and < 10 or 20)
        {
            return Probe.Value(1);
        }
        return Probe.Value(0);
    }
}
