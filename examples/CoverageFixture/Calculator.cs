public static class Calculator
{
    public static int Choose(int value)
    {
        Probe.Hit(); // point
        if (value > 0) // block
        {
            Probe.Hit(); // point
            return value; // block
        }
        Probe.Hit(); // point
        return 0; // block
    }
}
