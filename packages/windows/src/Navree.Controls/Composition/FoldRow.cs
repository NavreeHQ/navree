using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Windows.Foundation;

namespace Navree.Controls.Composition;

/// <summary>
/// A row of parts that stands across while they fit side by side at their own widths and goes on to a second line when they do not: a card's title
/// and the button of its menu stand in one line in a wide card, and in a narrow one the button goes under the title and the title keeps the whole
/// line, so its words wrap at the spaces and no word is broken (a word wider than the whole line is the only one that is). A part alone on a line
/// takes the width of the line. It lays out in the frame of its element, so right to left puts the first part at the right.
/// </summary>
public sealed partial class FoldRow : Panel
{
    private readonly List<List<UIElement>> lines = [];

    /// <summary>The space between parts of a line.</summary>
    public double Spacing { get; set; } = 8;

    /// <summary>The space between lines.</summary>
    public double LineSpacing { get; set; } = 4;

    /// <summary>Whether the first part stands at the start and the last at the end of the line, the rest spread between them.</summary>
    public bool Between { get; set; }

    /// <inheritdoc />
    protected override Size MeasureOverride(Size availableSize)
    {
        double room = availableSize.Width;
        foreach (UIElement child in Children)
        {
            child.Measure(new Size(double.PositiveInfinity, double.PositiveInfinity));
        }

        lines.Clear();
        List<UIElement> current = [];
        double used = 0;
        foreach (UIElement child in Children)
        {
            double wanted = child.DesiredSize.Width;
            if (current.Count > 0 && !double.IsInfinity(room) && used + Spacing + wanted > room)
            {
                lines.Add(current);
                current = [];
                used = 0;
            }

            used += (current.Count > 0 ? Spacing : 0) + wanted;
            current.Add(child);
        }

        if (current.Count > 0)
        {
            lines.Add(current);
        }

        double width = 0;
        double height = 0;
        foreach (List<UIElement> line in lines)
        {
            if (line.Count == 1 && !double.IsInfinity(room) && line[0].DesiredSize.Width > room)
            {
                // Alone on its line and wider than the line: it takes the line, and its own words wrap to it.
                line[0].Measure(new Size(room, double.PositiveInfinity));
            }

            width = Math.Max(width, line.Sum(child => child.DesiredSize.Width) + (Spacing * (line.Count - 1)));
            height += line.Max(child => child.DesiredSize.Height);
        }

        height += LineSpacing * Math.Max(0, lines.Count - 1);
        return new Size(Between && !double.IsInfinity(room) ? room : Math.Min(width, double.IsInfinity(room) ? width : room), height);
    }

    /// <inheritdoc />
    protected override Size ArrangeOverride(Size finalSize)
    {
        double top = 0;
        for (int at = 0; at < lines.Count; at++)
        {
            List<UIElement> line = lines[at];
            double height = line.Max(child => child.DesiredSize.Height);
            double natural = line.Sum(child => child.DesiredSize.Width);
            double gap = Spacing;
            double left = 0;
            if (Between && line.Count > 1)
            {
                gap = Math.Max(Spacing, (finalSize.Width - natural) / (line.Count - 1));
            }
            else if (Between && at > 0 && line.Count == 1)
            {
                // The part that went to a line of its own stands at the end, where it stood.
                left = Math.Max(0, finalSize.Width - natural);
            }

            foreach (UIElement child in line)
            {
                double width = Math.Min(child.DesiredSize.Width, finalSize.Width);
                child.Arrange(new Rect(left, top + ((height - child.DesiredSize.Height) / 2), width, child.DesiredSize.Height));
                left += width + gap;
            }

            top += height + LineSpacing;
        }

        return finalSize;
    }
}
