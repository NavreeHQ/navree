using Microsoft.UI;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Shapes;
using Navree.Design;
using Windows.Foundation;
using ShapePath = Microsoft.UI.Xaml.Shapes.Path;

namespace Navree.Controls.Drawing;

/// <summary>Where a text stands against the point it is put at, across.</summary>
internal enum Across
{
    /// <summary>The text starts at the point.</summary>
    Start,

    /// <summary>The text is centred on the point.</summary>
    Middle,

    /// <summary>The text ends at the point.</summary>
    End,
}

/// <summary>Where a text stands against the point it is put at, down.</summary>
internal enum Down
{
    /// <summary>The text hangs from the point.</summary>
    Top,

    /// <summary>The text is centred on the point.</summary>
    Middle,

    /// <summary>The text stands on the point.</summary>
    Bottom,
}

/// <summary>The small drawing tools every drawn kind uses: shapes and their geometry, and texts put at a point. Everything is vector, so it is sharp at every scale.</summary>
internal static class Ink
{
    /// <summary>A colour of ours as the system's colour type.</summary>
    internal static Windows.UI.Color Colour(Rgb colour, double opacity = 1) =>
        ColorHelper.FromArgb((byte)Math.Clamp((int)Math.Round(opacity * 255), 0, 255), colour.R, colour.G, colour.B);

    /// <summary>A plain brush.</summary>
    internal static SolidColorBrush Solid(Rgb colour, double opacity = 1) => new(Colour(colour, opacity));

    /// <summary>Puts an element at a place on a canvas.</summary>
    internal static T Put<T>(this Canvas canvas, T element, double x, double y)
        where T : UIElement
    {
        Canvas.SetLeft(element, x);
        Canvas.SetTop(element, y);
        canvas.Children.Add(element);
        return element;
    }

    /// <summary>A rectangle, with rounded corners when asked.</summary>
    internal static Rectangle Box(double width, double height, Brush? fill, double radius = 0, Brush? stroke = null, double strokeWidth = 0)
    {
        Rectangle box = new()
        {
            Width = Math.Max(width, 0),
            Height = Math.Max(height, 0),
            Fill = fill,
            RadiusX = radius,
            RadiusY = radius,
            IsHitTestVisible = false,
        };
        if (stroke is not null)
        {
            box.Stroke = stroke;
            box.StrokeThickness = strokeWidth;
        }

        return box;
    }

    /// <summary>A disc of a width.</summary>
    internal static Ellipse Disc(double width, Brush? fill, Brush? stroke = null, double strokeWidth = 0)
    {
        Ellipse disc = new() { Width = width, Height = width, Fill = fill, IsHitTestVisible = false };
        if (stroke is not null)
        {
            disc.Stroke = stroke;
            disc.StrokeThickness = strokeWidth;
        }

        return disc;
    }

    /// <summary>A line drawn along a geometry.</summary>
    internal static ShapePath Stroked(Geometry geometry, Brush stroke, double width, PenLineCap cap = PenLineCap.Flat, double[]? dash = null)
    {
        ShapePath path = new()
        {
            Data = geometry,
            Stroke = stroke,
            StrokeThickness = width,
            StrokeStartLineCap = cap,
            StrokeEndLineCap = cap,
            StrokeLineJoin = PenLineJoin.Round,
            IsHitTestVisible = false,
        };
        if (dash is not null)
        {
            DoubleCollection pattern = new();
            foreach (double part in dash)
            {
                pattern.Add(part);
            }

            path.StrokeDashArray = pattern;
        }

        return path;
    }

    /// <summary>A filled geometry.</summary>
    internal static ShapePath Filled(Geometry geometry, Brush fill) =>
        new() { Data = geometry, Fill = fill, IsHitTestVisible = false };

    /// <summary>A straight line from one point to another.</summary>
    internal static ShapePath Rule(double x1, double y1, double x2, double y2, Brush stroke, double width = 1, double[]? dash = null) =>
        Stroked(Lines([new Pt(x1, y1), new Pt(x2, y2)]), stroke, width, PenLineCap.Flat, dash);

