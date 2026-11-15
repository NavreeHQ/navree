using System.Globalization;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using Navree.Controls.Drawing;
using Navree.Controls.Health;
using Navree.Design;
using Navree.Protocol;
using Windows.Foundation;

namespace Navree.Controls.Flags;

/// <summary>
/// A country's flag, drawn from shapes. Windows draws no flag for a pair of regional indicator letters (it shows the letters), so the
/// client draws the flags of the countries it knows and, for any other, a neutral flag with the country's letters on it. It is vector,
/// so it is sharp at every scale, and a screen reader is told the country's name.
/// </summary>
public sealed partial class FlagView : KindControl
{
    private readonly string country;
    private readonly double height;

    /// <summary>Draws a flag.</summary>
    /// <param name="country">The two-letter country code.</param>
    /// <param name="height">How high the flag stands, in effective pixels; it is a half as wide again.</param>
    public FlagView(string country, double height = 16)
    {
        this.country = country.ToUpperInvariant();
        this.height = height;
        Start();
    }

    /// <summary>The country's name in the person's language where Windows knows it, else its letters.</summary>
    /// <param name="country">The two-letter country code.</param>
    /// <returns>The name.</returns>
    public static string NameOf(string country)
    {
        try
        {
            return new RegionInfo(country.ToUpperInvariant()).DisplayName;
        }
        catch (ArgumentException)
        {
            return country.ToUpperInvariant();
        }
    }

    internal override UIElement Build(Palette palette)
    {
        Canvas canvas = new() { Width = FlagRules.Width, Height = FlagRules.Height, Clip = new RectangleGeometry { Rect = new Rect(0, 0, FlagRules.Width, FlagRules.Height) } };
        IReadOnlyList<FlagShape>? shapes = FlagRules.Of(country);
        if (shapes is null)
        {
            // No flag of its own: a neutral one with the country's letters on it, never a lie about another country's.
            canvas.Put(Ink.Box(FlagRules.Width, FlagRules.Height, Palette.Mark(palette.Of(Models.Hue.Neutral), 0.2)), 0, 0);
            canvas.Say(Ink.Words(country, 20, palette.Primary, strong: true), FlagRules.Width / 2, FlagRules.Height / 2, Across.Middle, Down.Middle);
        }
        else
        {
            foreach (FlagShape shape in shapes)
            {
                Add(canvas, shape);
            }
        }

        // A hairline round the edge, so a white band does not vanish on a white page.
        canvas.Put(Ink.Box(FlagRules.Width, FlagRules.Height, null, 0, Palette.Mark(palette.Of(Models.Hue.Neutral), 0.5), 2), 0, 0);
        Viewbox viewbox = new() { Child = canvas, Height = height, Width = height * 1.5, Stretch = Stretch.Uniform, VerticalAlignment = VerticalAlignment.Center };
        AutomationProperties.SetName(viewbox, NameOf(country));
        return viewbox;
    }

    private static void Add(Canvas canvas, FlagShape shape)
    {
        switch (shape)
        {
            case FlagRect rect:
                canvas.Put(Ink.Box(rect.Width, rect.Height, Ink.Solid(rect.Fill)), rect.X, rect.Y);
                break;
            case FlagDisc disc:
                canvas.Put(Ink.Disc(disc.Radius * 2, Ink.Solid(disc.Fill)), disc.X - disc.Radius, disc.Y - disc.Radius);
                break;
            case FlagRing ring:
                canvas.Put(Ink.Stroked(Ink.Arc(ring.X, ring.Y, ring.Radius, 0, 359.9), Ink.Solid(ring.Stroke), ring.Width), 0, 0);
                break;
            case FlagSpokes spokes:
                for (int at = 0; at < spokes.Count; at++)
                {
                    Pt end = Ink.Polar(spokes.X, spokes.Y, spokes.Radius, at * 360.0 / spokes.Count);
                    canvas.Put(Ink.Rule(spokes.X, spokes.Y, end.X, end.Y, Ink.Solid(spokes.Stroke), spokes.Width), 0, 0);
                }

                break;
            case FlagStar star:
                canvas.Put(Ink.Filled(Ink.Lines(FlagRules.StarPoints(star.X, star.Y, star.Radius), closed: true), Ink.Solid(star.Fill)), 0, 0);
                break;
            case FlagLine line:
                canvas.Put(Ink.Rule(line.X1, line.Y1, line.X2, line.Y2, Ink.Solid(line.Stroke), line.Width), 0, 0);
                break;
            default:
                break;
        }
    }
}

/// <summary>A text that may hold flags: the flags are drawn, the words round them are the page's.</summary>
public static class FlagInline
{
    /// <summary>Whether a text holds a flag.</summary>
    /// <param name="text">The text.</param>
    /// <returns>Whether it does.</returns>
    public static bool Holds(string text) => FlagText.Holds(text);

    /// <summary>The text as one element: its words and its flags side by side, each flag as high as the words are.</summary>
    /// <param name="text">The text.</param>
    /// <param name="words">Makes the element for a stretch of words.</param>
    /// <param name="flagHeight">How high a flag stands.</param>
    /// <returns>The element.</returns>
    public static UIElement Create(string text, Func<string, FrameworkElement> words, double flagHeight)
    {
        StackPanel line = new() { Orientation = Orientation.Horizontal, Spacing = 6 };
        foreach (FlagText.Piece piece in FlagText.Split(text))
        {
            if (piece.Country is not null)
            {
                line.Children.Add(new FlagView(piece.Country, flagHeight));
            }
            else
            {
                string trimmed = piece.Text.Trim();
                if (trimmed.Length > 0)
                {
                    FrameworkElement made = words(trimmed);
                    made.VerticalAlignment = VerticalAlignment.Center;
                    line.Children.Add(made);
                }
            }
        }

        return line;
    }
}
