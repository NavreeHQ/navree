using Navree.Protocol;

namespace Navree.Design;

/// <summary>Reading a tree the core has made ready: what a node holds, its id, and the few things placement looks for. Nothing here changes a tree.</summary>
public static class Tree
{
    /// <summary>The node's id, when it has one.</summary>
    /// <param name="node">The node.</param>
    /// <returns>Its id.</returns>
    public static string? IdOf(Models.Node node) =>
        node switch
        {
            Models.Stack x => x.Id,
            Models.Grid x => x.Id,
            Models.Section x => x.Id,
            Models.List x => x.Id,
            Models.Row x => x.Id,
            Models.Split x => x.Id,
            Models.Tabs x => x.Id,
            Models.Disclosure x => x.Id,
            Models.Divider x => x.Id,
            Models.Spacer x => x.Id,
            Models.Region x => x.Id,
            Models.Text x => x.Id,
            Models.Rich x => x.Id,
            Models.Image x => x.Id,
            Models.Icon x => x.Id,
            Models.Avatar x => x.Id,
            Models.Badge x => x.Id,
            Models.Progress x => x.Id,
            Models.Banner x => x.Id,
            Models.Empty x => x.Id,
            Models.Facts x => x.Id,
            Models.Table x => x.Id,
            Models.Chart x => x.Id,
            Models.Code x => x.Id,
            Models.Button x => x.Id,
            Models.Link x => x.Id,
            Models.Menu x => x.Id,
            Models.Form x => x.Id,
            Models.Field x => x.Id,
            Models.Sheet x => x.Id,
            Models.Dialog x => x.Id,
            Models.Popover x => x.Id,
            Models.NativeView x => x.Id,
            Models.WebRegion x => x.Id,
            Models.Toolbar x => x.Id,
            Models.Gauge x => x.Id,
            Models.Figure x => x.Id,
            Models.Timeline x => x.Id,
            Models.FeedRow x => x.Id,
            Models.Message x => x.Id,
            Models.Pane x => x.Id,
            Models.Card x => x.Id,
            Models.NodeUnknown x => x.Id,
            _ => null,
        };

    /// <summary>The nodes directly inside this one, in order, overlays included.</summary>
    /// <param name="node">The node.</param>
    /// <returns>Its children.</returns>
    public static IReadOnlyList<Models.Node> ChildNodes(Models.Node node) =>
        node switch
        {
            Models.Stack x => x.Children,
            Models.Grid x => x.Children,
            Models.Section x => [.. x.Actions, .. x.Children, .. x.Footer],
            Models.List x => x.Children,
            Models.Row x => [.. new Models.Node?[] { x.Leading, x.Trailing, x.Badge }.OfType<Models.Node>(), .. x.Detail, .. x.Menu, .. x.Swipe],
            Models.Split x => [x.Primary, x.Detail],
            Models.Tabs x => [.. x.Items.SelectMany(item => item.Children)],
            Models.Disclosure x => x.Children,
            Models.Region x => x.Children,
            Models.Banner x => [.. new Models.Node?[] { x.Icon }.OfType<Models.Node>(), .. x.Actions],
            Models.Empty x => [.. new Models.Node?[] { x.Icon }.OfType<Models.Node>(), .. x.Actions],
            Models.Facts x => [.. x.Items.SelectMany(item => item.Value)],
            Models.Table x => [.. new Models.Node?[] { x.Query, x.Empty }.OfType<Models.Node>(), .. x.Rows.SelectMany(row => row.Cells.Concat(row.Actions))],
            Models.Link x => x.Children,
            Models.Menu x => x.Items,
            Models.Form x => x.Children,
            Models.Sheet x => [.. x.Children, .. x.Actions],
            Models.Dialog x => [.. x.Children, .. x.Actions],
            Models.Popover x => x.Children,
            Models.Toolbar x => [.. x.Items.SelectMany(item => item.Menu)],
            Models.Timeline x => [.. x.Items.SelectMany(item => item.Detail)],
            Models.FeedRow x => x.Detail,
            Models.Message x => x.Body,
            Models.Pane x => x.Children,
            Models.Figure x => [.. new Models.Node?[] { x.Aside, x.Chart }.OfType<Models.Node>()],
            Models.Card x => [.. x.Children, .. x.Menu],
            Models.Layers x => x.Children,
            Models.Carousel x => x.Children,
            Models.Adaptive x => [.. x.Compact, .. x.Wide],
            Models.DayPager x => [.. new Models.Node?[] { x.Scores }.OfType<Models.Node>()],
            Models.NativeView x => [.. new Models.Node?[] { x.Alt }.OfType<Models.Node>()],
            Models.NodeUnknown x => [.. new Models.Node?[] { x.Alt }.OfType<Models.Node>()],
            _ => [],
        };

    /// <summary>This node and every node inside it, depth first, in order.</summary>
    /// <param name="node">The node.</param>
    /// <returns>The nodes.</returns>
    public static IEnumerable<Models.Node> Walk(Models.Node node)
    {
        Stack<Models.Node> pending = new();
        pending.Push(node);
        while (pending.Count > 0)
        {
            Models.Node next = pending.Pop();
            yield return next;
            IReadOnlyList<Models.Node> inner = ChildNodes(next);
            for (int at = inner.Count - 1; at >= 0; at--)
            {
                pending.Push(inner[at]);
            }
        }
    }

    /// <summary>Every node of a screen: its actions, then its body.</summary>
    /// <param name="screen">The screen.</param>
    /// <returns>The nodes.</returns>
    public static IEnumerable<Models.Node> Nodes(Models.Screen screen) =>
        screen.Actions.Concat([screen.Body]).SelectMany(Walk);

    /// <summary>The fields of a form, nested ones included, but not those of a form inside it.</summary>
    /// <param name="form">The form.</param>
    /// <returns>The fields.</returns>
    public static IReadOnlyList<Models.Field> FieldsOf(Models.Form form)
    {
        List<Models.Field> fields = [];

        void Visit(Models.Node node)
        {
            switch (node)
            {
                case Models.Field field:
                    fields.Add(field);
                    break;
                case Models.Form:
                    break;
                default:
                    foreach (Models.Node child in ChildNodes(node))
                    {
                        Visit(child);
                    }

                    break;
            }
        }

        foreach (Models.Node child in form.Children)
        {
            Visit(child);
        }

        return fields;
    }

    /// <summary>The id a form goes by in the view state: its own, or <c>@</c> and its address when it came with none.</summary>
    /// <param name="form">The form.</param>
    /// <returns>The id.</returns>
    public static string FormId(Models.Form form) => form.Id ?? "@" + form.Action;

    /// <summary>Whether this action writes: a submit, or a request that is not a plain read.</summary>
    /// <param name="action">The action.</param>
    /// <returns>Whether it writes.</returns>
    public static bool Writes(Models.Action? action) =>
        action switch
        {
            Models.ActionRequest request => request.Method != Models.Method.Get,
            Models.ActionSubmit => true,
            _ => false,
        };
}
