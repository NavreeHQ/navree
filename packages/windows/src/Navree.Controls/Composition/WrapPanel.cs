using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Windows.Foundation;

namespace Navree.Controls.Composition;

/// <summary>
/// Parts side by side, onto a new line where the room runs out: a stack that wraps, and the keys of a chart. The box has no panel that
/// does this, so it is ours. It lays out in the frame of its element, so right to left puts the first part at the right.
/// </summary>
public sealed partial class WrapPanel : Panel
{
    /// <summary>The space between two parts on a line.</summary>
    public double HorizontalSpacing { get; set; } = 14;

    /// <summary>The space between two lines.</summary>
    public double VerticalSpacing { get; set; } = 6;

    /// <inheritdoc />
    protected override Size MeasureOverride(Size availableSize)
    {
        double limit = double.IsInfinity(availableSize.Width) ? double.PositiveInfinity : availableSize.Width;
        foreach (UIElement child in Children)
        {
            child.Measure(new Size(limit, double.PositiveInfinity));
        }

        Size used = Place(limit, null);
        return new Size(double.IsInfinity(limit) ? used.Width : Math.Min(limit, used.Width), used.Height);
    }

    /// <inheritdoc />
    protected override Size ArrangeOverride(Size finalSize)
    {
        Place(finalSize.Width, (child, rect) => child.Arrange(rect));
        return finalSize;
    }

    // Walks the parts in order, wrapping where the next would pass the limit; tells where each stands.
    private Size Place(double limit, Action<UIElement, Rect>? arrange)
    {
        double x = 0;
        double y = 0;
        double line = 0;
        double widest = 0;
        foreach (UIElement child in Children)
        {
            Size size = child.DesiredSize;
            if (x > 0 && x + size.Width > limit)
            {
                y += line + VerticalSpacing;
                x = 0;
                line = 0;
            }

            arrange?.Invoke(child, new Rect(x, y, size.Width, size.Height));
            x += size.Width;
            widest = Math.Max(widest, x);
            x += HorizontalSpacing;
            line = Math.Max(line, size.Height);
        }

        return new Size(widest, Children.Count == 0 ? 0 : y + line);
    }
}
