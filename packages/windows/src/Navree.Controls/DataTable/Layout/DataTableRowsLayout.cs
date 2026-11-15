using System.Collections.Specialized;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Navree.Design.Tables;
using Windows.Foundation;

namespace Navree.Controls;

/// <summary>
/// Places the rows of a table: every row has one height and a few that are open have more, so a
/// row's place is arithmetic and nothing is estimated. Only the rows in view, a margin of rows, and
/// the pinned row are made; the pinned row is the active one and is kept wherever it stands.
/// </summary>
internal sealed class DataTableRowsLayout : VirtualizingLayout
{
    /// <summary>The geometry of the rows; set it, then call <see cref="Reflow"/>.</summary>
    internal Geometry Geometry { get; set; } = new(40, []);

    /// <summary>The width of every row; zero takes the width offered.</summary>
    internal double RowWidth { get; set; }

    /// <summary>The index of the row to keep made wherever it stands, or minus one.</summary>
    internal int PinnedIndex { get; set; } = -1;

    /// <summary>Tells a row that it is the one to measure with room for its detail and how much room it has had.</summary>
    internal event Action<int, double>? DetailMeasured;

    /// <summary>The rows made in the last pass, with their indices, for those who read where they stand.</summary>
    internal IReadOnlyList<(int Index, UIElement Element)> LastRealized => realized.Select(r => (r.Index, r.Element)).ToList();

    private readonly List<(int Index, UIElement Element, double Height)> realized = [];

    /// <summary>Asks for the rows to be placed again.</summary>
    internal void Reflow() => InvalidateMeasure();

    /// <inheritdoc />
    protected override Size MeasureOverride(VirtualizingLayoutContext context, Size availableSize)
    {
        return MeasureRows(context, availableSize);
    }

    private Size MeasureRows(VirtualizingLayoutContext context, Size availableSize)
    {
        realized.Clear();
        int count = context.ItemCount;
        double width = RowWidth > 0 ? RowWidth : (double.IsInfinity(availableSize.Width) ? 0 : availableSize.Width);
        if (count == 0)
        {
            return new Size(width, 0);
        }

        Geometry geometry = Geometry;
        Rect rect = context.RealizationRect;
        (int first, int last) = geometry.RealizedRange(rect.Y, rect.Height, count);
        for (int index = first; index <= last; index++)
        {
            Realize(context, geometry, index, width);
        }

        int pinned = PinnedIndex;
        if (pinned >= 0 && pinned < count && (pinned < first || pinned > last))
        {
            Realize(context, geometry, pinned, width);
        }

        return new Size(width, geometry.Extent(count));
    }

    /// <inheritdoc />
    protected override Size ArrangeOverride(VirtualizingLayoutContext context, Size finalSize)
    {
        Geometry geometry = Geometry;
        double width = RowWidth > 0 ? RowWidth : finalSize.Width;
        foreach ((int index, UIElement element, double height) in realized)
        {
            element.Arrange(new Rect(0, geometry.RowTop(index), width, height));
        }

        return finalSize;
    }

    /// <inheritdoc />
    protected override void OnItemsChangedCore(VirtualizingLayoutContext context, object source, NotifyCollectionChangedEventArgs args) =>
        InvalidateMeasure();

    private void Realize(VirtualizingLayoutContext context, Geometry geometry, int index, double width)
    {
        UIElement element = context.GetOrCreateElementAt(index);
        double closed = geometry.RowHeight;
        bool open = geometry.HeightAt(index) > closed;
        element.Measure(new Size(width, open ? double.PositiveInfinity : closed));
        double height = open ? Math.Max(element.DesiredSize.Height, closed) : closed;
        if (open && Math.Abs((height - closed) - (geometry.HeightAt(index) - closed)) > 0.5)
        {
            DetailMeasured?.Invoke(index, height - closed);
        }

        realized.Add((index, element, height));
    }
}
