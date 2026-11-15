using System.Globalization;

namespace Navree.Tests;

/// <summary>
/// Pins the culture a test run starts in, so no test depends on the region of the machine that runs it. A test that needs a culture says which
/// and builds it (with the patterns it assumes) itself. <c>NAVREE_TEST_CULTURE</c> names another culture to run the whole suite in, to find
/// a test that passes only in one (for example <c>de-DE</c> or <c>hi-IN</c>); left out, the invariant culture is used.
/// </summary>
[TestClass]
public static class TestCulture
{
    /// <summary>Sets the culture of the run.</summary>
    /// <param name="context">The run's context.</param>
    [AssemblyInitialize]
    public static void Fix(TestContext context)
    {
        string? name = Environment.GetEnvironmentVariable("NAVREE_TEST_CULTURE");
        CultureInfo culture = string.IsNullOrWhiteSpace(name) ? CultureInfo.InvariantCulture : CultureInfo.GetCultureInfo(name);
        CultureInfo.DefaultThreadCurrentCulture = culture;
        CultureInfo.DefaultThreadCurrentUICulture = culture;
        CultureInfo.CurrentCulture = culture;
        CultureInfo.CurrentUICulture = culture;
    }
}
