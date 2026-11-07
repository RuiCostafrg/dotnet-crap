using CoverletFixtures.Beta;
using NUnit.Framework;

namespace CoverletFixtures.NUnit.Tests;

public sealed class NUnitCoverageTests
{
    [Test]
    public void Exercises_both_branches_in_second_project()
    {
        var gauge = new OtherGauge();
        Assert.That(gauge.Full(0), Is.EqualTo(0));
        Assert.That(gauge.Full(1), Is.EqualTo(1));
    }
}
