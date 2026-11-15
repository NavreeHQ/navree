namespace Navree.Controls.Tests.Tables;

/// <summary>Comparisons the tests share.</summary>
internal static class Same
{
    /// <summary>Asserts two sequences hold the same items in the same order.</summary>
    internal static void Items<T>(IEnumerable<T> expected, IEnumerable<T> actual)
    {
        T[] want = expected.ToArray();
        T[] got = actual.ToArray();
        Assert.IsTrue(
            want.SequenceEqual(got),
            "expected [" + string.Join(", ", want) + "] but got [" + string.Join(", ", got) + "]");
    }

    /// <summary>Asserts two numbers are within a hair of each other.</summary>
    internal static void Number(double expected, double actual) => Assert.AreEqual(expected, actual, 1e-6);
}
