using Microsoft.UI;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Shapes;
using Windows.Foundation;
using Windows.UI;

namespace Navree.Gallery;

/// <summary>
/// A row made of marked parts, so a picture shows where each part stands: a magenta bar at the start
/// edge of the frozen cell, a cyan bar at the end edge of the end cell, and coloured blocks with
/// yellow bars among the scrolling cells. The parts are the ones a table row has.
/// </summary>
public sealed class ProbeRow : Panel
{
    private double rowWidth;
    private double viewport = 960;
    private double height = 32;
    private double builtWidth = -1;
    private double builtStart = -1;
    private double builtEnd = -1;
    private bool builtHeader;
    private TextBlock? label;

    /// <summary>Makes a row with its four parts.</summary>
    public ProbeRow()
    {
        Children.Add(Chrome);
        Children.Add(ScrollCells);
        Children.Add(FrozenStart);
        Children.Add(FrozenEnd);
    }

    /// <summary>The background of the row, as wide as the viewport.</summary>
    internal Border Chrome { get; } = new();

    /// <summary>The scrolling cells.</summary>
    internal Canvas ScrollCells { get; } = new();

    /// <summary>The cell kept at the start edge.</summary>
    internal Canvas FrozenStart { get; } = new();

    /// <summary>The cell kept at the end edge.</summary>
    internal Canvas FrozenEnd { get; } = new();

    /// <summary>The row's index, or minus one for the header.</summary>
    internal int Index { get; private set; }

    /// <summary>Sets what the row shows.</summary>
    /// <param name="index">The row's index, or minus one for the header.</param>
    /// <param name="header">Whether it is the header.</param>
    /// <param name="width">The row's width.</param>
    /// <param name="viewportWidth">The viewport's width.</param>
    /// <param name="start">The width kept at the start.</param>
    /// <param name="end">The width kept at the end.</param>
    /// <param name="rowHeight">The row's height.</param>
    internal void Build(int index, bool header, double width, double viewportWidth, double start, double end, double rowHeight)
    {
        Index = index;
        viewport = viewportWidth;
        height = rowHeight;
        rowWidth = width;
        Chrome.Background = new SolidColorBrush(header
            ? Color.FromArgb(0x50, 0x60, 0x60, 0x60)
            : Color.FromArgb(index % 2 == 0 ? (byte)0x28 : (byte)0x10, 0x80, 0x80, 0x80));

        if (builtWidth != width || builtStart != start || builtEnd != end || builtHeader != header)
        {
            builtWidth = width;
            builtStart = start;
            builtEnd = end;
            builtHeader = header;
            Rebuild(header, width, start, end, rowHeight);
        }

        if (label is not null)
        {
            label.Text = header ? "FROZEN header" : "FROZEN r" + index;
        }

        InvalidateMeasure();
    }

    /// <inheritdoc />
    protected override Size MeasureOverride(Size availableSize)
    {
        Chrome.Measure(new Size(viewport, height));
        ScrollCells.Measure(new Size(rowWidth, height));
        FrozenStart.Measure(new Size(rowWidth, height));
        FrozenEnd.Measure(new Size(rowWidth, height));
        return new Size(rowWidth, height);
    }

    /// <inheritdoc />
    protected override Size ArrangeOverride(Size finalSize)
    {
        Chrome.Arrange(new Rect(0, 0, viewport, height));
        ScrollCells.Arrange(new Rect(0, 0, rowWidth, height));
        FrozenStart.Arrange(new Rect(0, 0, rowWidth, height));
        FrozenEnd.Arrange(new Rect(0, 0, rowWidth, height));
        return new Size(rowWidth, height);
    }

    private static SolidColorBrush Solid(byte r, byte g, byte b) => new(Color.FromArgb(0xFF, r, g, b));

    private static Border Zone(string tag, double left, double width, double rowHeight) => new()
    {
        Tag = tag,
        Width = width,
        Height = rowHeight,
        Background = new SolidColorBrush(Colors.Transparent),
    };

    private void Rebuild(bool header, double width, double start, double end, double rowHeight)
    {
        ScrollCells.Children.Clear();
        FrozenStart.Children.Clear();
        FrozenEnd.Children.Clear();

        // The scrolling cells: a block every hundred pixels from the start group's end to the end group's start.
        SolidColorBrush[] palette =
        [
            Solid(220, 50, 50), Solid(50, 170, 70), Solid(50, 90, 220), Solid(240, 150, 30), Solid(30, 170, 170), Solid(150, 60, 190),
        ];
        int block = 0;
        for (double x = start; x + 98 <= width - end; x += 100)
        {
            Rectangle rectangle = new() { Tag = "scroll", Width = 98, Height = rowHeight - 6, Fill = palette[block % palette.Length] };
            Canvas.SetLeft(rectangle, x);
            Canvas.SetTop(rectangle, 3);
            ScrollCells.Children.Add(rectangle);
            if (block % 5 == 0)
            {
                TextBlock text = new() { Text = "x" + (int)x, FontSize = 11, Foreground = Solid(255, 255, 255), IsHitTestVisible = false };
                Canvas.SetLeft(text, x + 4);
                Canvas.SetTop(text, 8);
                ScrollCells.Children.Add(text);
            }

            block++;
        }

        // Yellow bars the pictures use to see how far the scrolling cells moved.
        for (double x = start + 500; x + 6 <= width - end; x += 700)
        {
            Rectangle bar = new() { Tag = "scroll", Width = 6, Height = rowHeight, Fill = Solid(255, 255, 0) };
            Canvas.SetLeft(bar, x);
            ScrollCells.Children.Add(bar);
        }

        Border startZone = Zone("start", 0, start, rowHeight);
        FrozenStart.Children.Add(startZone);
        Rectangle magenta = new() { Tag = "start", Width = 6, Height = rowHeight, Fill = Solid(255, 0, 255) };
        FrozenStart.Children.Add(magenta);
        label = new TextBlock { FontSize = 12, IsHitTestVisible = false };
        Canvas.SetLeft(label, 12);
        Canvas.SetTop(label, 7);
        FrozenStart.Children.Add(label);

        Border endZone = Zone("end", width - end, end, rowHeight);
        Canvas.SetLeft(endZone, width - end);
        FrozenEnd.Children.Add(endZone);
        Rectangle cyan = new() { Tag = "end", Width = 6, Height = rowHeight, Fill = Solid(0, 255, 255) };
        Canvas.SetLeft(cyan, width - 6);
        FrozenEnd.Children.Add(cyan);
        TextBlock endText = new() { Text = header ? "END header" : "END", FontSize = 12, IsHitTestVisible = false };
        Canvas.SetLeft(endText, (width - end) + 12);
        Canvas.SetTop(endText, 7);
        FrozenEnd.Children.Add(endText);
    }
}

/// <summary>Makes the probe rows for a repeater and keeps the ones it gives back.</summary>
internal sealed partial class ProbeFactory : IElementFactory
{
    private readonly Stack<ProbeRow> pool = [];

    /// <inheritdoc />
    public UIElement GetElement(ElementFactoryGetArgs args) => pool.Count > 0 ? pool.Pop() : new ProbeRow();

    /// <inheritdoc />
    public void RecycleElement(ElementFactoryRecycleArgs args)
    {
        if (args.Element is ProbeRow row)
        {
            pool.Push(row);
        }
    }
}
