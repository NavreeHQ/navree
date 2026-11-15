using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Navree.Design;
using Windows.Foundation;

namespace Navree.Controls.Composition;

/// <summary>
/// Parts in columns of one width, as many columns as the room allows (by the page's count and least width), each row as high as its
/// highest part so a card is never cut. The box's own uniform grid gives every part the size of the first, which cuts the taller ones.
/// </summary>
public sealed partial class FlowGrid : Panel
{
    /// <summary>The most columns, or none.</summary>
    public int? Cols { get; set; }

    /// <summary>The least width of a column, or none.</summary>
    public int? Min { get; set; }

    /// <summary>The space between parts, across and down.</summary>
    public double Gap { get; set; } = 12;

    private int ColumnsFor(double width) => ComposeRules.Columns(width, Cols, Min, Gap, Children.Count);

    /// <inheritdoc />
    protected override Size MeasureOverride(Size availableSize)
    {
        double width = double.IsInfinity(availableSize.Width) ? 800 : availableSize.Width;
        int columns = ColumnsFor(width);
        double each = ComposeRules.ColumnWidth(width, columns, Gap);
        double height = 0;
        double rowHeight = 0;
        int at = 0;
        foreach (UIElement child in Children)
        {
            child.Measure(new Size(each, double.PositiveInfinity));
            rowHeight = Math.Max(rowHeight, child.DesiredSize.Height);
            at++;
            if (at % columns == 0 || at == Children.Count)
            {
                height += rowHeight + (at == Children.Count ? 0 : Gap);
                rowHeight = 0;
            }
        }

        return new Size(width, height);
    }

    /// <inheritdoc />
    protected override Size ArrangeOverride(Size finalSize)
    {
        int columns = ColumnsFor(finalSize.Width);
        double each = ComposeRules.ColumnWidth(finalSize.Width, columns, Gap);
        double y = 0;
        for (int start = 0; start < Children.Count; start += columns)
        {
            double rowHeight = 0;
            for (int at = start; at < Math.Min(start + columns, Children.Count); at++)
            {
                rowHeight = Math.Max(rowHeight, Children[at].DesiredSize.Height);
            }

            for (int at = start; at < Math.Min(start + columns, Children.Count); at++)
            {
                Children[at].Arrange(new Rect((at - start) * (each + Gap), y, each, rowHeight));
            }

            y += rowHeight + Gap;
        }

        return finalSize;
    }
}
