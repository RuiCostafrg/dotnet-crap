namespace CoverletFixtures.Beta;

public sealed class OtherGauge
{
    public int Full(int value)
    {
        if (value == 0)
            return 0;
        return 1;
    }
}
