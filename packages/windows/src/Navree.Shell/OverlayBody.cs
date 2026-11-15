using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Navree.Design.Pages;
using Navree.Protocol;

namespace Navree.Shell;

/// <summary>
/// What a sheet, a dialog or a popover of the page holds, drawn once and kept: when an answer changes the screen, the body is brought up to date in
/// place (a server's error stands at its field, a button that is busy says so) and the sheet that is open stays the same one, with the focus in it.
/// </summary>
internal sealed class OverlayBodyView
{
    private readonly RenderContext context;
    private readonly NodeRenderer renderer;
    private readonly StackPanel body = new() { Spacing = 12 };
    private readonly Group children;
    private readonly Group? actions;

    /// <summary>Draws the body of an overlay.</summary>
    /// <param name="context">The drawing of the screen it is an overlay of.</param>
    /// <param name="request">The overlay.</param>
    internal OverlayBodyView(RenderContext context, OverlayRequest request)
    {
        this.context = context;
        renderer = new NodeRenderer(context);
        Owner = new Drawn(request.Node) { Screen = context.Screen, Element = body };
        switch (request.Node)
        {
            case Models.Sheet sheet:
                if (sheet.Subtitle is not null)
                {
                    body.Children.Add(Look.Label(sheet.Subtitle, muted: true));
                }

                int lead = body.Children.Count;
                children = renderer.DrawRun(Owner, sheet.Children, new PanelHost(body.Children), () => lead, request.Id);
                actions = renderer.DrawRun(Owner, sheet.Actions, new PanelHost(body.Children), () => lead + children.Records.Count, request.Id);
                break;
            case Models.Dialog dialog:
                if (dialog.Text is not null)
                {
                    body.Children.Add(Look.Label(dialog.Text));
                }

                int start = body.Children.Count;
                children = renderer.DrawRun(Owner, dialog.Children, new PanelHost(body.Children), () => start, request.Id);
                StackPanel row = new() { Orientation = Orientation.Horizontal, Spacing = 8, HorizontalAlignment = HorizontalAlignment.Right };
                body.Children.Add(row);
                actions = renderer.DrawRun(Owner, dialog.Actions, new PanelHost(row.Children), () => 0, request.Id);
                break;
            case Models.Popover popover:
                children = renderer.DrawRun(Owner, popover.Children, new PanelHost(body.Children), () => 0, request.Id);
                break;
            default:
                children = new Group { Host = new PanelHost(body.Children), Start = () => 0 };
                break;
        }

        context.AddRoot(Owner);
    }

    /// <summary>The record of the body: the parts drawn for it are its children.</summary>
    internal Drawn Owner { get; }

    /// <summary>The drawing of the screen the body was drawn for.</summary>
    internal RenderContext Context => context;

    /// <summary>The element that holds the body.</summary>
    internal UIElement Element => body;

    /// <summary>Brings the body up to date with the overlay as the tree has it now.</summary>
    /// <param name="now">The overlay.</param>
    /// <returns>False when the body has to be drawn again (the overlay is another kind, or its own words changed beyond its parts).</returns>
    internal bool Update(OverlayRequest now)
    {
        if (now.Node.GetType() != Owner.Node.GetType())
        {
            return false;
        }

        renderer.Failed = false;

        bool done = (Owner.Node, now.Node) switch
        {
            (Models.Sheet was, Models.Sheet next) => PageRules.Same(Bare(was), Bare(next)) && children.Update(renderer, Owner, next.Children) && (actions?.Update(renderer, Owner, next.Actions) ?? true),
            (Models.Dialog was, Models.Dialog next) => PageRules.Same(Bare(was), Bare(next)) && children.Update(renderer, Owner, next.Children) && (actions?.Update(renderer, Owner, next.Actions) ?? true),
            (Models.Popover was, Models.Popover next) => PageRules.Same(Bare(was), Bare(next)) && children.Update(renderer, Owner, next.Children),
            _ => false,
        };
        if (done && !renderer.Failed)
        {
            Owner.Node = now.Node;
            Owner.Screen = context.Screen;
            return true;
        }

        PageReports.Note("an overlay body is drawn again: " + now.Node.GetType().Name + " " + now.Id + (renderer.Failed ? " (a part could not be put in its place)" : " (its own words or its parts changed beyond what can be brought up to date)"));
        return false;
    }

    private static Models.Sheet Bare(Models.Sheet node) => node with { Children = [], Actions = [] };

    private static Models.Dialog Bare(Models.Dialog node) => node with { Children = [], Actions = [] };

    private static Models.Popover Bare(Models.Popover node) => node with { Children = [] };
}
