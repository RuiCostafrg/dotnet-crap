using CoverletFixtures.Beta;
using NUnit.Framework;

namespace CoverletFixtures.NUnit.Tests;

public sealed class NUnitCoverageTests
{
    [Test]
    public void Exercises_both_branches_in_second_project()
    {
        // Prove the test host runtime, independently of the SDK used to build it.
        var expected = System.Environment.GetEnvironmentVariable("DOTNET_CRAP_TEST_TFM");
        if (expected is not null && expected != $"net{System.Environment.Version.Major}.0")
            throw new System.InvalidOperationException($"Expected {expected}, running {System.Environment.Version}");
        var gauge = new OtherGauge();
        Assert.That(gauge.Full(0), Is.EqualTo(0));
        Assert.That(gauge.Full(1), Is.EqualTo(1));
        // Distinguish this project's report from the paired xUnit project.
        Assert.That(gauge.Full(1), Is.EqualTo(1));
    }
}
