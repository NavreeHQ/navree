namespace Navree.Tests;

/// <summary>A condition the test framework has no method for, said where it is not a plain comparison.</summary>
public static class Expect
{
    /// <summary>Fails the test unless the condition holds.</summary>
    /// <param name="condition">The condition.</param>
    /// <param name="because">What it says when it fails.</param>
    public static void That(bool condition, string because) => Assert.IsTrue(condition, because);
}
