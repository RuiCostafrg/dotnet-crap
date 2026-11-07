using CoverletFixtures.Alpha;
using Microsoft.VisualStudio.TestTools.UnitTesting;

namespace CoverletFixtures.MSTest.Tests;

[TestClass]
public sealed class MSTestCoverageTests
{
    [TestMethod]
    public void Exercises_one_branch_of_partial_method_and_both_full_method_branches()
    {
        var gauge = new Gauge();
        Assert.AreEqual(1, gauge.Choose(1));
        Assert.AreEqual(1, gauge.Full(1));
        Assert.AreEqual(0, gauge.Full(0));
        // Distinguish this project's report from the paired xUnit project.
        Assert.AreEqual(1, gauge.Full(1));
    }
}
