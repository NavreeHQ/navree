using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Navree.Design.Pages;
using Navree.Protocol;

namespace Navree.Shell;

// How a part that holds children is made to bring them up to date: its children are drawn into a run, each child's record is kept in it, and the
// part says how to be brought up to date when its own properties did not change and only its children did.
internal sealed partial class NodeRenderer
{
    /// <summary>Draws children one after the other into a host, and keeps their records in a run the part being drawn can bring up to date.</summary>
    /// <param name="children">The children.</param>
    /// <param name="host">Where their elements go.</param>
    /// <param name="start">The place in the host where the run begins, as the run is now.</param>
    /// <param name="place">What a child needs put on its element when it takes its place.</param>
    /// <param name="resizable">Whether children may come and go without the part being drawn again.</param>
    /// <returns>The run.</returns>
    private Group Run(IReadOnlyList<Models.Node> children, IHost host, Func<int> start, Action<Models.Node, UIElement, int>? place = null, bool resizable = true)
    {
        Group group = new() { Host = host, Start = start, Place = place, Resizable = resizable, Form = currentForm, Overlay = currentOverlay };
        for (int at = 0; at < children.Count; at++)
        {
            Drawn record = current is null ? new Drawn(children[at]) { Element = RenderLoose(children[at]) } : RenderRecord(children[at]);
            group.Records.Add(record);
            place?.Invoke(children[at], record.Element, at);
            host.Insert(host.Count, record.Element);
        }

        current?.Groups.Add(group);
        return group;
    }

    /// <summary>
    /// Says how the part being drawn is brought up to date: it is, in place, when its own properties are the same (its children aside) and its run of
    /// children can be; the nodes it draws that are not in the run must stand too, or it is drawn again.
    /// </summary>
    /// <typeparam name="T">The kind of the part.</typeparam>
    /// <param name="withoutChildren">The node with its children left out: what is compared.</param>
    /// <param name="group">The run.</param>
    /// <param name="children">The children of a node.</param>
    /// <param name="stillSo">Whether what was drawn from the old node is right for the new (the way a stack runs), or none.</param>
    /// <param name="others">The nodes of a part that are drawn outside its run (a menu), which must stand.</param>
    private void UpdateRun<T>(Func<T, T> withoutChildren, Group group, Func<T, IReadOnlyList<Models.Node>> children, Func<T, bool>? stillSo = null, Func<T, IEnumerable<Models.Node>>? others = null)
        where T : Models.Node
    {
        Drawn me = current!;
        me.Update = now =>
        {
            if (now is not T next || me.Node is not T previous)
            {
                return false;
            }

            if (stillSo is not null && !stillSo(next))
            {
                return false;
            }

            if (!PageRules.Same(withoutChildren(previous), withoutChildren(next)) || !OthersStand(me, others, next))
            {
                return false;
            }

            return group.Update(this, me, children(next));
        };
    }

    // The nodes a part draws outside its run have the same states in the screen as they had.
    private bool OthersStand<T>(Drawn me, Func<T, IEnumerable<Models.Node>>? others, T next)
        where T : Models.Node
    {
        if (others is null)
        {
            return true;
        }

        ScreenFacts was = new(me.Screen, fields: false);
        ScreenFacts now = new(context.Screen, fields: false);
        return others(next).All(node => PageRules.SameAround(node, was, now));
    }
}