    /// <summary>A geometry of straight lines through points; closed when asked, or closed down to a level to fill under the line.</summary>
    internal static PathGeometry Lines(IReadOnlyList<Pt> points, bool closed = false, double? fillTo = null)
    {
        PathFigure figure = new() { IsClosed = closed || fillTo.HasValue, IsFilled = closed || fillTo.HasValue };
        if (fillTo is double level && points.Count > 0)
        {
            figure.StartPoint = new Point(points[0].X, level);
            figure.Segments.Add(new LineSegment { Point = new Point(points[0].X, points[0].Y) });
            for (int at = 1; at < points.Count; at++)
            {
                figure.Segments.Add(new LineSegment { Point = new Point(points[at].X, points[at].Y) });
            }

            figure.Segments.Add(new LineSegment { Point = new Point(points[^1].X, level) });
        }
        else
        {
            for (int at = 0; at < points.Count; at++)
            {
                Point point = new(points[at].X, points[at].Y);
                if (at == 0)
                {
                    figure.StartPoint = point;
                }
                else
                {
                    figure.Segments.Add(new LineSegment { Point = point });
                }
            }
        }

        PathGeometry geometry = new();
        geometry.Figures.Add(figure);
        return geometry;
    }

    /// <summary>A smooth geometry through points that never goes past them; closed down to a level to fill under the line when asked.</summary>
    internal static PathGeometry Smooth(IReadOnlyList<Pt> points, double? fillTo = null)
    {
        PathFigure figure = new() { IsClosed = fillTo.HasValue, IsFilled = fillTo.HasValue };
        if (points.Count > 0)
        {
            Point first = new(points[0].X, points[0].Y);
            if (fillTo is double level)
            {
                figure.StartPoint = new Point(first.X, level);
                figure.Segments.Add(new LineSegment { Point = first });
            }
            else
            {
                figure.StartPoint = first;
            }
        }

        foreach (Bezier piece in Curves.Monotone(points))
        {
            figure.Segments.Add(new BezierSegment
            {
                Point1 = new Point(piece.First.X, piece.First.Y),
                Point2 = new Point(piece.Second.X, piece.Second.Y),
                Point3 = new Point(piece.To.X, piece.To.Y),
            });
        }

        if (fillTo is double bottom && points.Count > 0)
        {
            figure.Segments.Add(new LineSegment { Point = new Point(points[^1].X, bottom) });
        }

        PathGeometry geometry = new();
        geometry.Figures.Add(figure);
        return geometry;
    }

    /// <summary>An arc round a middle, clockwise from an angle (degrees from three o'clock), as far as a sweep (degrees).</summary>
    internal static PathGeometry Arc(double cx, double cy, double radius, double startDegrees, double sweepDegrees)
    {
        double sweep = Math.Clamp(sweepDegrees, 0, 359.99);
        Point At(double degrees)
        {
            double angle = degrees * Math.PI / 180;
            return new Point(cx + (radius * Math.Cos(angle)), cy + (radius * Math.Sin(angle)));
        }

        PathFigure figure = new() { IsClosed = false, IsFilled = false, StartPoint = At(startDegrees) };
        double done = 0;
        while (done < sweep - 1e-6)
        {
            double chunk = Math.Min(90, sweep - done);
            done += chunk;
            figure.Segments.Add(new ArcSegment
            {
                Point = At(startDegrees + done),
                Size = new Size(radius, radius),
                RotationAngle = 0,
                IsLargeArc = false,
                SweepDirection = SweepDirection.Clockwise,
            });
        }

        PathGeometry geometry = new();
        geometry.Figures.Add(figure);
        return geometry;
    }

    /// <summary>The point at an angle and a distance from a middle.</summary>
    internal static Pt Polar(double cx, double cy, double radius, double degrees)
    {
        double angle = degrees * Math.PI / 180;
        return new Pt(cx + (radius * Math.Cos(angle)), cy + (radius * Math.Sin(angle)));
    }


    /// <summary>The colour a fraction of the way from one colour to another.</summary>
    internal static Windows.UI.Color Blend(Windows.UI.Color one, Windows.UI.Color other, double fraction)
    {
        double t = Math.Clamp(fraction, 0, 1);
        return Windows.UI.Color.FromArgb(
            (byte)Math.Round(one.A + ((other.A - one.A) * t)),
            (byte)Math.Round(one.R + ((other.R - one.R) * t)),
            (byte)Math.Round(one.G + ((other.G - one.G) * t)),
            (byte)Math.Round(one.B + ((other.B - one.B) * t)));
    }

