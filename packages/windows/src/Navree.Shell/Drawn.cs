using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Navree.Design.Pages;
using Navree.Protocol;
using Navree.Runtime;

namespace Navree.Shell;

/// <summary>
/// What one call of <see cref="NodeRenderer.Render"/> left: the node, the element drawn for it, the records of the parts drawn inside it, and what is
/// needed to bring it up to date, or to draw it again alone in its own place, when an answer changes the screen. A part whose node did not change
/// stands, and nothing below it is looked at.
/// </summary>
internal sealed class Drawn
{
    /// <summary>Makes the record of a node that is being drawn.</summary>
    /// <param name="node">The node.</param>
    internal Drawn(Models.Node node) => Node = node;

    /// <summary>The node as it was last drawn, or brought up to date with.</summary>
    internal Models.Node Node { get; set; }

    /// <summary>What was drawn for it: what the part above it holds in its place.</summary>
    internal UIElement Element { get; set; } = null!;

    /// <summary>The record of the part it is inside, or none for the top of a drawing.</summary>
    internal Drawn? Parent { get; set; }

    /// <summary>The records of the parts drawn inside it, in the order they were drawn.</summary>
    internal List<Drawn> Children { get; } = [];

    /// <summary>The screen as it was when this part was drawn or last brought up to date: what the part holds about the screen is what it held then.</summary>
    internal ScreenView Screen { get; set; } = null!;

    /// <summary>The form the part stood in when it was drawn, to draw it again in the same one.</summary>
    internal string? Form { get; init; }

    /// <summary>The sheet, dialog or popover it stood in.</summary>
    internal string? Overlay { get; init; }

    /// <summary>Whether the first heading that says the screen's title had been left out already when this part began to be drawn.</summary>
    internal bool CoveredBefore { get; init; }

    /// <summary>The words drawn last before this part began, so a chart does not say its own title under the same words.</summary>
    internal string? WordsBefore { get; init; }

    /// <summary>The sheets, dialogs and popovers this part found.</summary>
    internal List<OverlayRequest> Overlays { get; } = [];

    /// <summary>The fields this part drew itself, by form and id.</summary>
    internal List<((string Form, string Field) Key, FieldHandle Handle)> Fields { get; } = [];

    /// <summary>The buttons this part drew itself, by the key that names them.</summary>
    internal List<(string Key, FrameworkElement Element)> Triggers { get; } = [];

    /// <summary>The runs of children this part holds in its element, one for each place a child is put.</summary>
    internal List<Group> Groups { get; } = [];

    /// <summary>
    /// Brings the drawn part up to date with a node of the same kind and id that is not equal to the one it was drawn from; false when it cannot
    /// (the part is then drawn again). Null for a kind that is always drawn again.
    /// </summary>
    internal Func<Models.Node, bool>? Update { get; set; }

    /// <summary>Puts another element where a child of this part stood, when the part has its own way (the page's places); the groups are tried first.</summary>
    internal Func<Drawn, Drawn, bool>? SwapOwn { get; set; }

    /// <summary>Puts the record of a part drawn again where a child's record stood, and its element where the child's element stood.</summary>
    /// <param name="old">The child's record.</param>
    /// <param name="fresh">The record of the part drawn again.</param>
    /// <returns>False when this part has no way to do it.</returns>
    internal bool Swap(Drawn old, Drawn fresh)
    {
        foreach (Group group in Groups)
        {
            if (group.Replace(old, fresh))
            {
                fresh.Parent = this;
                int at = Children.IndexOf(old);
                if (at >= 0)
                {
                    Children[at] = fresh;
                }

                return true;
            }
        }

        if (SwapOwn is not null && SwapOwn(old, fresh))
        {
            fresh.Parent = this;
            int at = Children.IndexOf(old);
            if (at >= 0)
            {
                Children[at] = fresh;
            }

            return true;
        }

        return false;
    }

    /// <summary>This record and every record inside it, depth first, in the order they were drawn.</summary>
    /// <returns>The records.</returns>
    internal IEnumerable<Drawn> Walk()
    {
        Stack<Drawn> pending = new();
        pending.Push(this);
        while (pending.Count > 0)
        {
            Drawn next = pending.Pop();
            yield return next;
            for (int at = next.Children.Count - 1; at >= 0; at--)
            {
                pending.Push(next.Children[at]);
            }
        }
    }
}

