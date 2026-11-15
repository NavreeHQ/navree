using System.Reflection;
using System.Runtime.CompilerServices;
using Navree.Controls.Tests.Tables;

namespace Navree.Controls.Tests.Controls;

[TestClass]
public sealed class ProductBuildTests
{
    private const BindingFlags Everything = BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Static | BindingFlags.Instance | BindingFlags.DeclaredOnly;

    private static readonly Assembly Controls = typeof(DataTable).Assembly;

    [TestMethod]
    public void The_product_build_holds_no_measuring_parts()
    {
        Assert.IsNull(Controls.GetType("Navree.Controls.Diag"), "the diagnostics class is only in the measuring build");
        Assert.IsNull(Controls.GetType("Navree.Controls.StickyMode"), "the control moves its header and frozen columns one way");
        Assert.IsNull(typeof(DataTable).GetMethod("Watch", Everything), "the table has no place to write measuring lines");
        Assert.IsNull(typeof(DataTable).GetProperty("StickyWay", Everything));
        Type sticky = Controls.GetType("Navree.Controls.StickyCoordinator")!;
        Assert.IsNull(sticky.GetProperty("Notes", Everything), "the coordinator keeps no notes");
        Assert.IsNull(sticky.GetProperty("Mode", Everything));
    }

    [TestMethod]
    public void Only_the_controls_own_tests_see_the_inner_parts_of_the_product_build()
    {
        string[] friends = [.. Controls.GetCustomAttributes<InternalsVisibleToAttribute>().Select(a => a.AssemblyName)];
        Same.Items(["Navree.Controls.Tests"], friends);
    }
}
