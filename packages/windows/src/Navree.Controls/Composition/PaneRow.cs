using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Windows.Foundation;

namespace Navree.Controls.Composition;

/// <summary>
/// Two panes of a split screen: side by side, the first at a width of its own and the second taking the rest, when the room is wide enough for
/// both, and one under the other when it is not. The room is what this panel is given, not the window's, so a split inside a pane of another
/// split chooses by the room it has left. It lays out in the frame of its element, so right to left puts the first pane at the right.
/// </summary>
public sealed partial class PaneRow : Panel
{
    /// <summary>The width of the first pane when the two stand side by side.</summary>
    public double FirstWidth { get; set; } = 360;

    /// <summary>The space between the panes.</summary>
    public double Spacing { get; set; } = 24;

    /// <summary>The least width the two need to stand side by side.</summary>
    public double SideBySideFrom { get; set; } = 760;

    /// <summary>Whether, one under the other, the second comes first: the server says it is the pane in hand.</summary>
    public bool SecondFirstWhenStacked { get; set; }

    /// <inheritdoc />
    protected override Size MeasureOverride(Size availableSize)
    {
        double room = availableSize.Width;
        bool beside = Beside(room);
        double width = 0;
        double height = 0;
        for (int at = 0; at < Children.Count; at++)
        {
            UIElement child = Children[at];
            double give = double.IsInfinity(room) ? double.PositiveInfinity : beside ? (at == 0 ? FirstWidth : Math.Max(0, room - FirstWidth - Spacing)) : room;
            child.Measure(new Size(give, double.PositiveInfinity));
            if (beside)
            {
                height = Math.Max(height, child.DesiredSize.Height);
                width += child.DesiredSize.Width + (at > 0 ? Spacing : 0);
            }
            else
            {
                height += child.DesiredSize.Height + (at > 0 ? Spacing : 0);
                width = Math.Max(width, child.DesiredSize.Width);
            }
        }

        return new Size(double.IsInfinity(room) ? width : room, height);
    }

    /// <inheritdoc />
    protected override Size ArrangeOverride(Size finalSize)
    {
        bool beside = Beside(finalSize.Width);
        if (beside)
        {
            double left = 0;
            for (int at = 0; at < Children.Count; at++)
            {
                double width = at == 0 ? FirstWidth : Math.Max(0, finalSize.Width - FirstWidth - Spacing);
                Children[at].Arrange(new Rect(left, 0, width, finalSize.Height));
                left += width + Spacing;
            }

            return finalSize;
        }

        double top = 0;
        IEnumerable<UIElement> order = SecondFirstWhenStacked ? System.Linq.Enumerable.Reverse(Children) : Children;
        foreach (UIElement child in order)
        {
            child.Arrange(new Rect(0, top, finalSize.Width, child.DesiredSize.Height));
            top += child.DesiredSize.Height + Spacing;
        }

        return finalSize;
    }

    private bool Beside(double room) => Children.Count == 2 && !double.IsInfinity(room) && room >= SideBySideFrom;
}