/// <summary>Where the elements of a run of children stand in the element of the part that holds them.</summary>
internal interface IHost
{
    /// <summary>How many elements there are.</summary>
    int Count { get; }

    /// <summary>The element at a place.</summary>
    /// <param name="index">The place.</param>
    /// <returns>The element.</returns>
    UIElement At(int index);

    /// <summary>The place of an element, or minus one.</summary>
    /// <param name="element">The element.</param>
    /// <returns>Its place.</returns>
    int IndexOf(UIElement element);

    /// <summary>Puts an element in.</summary>
    /// <param name="index">The place.</param>
    /// <param name="element">The element.</param>
    void Insert(int index, UIElement element);

    /// <summary>Takes the element at a place out.</summary>
    /// <param name="index">The place.</param>
    void RemoveAt(int index);

    /// <summary>Puts an element where another stood.</summary>
    /// <param name="index">The place.</param>
    /// <param name="element">The element.</param>
    void Set(int index, UIElement element);
}

/// <summary>The children of a panel.</summary>
internal sealed class PanelHost(UIElementCollection children) : IHost
{
    /// <inheritdoc />
    public int Count => children.Count;

    /// <inheritdoc />
    public UIElement At(int index) => children[index];

    /// <inheritdoc />
    public int IndexOf(UIElement element) => children.IndexOf(element);

    /// <inheritdoc />
    public void Insert(int index, UIElement element) => children.Insert(index, element);

    /// <inheritdoc />
    public void RemoveAt(int index) => children.RemoveAt(index);

    /// <inheritdoc />
    public void Set(int index, UIElement element) => children[index] = element;
}

/// <summary>The items of a list view.</summary>
internal sealed class ItemsHost(ItemCollection items) : IHost
{
    /// <inheritdoc />
    public int Count => items.Count;

    /// <inheritdoc />
    public UIElement At(int index) => (UIElement)items[index];

    /// <inheritdoc />
    public int IndexOf(UIElement element) => items.IndexOf(element);

    /// <inheritdoc />
    public void Insert(int index, UIElement element) => items.Insert(index, element);

    /// <inheritdoc />
    public void RemoveAt(int index) => items.RemoveAt(index);

    /// <inheritdoc />
    public void Set(int index, UIElement element) => items[index] = element;
}

/// <summary>
/// A run of children that a part draws one element each for and puts side by side in one place of its element (the children of a stack, the rows of
/// a list). It is how the part's children are matched with the ones it had, drawn when they are new, taken out when they are gone, put in order,
/// and drawn again alone where one cannot be brought up to date.
/// </summary>
internal sealed class Group
{
    /// <summary>Where the elements of the run stand.</summary>
    internal required IHost Host { get; set; }

    /// <summary>The place in the host where the first element of the run stands, as the run is now.</summary>
    internal required Func<int> Start { get; init; }

    /// <summary>The records of the run's children, in order.</summary>
    internal List<Drawn> Records { get; } = [];

    /// <summary>What a child needs put on its element when it takes its place (alignment, a column of a grid).</summary>
    internal Action<Models.Node, UIElement, int>? Place { get; set; }

    /// <summary>Whether children may be added, taken out and moved; a run that cannot (the columns of a grid) is drawn again when they are.</summary>
    internal bool Resizable { get; init; } = true;

    /// <summary>The form and the sheet the children stand in, to draw a new one in the same.</summary>
    internal string? Form { get; init; }

    /// <summary>The sheet, dialog or popover the children stand in.</summary>
    internal string? Overlay { get; init; }

    /// <summary>Puts the record of a child drawn again where the old one stood.</summary>
    /// <param name="old">The old record.</param>
    /// <param name="fresh">The new one.</param>
    /// <returns>False when the old one is not in this run.</returns>
    internal bool Replace(Drawn old, Drawn fresh)
    {
        int at = Records.IndexOf(old);
        if (at < 0)
        {
            return false;
        }

        int place = Host.IndexOf(old.Element);
        if (place < 0)
        {
            return false;
        }

        Place?.Invoke(fresh.Node, fresh.Element, at);
        Host.Set(place, fresh.Element);

        Records[at] = fresh;
        return true;
    }

