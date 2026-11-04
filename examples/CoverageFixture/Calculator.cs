public static class Calculator
{
    public static int Choose(int value)
    {
        Probe.Hit(); // point
        if (value > 0)
        {
            Probe.Hit(); // point
            return value;
        }
        Probe.Hit(); // point
        return 0;
    }
}
