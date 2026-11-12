using CoverletFixtures.Beta;
using Xunit;

namespace CoverletFixtures.Beta.Tests;

public sealed class BetaCoverageTests
{
    [Fact]
    public void Exercises_both_branches_in_second_project()
    {
        // Prove the test host runtime, independently of the SDK used to build it.
        var expected = System.Environment.GetEnvironmentVariable("DOTNET_CRAP_TEST_TFM");
        if (expected is not null && expected != $"net{System.Environment.Version.Major}.0")
            throw new System.InvalidOperationException($"Expected {expected}, running {System.Environment.Version}");
        var gauge = new OtherGauge();
        Assert.Equal(0, gauge.Full(0));
        Assert.Equal(1, gauge.Full(1));
    }
}