    /// <summary>Brings the run up to date with the children there are now.</summary>
    /// <param name="renderer">What draws a child that is new.</param>
    /// <param name="owner">The record of the part that holds the run.</param>
    /// <param name="now">The children now.</param>
    /// <returns>False when the run cannot be brought up to date in place; nothing was changed then.</returns>
    internal bool Update(NodeRenderer renderer, Drawn owner, IReadOnlyList<Models.Node> now)
    {
        IReadOnlyList<Models.Node> old = [.. Records.Select(record => record.Node)];
        Pairing pairing = PageRules.Pair(old, now);
        if (pairing.Refused)
        {
            PageReports.Note("a run is drawn again: two children have one id");
            return false;
        }

        if (!Resizable && (pairing.Gone.Count > 0 || now.Count != old.Count || pairing.Old.Any(at => at < 0) || pairing.Moved.Any(moved => moved)))
        {
            PageReports.Note("a run that cannot grow is drawn again: " + old.Count + " children, now " + now.Count);
            return false;
        }

        // The children that continue are brought up to date where they stand; one that cannot be is drawn again in its own place.
        List<Drawn> before = [.. Records];
        for (int at = 0; at < now.Count; at++)
        {
            if (pairing.Old[at] >= 0)
            {
                _ = renderer.Refresh(before[pairing.Old[at]], now[at]);
            }
        }

        foreach (int gone in pairing.Gone.Reverse())
        {
            int place = Host.IndexOf(Records[gone].Element);
            if (place >= 0)
            {
                Host.RemoveAt(place);
            }
        }

        List<Drawn> desired = new(now.Count);
        for (int at = 0; at < now.Count; at++)
        {
            desired.Add(pairing.Old[at] >= 0 ? Records[pairing.Old[at]] : renderer.RenderIn(owner, now[at], Form, Overlay));
        }

        // A child that keeps its order among the others is never taken out of its place (it would lose the focus); only the ones that moved are.
        int structural = pairing.Gone.Count;
        for (int at = 0; at < desired.Count; at++)
        {
            if (pairing.Moved[at])
            {
                int taken = Host.IndexOf(desired[at].Element);
                if (taken >= 0)
                {
                    Host.RemoveAt(taken);
                }

                structural++;
            }
            else if (pairing.Old[at] < 0)
            {
                structural++;
            }
        }

        renderer.Structural += structural;
        int position = Start();
        for (int at = 0; at < desired.Count; at++)
        {
            UIElement element = desired[at].Element;
            Place?.Invoke(now[at], element, at);
            if (position >= Host.Count || !ReferenceEquals(Host.At(position), element))
            {
                int found = Host.IndexOf(element);
                if (found >= 0)
                {
                    Host.RemoveAt(found);
                    if (found < position)
                    {
                        position--;
                    }
                }

                Host.Insert(position, element);
            }

            position++;
        }

        HashSet<Drawn> mine = [.. before, .. Records];
        int first = owner.Children.FindIndex(mine.Contains);
        owner.Children.RemoveAll(mine.Contains);
        foreach (Drawn record in desired)
        {
            record.Parent = owner;
        }

        owner.Children.InsertRange(first < 0 ? owner.Children.Count : Math.Min(first, owner.Children.Count), desired);
        Records.Clear();
        Records.AddRange(desired);
        return true;
    }
}

/// <summary>The screen's own words about a part, written so two views of the screen can be compared.</summary>
internal sealed class ScreenFacts(ScreenView screen, bool fields = true) : IScreenFacts
{
    /// <inheritdoc />
    public string StatesOf(string id) =>
        screen.States.TryGetValue(id, out IReadOnlyList<Models.State>? held) ? string.Join(',', held.Select(state => state.ToString())) : string.Empty;

    /// <inheritdoc />
    public string ListOf(string id) =>
        screen.Lists.TryGetValue(id, out ListState? held) ? held.Loading + "," + held.Failed + "," + held.LoadingBefore + "," + held.FailedBefore + "," + held.Across : string.Empty;