    /// <summary>
    /// An arc from twelve o'clock clockwise, as far as a sweep, whose colour goes from one colour to another along its length (the box has
    /// no angular gradient, so it is laid in short pieces), with round ends.
    /// </summary>
    internal static void BlendedArc(Canvas canvas, double cx, double cy, double radius, double stroke, double startDegrees, double sweepDegrees, Windows.UI.Color begin, Windows.UI.Color end)
    {
        if (sweepDegrees <= 0)
        {
            return;
        }

        if (begin == end)
        {
            canvas.Put(Stroked(Arc(cx, cy, radius, startDegrees, sweepDegrees), new SolidColorBrush(begin), stroke, PenLineCap.Round), 0, 0);
            return;
        }

        int pieces = Math.Clamp((int)Math.Ceiling(sweepDegrees / 5), 1, 72);
        double each = sweepDegrees / pieces;
        for (int at = 0; at < pieces; at++)
        {
            Windows.UI.Color colour = Blend(begin, end, (at + 0.5) / pieces);
            double overlap = at < pieces - 1 ? 0.6 : 0;
            canvas.Put(Stroked(Arc(cx, cy, radius, startDegrees + (at * each), each + overlap), new SolidColorBrush(colour), stroke, PenLineCap.Flat), 0, 0);
        }

        Pt first = Polar(cx, cy, radius, startDegrees);
        Pt last = Polar(cx, cy, radius, startDegrees + sweepDegrees);
        canvas.Put(Disc(stroke, new SolidColorBrush(begin)), first.X - (stroke / 2), first.Y - (stroke / 2));
        canvas.Put(Disc(stroke, new SolidColorBrush(end)), last.X - (stroke / 2), last.Y - (stroke / 2));
    }

    /// <summary>
    /// Parts of a whole laid end to end in a bar with rounded ends and a hair between them: along the bar (across, or up from the foot).
    /// Each part's share of the length is given; a part with no share is not drawn.
    /// </summary>
    internal static void StackedBar(Canvas canvas, bool across, double x, double y, double width, double height, IReadOnlyList<(double Share, Brush Fill)> parts, double hair, double radius)
    {
        List<(double Share, Brush Fill)> drawn = [.. parts.Where(part => part.Share > 0)];
        if (drawn.Count == 0)
        {
            return;
        }

        double length = across ? width : height;
        double room = Math.Max(length - (hair * (drawn.Count - 1)), 1);
        double at = 0;
        for (int index = 0; index < drawn.Count; index++)
        {
            double size = Math.Max(room * drawn[index].Share, 1);
            bool first = index == 0;
            bool last = index == drawn.Count - 1;
            double px = across ? x + at : x;
            double py = across ? y : (y + height) - at - size;
            double w = across ? size : width;
            double h = across ? height : size;
            double corner = Math.Min(radius, Math.Min(w, h) / 2);
            if (first && last)
            {
                canvas.Put(Box(w, h, drawn[index].Fill, corner), px, py);
            }
            else if (first || last)
            {
                canvas.Put(Box(w, h, drawn[index].Fill, corner), px, py);

                // Square off the end that faces the next part.
                double cover = Math.Min(Math.Max((across ? w : h) - corner, 0), Math.Max(across ? w : h, 0));
                bool atStart = across ? last : first;
                if (across)
                {
                    canvas.Put(Box(cover, h, drawn[index].Fill), atStart ? px : px + (w - cover), py);
                }
                else
                {
                    // Up from the foot: the first part is at the bottom.
                    canvas.Put(Box(w, cover, drawn[index].Fill), px, first ? py : py + (h - cover));
                }
            }
            else
            {
                canvas.Put(Box(w, h, drawn[index].Fill), px, py);
            }

            at += size + hair;
        }
    }

    /// <summary>A text, in a size and a brush, that stays on one line.</summary>
    internal static TextBlock Words(string text, double size, Brush brush, bool strong = false)
    {
        TextBlock block = new()
        {
            Text = text,
            FontSize = size,
            Foreground = brush,
            TextWrapping = TextWrapping.NoWrap,
            IsHitTestVisible = false,
            TextReadingOrder = TextReadingOrder.DetectFromContent,
        };
        if (strong)
        {
            block.FontWeight = Microsoft.UI.Text.FontWeights.SemiBold;
        }

        return block;
    }

    /// <summary>How big a text comes out.</summary>
    internal static Size Measured(TextBlock block)
    {
        block.Measure(new Size(double.PositiveInfinity, double.PositiveInfinity));
        return block.DesiredSize;
    }

    /// <summary>Puts a text on a canvas against a point.</summary>
    internal static TextBlock Say(this Canvas canvas, TextBlock block, double x, double y, Across across = Across.Start, Down down = Down.Top)
    {
        Size size = Measured(block);
        double left = across switch
        {
            Across.Middle => x - (size.Width / 2),
            Across.End => x - size.Width,
            _ => x,
        };
        double top = down switch
        {
            Down.Middle => y - (size.Height / 2),
            Down.Bottom => y - size.Height,
            _ => y,
        };
        return canvas.Put(block, left, top);
    }
}
