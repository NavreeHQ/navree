using Microsoft.UI.Xaml;
using Navree.Design;
using Navree.Design.Pages;
using Navree.Protocol;
using Navree.Runtime;

namespace Navree.Shell;

// Bringing a page that is on the screen up to date after an answer: a part stands if what it was drawn from did not change; a part that changed
// brings itself up to date if its kind can; else it is drawn again alone, in its own place.
internal sealed partial class NodeRenderer
{
    /// <summary>How many parts stood, not looked at below them, since the count was last taken.</summary>
    internal int Stood { get; set; }

    /// <summary>How many were brought up to date in place.</summary>
    internal int Updated { get; set; }

    /// <summary>How many were drawn again in their own place.</summary>
    internal int Replaced { get; set; }

    /// <summary>Whether a part could not be put in its place, so the whole body is to be drawn again.</summary>
    internal bool Failed { get; set; }

    /// <summary>Draws a node as a child of a record that is not the one being drawn (the body of a page, under the page).</summary>
    /// <param name="parent">The record the node's record is a child of.</param>
    /// <param name="node">The node.</param>
    /// <returns>The node's record.</returns>
    internal Drawn RenderUnder(Drawn parent, Models.Node node)
    {
        Drawn? outer = current;
        current = parent;
        try
        {
            return RenderRecord(node);
        }
        finally
        {
            current = outer;
        }
    }

    /// <summary>Draws a node that is to be put into a part that stands: in the form and sheet that part draws its children in.</summary>
    /// <param name="owner">The record of the part it is put into.</param>
    /// <param name="node">The node.</param>
    /// <param name="form">The form the children stand in.</param>
    /// <param name="overlay">The sheet, dialog or popover the children stand in.</param>
    /// <returns>The node's record, not yet in its owner.</returns>
    internal Drawn RenderIn(Drawn owner, Models.Node node, string? form, string? overlay)
    {
        (string? outerForm, string? outerOverlay, Drawn? outer, Drawn? outerAdopter) = (currentForm, currentOverlay, current, adopter);
        currentForm = form;
        currentOverlay = overlay;
        current = null;
        adopter = owner;
        try
        {
            Drawn record = RenderRecord(node);
            record.Parent = owner;
            return record;
        }
        finally
        {
            currentForm = outerForm;
            currentOverlay = outerOverlay;
            current = outer;
            adopter = outerAdopter;
        }
    }

    /// <summary>Draws a node that no part of the page holds (a cell of a table that came into view): what it finds is kept in the context's own lists.</summary>
    /// <param name="node">The node.</param>
    /// <returns>The element.</returns>
    internal UIElement RenderLoose(Models.Node node)
    {
        Drawn? outer = current;
        current = null;
        try
        {
            Drawn record = RenderRecord(node);
            foreach (Drawn part in record.Walk())
            {
                foreach (((string Form, string Field) key, FieldHandle handle) in part.Fields)
                {
                    context.LooseFields[key] = handle;
                    context.Fields[key] = handle;
                }

                foreach ((string key, FrameworkElement element) in part.Triggers)
                {
                    context.LooseTriggers[key] = element;
                    context.Triggers[key] = element;
                }
            }

            PageReports.CellsDrawn++;
            return record.Element;
        }
        finally
        {
            current = outer;
        }
    }

    /// <summary>
    /// Brings a part that is drawn up to date with the node that is there now. It stands if nothing it was drawn from changed; else it is brought
    /// up to date in place if its kind can be; else it is drawn again, alone, in its own place.
    /// </summary>
    /// <param name="old">The part's record.</param>
    /// <param name="now">The node that stands there now.</param>
    /// <returns>The record that stands in its place: the same one, or the one of the part drawn again.</returns>
    internal Drawn Refresh(Drawn old, Models.Node now)
    {
        ScreenView screen = context.Screen;
        if (old.Node.GetType() != now.GetType() || Tree.IdOf(old.Node) != Tree.IdOf(now))
        {
            return Replace(old, now);
        }

        if (PageRules.Same(old.Node, now) && PageRules.SameAround(now, new ScreenFacts(old.Screen, fields: false), new ScreenFacts(screen, fields: false)))
        {
            Stood++;
            return old;
        }

        if (old.Update is { } update && TryUpdate(update, now))
        {
            old.Node = now;
            old.Screen = screen;
            Updated++;
            return old;
        }

        return Replace(old, now);
    }

    /// <summary>Draws a part again in its own place, whether or not it stands.</summary>
    /// <param name="old">The part's record.</param>
    /// <param name="now">The node that stands there now.</param>
    /// <returns>The record of the part drawn.</returns>
    internal Drawn Redraw(Drawn old, Models.Node now) => Replace(old, now);

    private static bool TryUpdate(Func<Models.Node, bool> update, Models.Node now)
    {
        try
        {
            return update(now);
        }
        catch (Exception problem) when (problem is not OutOfMemoryException)
        {
            System.Diagnostics.Debug.WriteLine("a part could not be brought up to date: " + problem.Message);
            return false;
        }
    }

    // Draws the node again where the old part stood, in the same form and sheet, and puts it in the old one's place.
    private Drawn Replace(Drawn old, Models.Node now)
    {
        (string? outerForm, string? outerOverlay, bool outerCovered, string? outerWords, Drawn? outer, Drawn? outerAdopter) = (currentForm, currentOverlay, titleCovered, wordsAbove, current, adopter);
        adopter = old.Parent;
        currentForm = old.Form;
        currentOverlay = old.Overlay;
        titleCovered = old.CoveredBefore;
        wordsAbove = old.WordsBefore;
        current = null;
        Drawn fresh;
        try
        {
            fresh = RenderRecord(now);
        }
        finally
        {
            currentForm = outerForm;
            currentOverlay = outerOverlay;
            titleCovered = outerCovered;
            wordsAbove = outerWords;
            current = outer;
            adopter = outerAdopter;
        }

        if (old.Parent is { } parent && parent.Swap(old, fresh))
        {
            Replaced++;
            return fresh;
        }

        Failed = true;
        return old;
    }
}