    /// <inheritdoc />
    public string RegionOf(string id) =>
        screen.Regions.TryGetValue(id, out RegionView? held) ? held.Loading + "," + held.Failure : string.Empty;

    /// <inheritdoc />
    public string FieldOf(string form, string field)
    {
        if (!fields || screen.FieldOf(form, field) is not { } held)
        {
            return string.Empty;
        }

        string value = held.Value switch
        {
            TextValue text => "t:" + text.Text,
            FlagValue flag => "f:" + flag.On,
            ManyValue many => "m:" + string.Join('\u0001', many.Values),
            FilesValue files => "F:" + string.Join('\u0001', files.Files.Select(file => file.Token)),
            RangeValue range => "r:" + range.From + "\u0001" + range.To,
            _ => "n",
        };
        string options = held.Options is null ? "-" : string.Join('\u0002', held.Options.Select(option => option.Label + "\u0001" + option.Value + "\u0001" + option.Disabled));
        return value + "\u0003" + string.Join('\u0002', held.Errors) + "\u0003" + options + "\u0003" + held.More + "," + held.Searching;
    }

    /// <inheritdoc />
    public bool IsOpen(string id) => screen.IsOpen(id);

    /// <inheritdoc />
    public bool FormBusy(string form) => screen.Forms.TryGetValue(form, out FormView? held) && held.Pending;
}

/// <summary>What one answer did to a page: how many parts were drawn again, how many stood, how many were brought up to date.</summary>
/// <param name="Screen">The screen's number.</param>
/// <param name="Kind">What was done: the first drawing, an update in place, or the page drawn again whole.</param>
/// <param name="Rendered">How many nodes were drawn (calls of the drawing) by the pass itself.</param>
/// <param name="Stood">How many parts stood, not looked at below them.</param>
/// <param name="Updated">How many were brought up to date in place.</param>
/// <param name="Replaced">How many were drawn again in their own place.</param>
/// <param name="Kinds">The kinds of the nodes that were drawn, with how many of each.</param>
/// <param name="Lazy">How many nodes were drawn since the last pass by parts that draw their own when they come into view (the cells of a table).</param>
public sealed record PageReport(long Screen, string Kind, int Rendered, int Stood, int Updated, int Replaced, string Kinds, int Lazy);

/// <summary>What the last answers did to the pages that are on the screen, for a check to read.</summary>
public static class PageReports
{
    private static readonly List<PageReport> Held = [];

    /// <summary>The latest reports, oldest first; the latest two hundred are kept.</summary>
    public static IReadOnlyList<PageReport> All
    {
        get
        {
            lock (Held)
            {
                return [.. Held];
            }
        }
    }

    /// <summary>The count of nodes that the table's rows drew when they came into view, since the app started.</summary>
    public static int CellsDrawn { get; internal set; }

    private static readonly List<string> Said = [];

    /// <summary>Why parts were drawn again in their place, the latest two hundred, for a check to read.</summary>
    public static IReadOnlyList<string> Notes
    {
        get
        {
            lock (Held)
            {
                return [.. Said];
            }
        }
    }

    internal static void Note(string what)
    {
        lock (Held)
        {
            Said.Add(what);
            if (Said.Count > 200)
            {
                Said.RemoveAt(0);
            }
        }
    }

    /// <summary>How many reports there have been in all, so a check can say which came after a moment.</summary>
    public static int Total { get; private set; }

    /// <summary>The first drawing of each screen, by the screen's number.</summary>
    public static IReadOnlyDictionary<long, PageReport> FirstDrawings
    {
        get
        {
            lock (Held)
            {
                return new Dictionary<long, PageReport>(Firsts);
            }
        }
    }

    private static readonly Dictionary<long, PageReport> Firsts = [];

    internal static void Add(PageReport report)
    {
        lock (Held)
        {
            Total++;
            if (report.Kind == "whole" && report.Rendered > 0 && !Firsts.ContainsKey(report.Screen))
            {
                Firsts[report.Screen] = report;
            }

            Held.Add(report);
            if (Held.Count > 200)
            {
                Held.RemoveAt(0);
            }
        }
    }
}
