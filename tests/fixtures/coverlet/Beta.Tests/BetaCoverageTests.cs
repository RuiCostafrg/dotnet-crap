using CoverletFixtures.Beta;
using Xunit;

namespace CoverletFixtures.Beta.Tests;

public sealed class BetaCoverageTests
{
    [Fact]
    public void Exercises_both_branches_in_second_project()
    {
        var gauge = new OtherGauge();
        Assert.Equal(0, gauge.Full(0));
        Assert.Equal(1, gauge.Full(1));
    }
}
