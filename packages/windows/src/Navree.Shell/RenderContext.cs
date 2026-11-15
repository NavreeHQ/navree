using Microsoft.UI.Xaml;
using Navree.Design;
using Navree.Protocol;
using Navree.Runtime;

namespace Navree.Shell;

/// <summary>A sheet, dialog or popover of a screen's tree, found while it was drawn: shown over the screen when the core says it is open.</summary>
/// <param name="Node">The overlay's node.</param>
/// <param name="Id">Its id.</param>
internal sealed record OverlayRequest(Models.Node Node, string Id);

/// <summary>
/// What drawing one screen at one width class needs to know, and what it gives back besides the element: the overlays it found and the fields it drew.
/// It lives as long as the screen is on the screen at that width; the screen it holds is set again at each answer, so whatever reads it when a
/// press happens reads the screen as it is then, never the one a control was drawn from.
/// </summary>
internal sealed class RenderContext
{
    /// <summary>The screen as it is now.</summary>
    internal required ScreenView Screen { get; set; }

    /// <summary>Tells the core something happened.</summary>
    internal required Action<string> Send { get; init; }

    /// <summary>How wide the window is.</summary>
    internal WidthClass Width { get; init; } = WidthClass.Expanded;

    /// <summary>Loads a picture's bytes by a whole address the core made, or null.</summary>
    internal Func<string, Task<byte[]?>>? Pictures { get; init; }

    /// <summary>The buttons drawn, by the key that names them (their id, or their words), so the one that opened a popover can be found again after the screen is drawn again.</summary>
    internal Dictionary<string, FrameworkElement> Triggers { get; } = [];

    /// <summary>The sheets, dialogs and popovers found in the tree.</summary>
    internal List<OverlayRequest> Overlays { get; } = [];

    /// <summary>The fields drawn, by form and field id, so their state can be shown again without drawing the screen again.</summary>
    internal Dictionary<(string Form, string Field), FieldHandle> Fields { get; } = [];

    /// <summary>Where a person's widths of a table's columns are kept, or none.</summary>
    internal TableLayouts? Layouts { get; init; }

    /// <summary>The element that raises assistive technology's notices.</summary>
    internal FrameworkElement? Announcer { get; init; }

    /// <summary>The fields that were drawn outside a drawing of the tree (a table's cells when they come into view, a sheet's body), by form and id.</summary>
    internal Dictionary<(string Form, string Field), FieldHandle> LooseFields { get; } = [];

    /// <summary>The buttons that were drawn outside a drawing of the tree, by key.</summary>
    internal Dictionary<string, FrameworkElement> LooseTriggers { get; } = [];

    /// <summary>The records of the parts of the screen's page that is drawn: the page itself, and the body in it.</summary>
    internal Drawn? Page { get; set; }

    /// <summary>
    /// A number that moves on when the sheets, dialogs and popovers of the tree changed (an overlay came, went, or holds something else), so what was
    /// made from them knows to be drawn again. The fields of an overlay are not in it: they are shown in place.
    /// </summary>
    internal int Version { get; private set; }

    private ScreenView? versionScreen;

    /// <summary>How many nodes were drawn by calls of the drawing since the count was last taken.</summary>
    internal int Rendered { get; set; }

    /// <summary>How many of each kind were drawn since the count was last taken.</summary>
    internal Dictionary<string, int> RenderedKinds { get; } = [];

    /// <summary>Whether the first drawing of the screen is done; a part that joins after it comes in as it says.</summary>
    internal bool Settled { get; set; }

    /// <summary>Makes the lists of what was found (overlays, fields, buttons) again from the records that stand.</summary>
    internal void Rebuild()
    {
        List<OverlayRequest> before = [.. Overlays];
        Overlays.Clear();
        Fields.Clear();
        Triggers.Clear();
        foreach (KeyValuePair<(string Form, string Field), FieldHandle> loose in LooseFields)
        {
            Fields[loose.Key] = loose.Value;
        }

        foreach (KeyValuePair<string, FrameworkElement> loose in LooseTriggers)
        {
            Triggers[loose.Key] = loose.Value;
        }

        if (Page is not null)
        {
            foreach (Drawn record in Page.Walk())
            {
                Overlays.AddRange(record.Overlays);
                foreach (((string Form, string Field) key, FieldHandle handle) in record.Fields)
                {
                    Fields[key] = handle;
                }

                foreach ((string key, FrameworkElement element) in record.Triggers)
                {
                    Triggers[key] = element;
                }
            }
        }

        bool changed = versionScreen is null || before.Count != Overlays.Count;
        if (!changed)
        {
            ScreenFacts was = new(versionScreen!, fields: false);
            ScreenFacts now = new(Screen, fields: false);
            for (int at = 0; at < before.Count && !changed; at++)
            {
                changed = before[at].Id != Overlays[at].Id
                    || !Design.Pages.PageRules.Same(before[at].Node, Overlays[at].Node)
                    || !Design.Pages.PageRules.SameAround(Overlays[at].Node, was, now);
            }
        }

        if (changed)
        {
            Version++;
            versionScreen = Screen;
        }
    }

    /// <summary>Shows each field what the screen holds for it: in place, so the box a person is typing in keeps its place.</summary>
    internal void ShowFields()
    {
        foreach (((string form, string field), FieldHandle handle) in Fields.ToArray())
        {
            handle.Show(Screen.FieldOf(form, field));
        }
    }

    /// <summary>Takes the count of what was drawn, and starts it again.</summary>
    /// <returns>How many nodes, and how many of each kind.</returns>
    internal (int Total, string Kinds) TakeCount()
    {
        int total = Rendered;
        string kinds = string.Join(", ", RenderedKinds.OrderByDescending(pair => pair.Value).Select(pair => pair.Key + " " + pair.Value));
        Rendered = 0;
        RenderedKinds.Clear();
        return (total, kinds);
    }
}

/// <summary>A field that is on the screen, and how to show its state.</summary>
internal sealed class FieldHandle
{
    /// <summary>Shows what the field holds and what is wrong with it.</summary>
    internal required Action<FieldView?> Show { get; init; }
}
