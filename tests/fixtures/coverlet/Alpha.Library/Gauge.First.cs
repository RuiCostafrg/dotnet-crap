namespace CoverletFixtures.Alpha;

public partial class Gauge
{
    public int Choose(int value)
    {
        if (value > 0)
            return 1;
        return 0;
    }

    public int Full(int value)
    {
        if (value > 0)
            return 1;
        return 0;
    }
}
