using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Automation.Peers;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Shapes;
using Navree.Design;
using Windows.Foundation;

namespace Navree.Controls.Composition;

/// <summary>
/// Parts in columns of one width, as many columns as the room allows (by the page's count and least width), each row as high as its
/// highest part so a card is never cut. The box's own uniform grid gives every part the size of the first, which cuts the taller ones.
/// Parts that stand with no surface of their own can be told apart by a hairline between columns, so what stands at the end of one part
/// (a ring) is not taken for the start of the next.
/// </summary>
public sealed partial class FlowGrid : Panel
{
    private readonly List<Rectangle> rules = [];

    /// <summary>The most columns, or none.</summary>
    public int? Cols { get; set; }

    /// <summary>The least width of a column, or none.</summary>
    public int? Min { get; set; }

    /// <summary>The space between parts, across and down.</summary>
    public double Gap { get; set; } = 12;

    /// <summary>The brush of a hairline between columns, or none for no hairline.</summary>
    public Brush? RuleBrush { get; set; }

    private int Parts => Children.Count - rules.Count;

    private int ColumnsFor(double width) => ComposeRules.Columns(width, Cols, Min, Gap, Parts);

    /// <inheritdoc />
    protected override Size MeasureOverride(Size availableSize)
    {
        double width = double.IsInfinity(availableSize.Width) ? 800 : availableSize.Width;
        int columns = ColumnsFor(width);
        double each = ComposeRules.ColumnWidth(width, columns, Gap);
        double height = 0;
        double rowHeight = 0;
        int rows = 0;
        for (int at = 0; at < Parts; at++)
        {
            UIElement child = Children[at];
            child.Measure(new Size(each, double.PositiveInfinity));
            rowHeight = Math.Max(rowHeight, child.DesiredSize.Height);
            if ((at + 1) % columns == 0 || at == Parts - 1)
            {
                rows++;
                height += rowHeight + (at == Parts - 1 ? 0 : Gap);
                rowHeight = 0;
            }
        }

        int needed = RuleBrush is null ? 0 : rows * Math.Max(columns - 1, 0);
        while (rules.Count < needed)
        {
            Rectangle rule = new() { Width = 1, Fill = RuleBrush, IsHitTestVisible = false };
            AutomationProperties.SetAccessibilityView(rule, AccessibilityView.Raw);
            rules.Add(rule);
            Children.Add(rule);
        }

        for (int at = 0; at < rules.Count; at++)
        {
            rules[at].Visibility = at < needed ? Visibility.Visible : Visibility.Collapsed;
            rules[at].Measure(new Size(1, 0));
        }

        return new Size(width, height);
    }

    /// <inheritdoc />
    protected override Size ArrangeOverride(Size finalSize)
    {
        int columns = ColumnsFor(finalSize.Width);
        double each = ComposeRules.ColumnWidth(finalSize.Width, columns, Gap);
        double y = 0;
        int rule = 0;
        for (int start = 0; start < Parts; start += columns)
        {
            double rowHeight = 0;
            int end = Math.Min(start + columns, Parts);
            for (int at = start; at < end; at++)
            {
                rowHeight = Math.Max(rowHeight, Children[at].DesiredSize.Height);
            }

            for (int at = start; at < end; at++)
            {
                double x = (at - start) * (each + Gap);
                Children[at].Arrange(new Rect(x, y, each, rowHeight));
                if (at > start && rule < rules.Count)
                {
                    rules[rule++].Arrange(new Rect(x - (Gap / 2), y, 1, rowHeight));
                }
            }

            y += rowHeight + Gap;
        }

        for (; rule < rules.Count; rule++)
        {
            rules[rule].Arrange(new Rect(0, 0, 0, 0));
        }

        return finalSize;
    }
}
