using CoverletFixtures.Alpha;
using Xunit;

namespace CoverletFixtures.Alpha.Tests;

public sealed class AlphaCoverageTests
{
    [Fact]
    public void Exercises_one_branch_of_partial_method_and_both_full_method_branches()
    {
        var gauge = new Gauge();
        Assert.Equal(1, gauge.Choose(1));
        Assert.Equal(1, gauge.Full(1));
        Assert.Equal(0, gauge.Full(0));
    }
}
