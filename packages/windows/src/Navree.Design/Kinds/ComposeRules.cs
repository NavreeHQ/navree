using Navree.Protocol;

namespace Navree.Design;

/// <summary>How a screen written for native clients is set out: the rules of the composition kinds, as plain functions.</summary>
public static class ComposeRules
{
    /// <summary>A room at least this wide is wide: what a large window gives a screen's content. The same line the other clients draw.</summary>
    public const double WideRoom = 600;

    /// <summary>How many parts of a width fit in a place this wide, the gaps between them counted: at least one.</summary>
    /// <param name="width">The place's width.</param>
    /// <param name="part">One part's width.</param>
    /// <param name="gap">The gap between parts.</param>
    /// <returns>How many fit.</returns>
    public static int Fit(double width, double part, double gap)
    {
        if (part + gap <= 0)
        {
            return int.MaxValue;
        }

        double count = Math.Floor((width + gap) / (part + gap));
        return !double.IsFinite(count) || count >= int.MaxValue ? int.MaxValue : Math.Max(1, (int)count);
    }

    /// <summary>The least width of a grid column when the page gives neither a count nor a least width.</summary>
    public const double GridLeast = 160;

    /// <summary>
    /// How many columns a grid has in a place this wide, at most: the count is the most, the least width the least a column may be; a grid that
    /// gives only a count draws that many. Never more columns than parts, never fewer than one.
    /// </summary>
    /// <param name="width">The place width.</param>
    /// <param name="cols">The count the page gives, or none.</param>
    /// <param name="least">The least width the page gives, or none.</param>
    /// <param name="gap">The gap between columns.</param>
    /// <param name="parts">How many parts there are.</param>
    /// <returns>The number of columns.</returns>
    public static int Columns(double width, int? cols, int? least, double gap, int parts)
    {
        int count;
        if (least is int column)
        {
            int fits = Fit(width, column, gap);
            count = cols is int most ? Math.Min(most, fits) : fits;
        }
        else if (cols is int fixedCount && fixedCount > 0)
        {
            count = fixedCount;
        }
        else
        {
            count = Fit(width, GridLeast, gap);
        }

        return Math.Max(1, Math.Min(count, Math.Max(parts, 1)));
    }

    /// <summary>
    /// The space between a grid's parts: its own steps, or 12 when it gives none; parts that are not cards (figures with a ring at their
    /// end) stand further apart, so the ring at the end of one is not taken for the start of the next.
    /// </summary>
    /// <param name="steps">The page's steps of spacing, or none (0).</param>
    /// <param name="cards">Whether every part is a card.</param>
    /// <returns>The gap.</returns>
    public static double GridGap(int steps, bool cards)
    {
        double gap = steps == 0 ? 12 : steps * 4;
        return cards ? gap : Math.Max(gap, 24);
    }

    /// <summary>The width of each column of a run of <paramref name="count"/> columns in a place this wide.</summary>
    /// <param name="width">The place's width.</param>
    /// <param name="count">How many columns.</param>
    /// <param name="gap">The gap between columns.</param>
    /// <returns>One column's width.</returns>
    public static double ColumnWidth(double width, int count, double gap)
    {
        int n = Math.Max(count, 1);
        return Math.Max((width - (gap * (n - 1))) / n, 0);
    }

    /// <summary>
    /// How a carousel lays its parts: how many show at once and how wide each is. With a least width, as many as fit at it, each the same
    /// width; when more are waiting, the next one shows only its edge, so the parts shown take the room less that edge. With no least
    /// width, each part has its own width (none).
    /// </summary>
    /// <param name="room">The room the carousel has.</param>
    /// <param name="least">The least width of a part, or none.</param>
    /// <param name="gap">The gap between parts.</param>
    /// <param name="parts">How many parts there are.</param>
    /// <param name="peek">How much of the next part shows when more are waiting.</param>
    /// <returns>How many show and how wide each is.</returns>
    public static (int Shown, double? Width) Carousel(double room, int? least, double gap, int parts, double peek = 24)
    {
        if (least is not int column)
        {
            return (Math.Max(parts, 1), null);
        }

        int fits = Fit(room, column, gap);
        if (parts <= fits)
        {
            int all = Math.Max(parts, 1);
            return (all, ColumnWidth(room, all, gap));
        }

        double available = room - gap - peek;
        int shown = Fit(available, column, gap);
        return (shown, ColumnWidth(available, shown, gap));
    }

    /// <summary>Whether a carousel scrolls: only when its parts do not all show.</summary>
    /// <param name="shown">How many show.</param>
    /// <param name="parts">How many there are.</param>
    /// <returns>Whether it scrolls.</returns>
    public static bool Scrolls(int shown, int parts) => parts > shown;

    /// <summary>How a part comes in when an answer adds it to a screen already drawn.</summary>
    public enum Arrival
    {
        /// <summary>It is simply there.</summary>
        None,

        /// <summary>It fades in.</summary>
        Fades,

        /// <summary>It rises into place.</summary>
        Rises,

        /// <summary>It grows into place.</summary>
        Grows,
    }

    /// <summary>How a card or a stack comes in: as it says, or only fading for a person who asked for less motion.</summary>
    /// <param name="appear">What the page says, or none.</param>
    /// <param name="reduceMotion">Whether the person asked for less motion.</param>
    /// <returns>The arrival.</returns>
    public static Arrival ArrivalOf(Models.Appear? appear, bool reduceMotion) =>
        appear switch
        {
            Models.Appear.Fade => Arrival.Fades,
            Models.Appear.Rise => reduceMotion ? Arrival.Fades : Arrival.Rises,
            Models.Appear.Scale => reduceMotion ? Arrival.Fades : Arrival.Grows,
            _ => Arrival.None,
        };

    /// <summary>The corner of a shape that is not a circle or a rectangle, for a side this long.</summary>
    /// <param name="form">The form.</param>
    /// <param name="width">The shape's width.</param>
    /// <param name="height">The shape's height.</param>
    /// <returns>The corner radius.</returns>
    public static double Corner(Models.ShapeForm form, double width, double height)
    {
        double shortest = Math.Max(Math.Min(width, height), 0);
        return form switch
        {
            Models.ShapeForm.Rect => 0,
            Models.ShapeForm.Capsule or Models.ShapeForm.Circle => shortest / 2,
            _ => Math.Min(8, shortest / 4),
        };
    }

    /// <summary>The share of a hue a tinted card's ground takes over the card's own, and of its edge.</summary>
    /// <param name="dark">Whether the ground is dark.</param>
    /// <returns>The opacity of the ground and the opacity of the edge.</returns>
    public static (double Ground, double Edge) Tint(bool dark) => dark ? (0.18, 0.4) : (0.1, 0.32);

    /// <summary>The longest word kept whole on its line.</summary>
    public const int WholeWord = 16;

    /// <summary>Whether a text is one short word or figure (<c>1,994</c>, <c>Movement</c>) that is never cut in two to fit a narrow place.</summary>
    /// <param name="text">The text.</param>
    /// <returns>Whether it is kept whole.</returns>
    public static bool KeepsWhole(string text)
    {
        string trimmed = text.Trim();
        return trimmed.Length > 0 && trimmed.Length <= WholeWord && !trimmed.Any(char.IsWhiteSpace);
    }

    /// <summary>The least side of a target a person points at: 40 effective pixels, as the system's own controls give.</summary>
    public const double TouchLeast = 40;

    /// <summary>What a target reaches past each side of a symbol this wide so the whole is at least <see cref="TouchLeast"/>.</summary>
    /// <param name="glyph">The symbol's width.</param>
    /// <returns>The padding on each side.</returns>
    public static double Outset(double glyph) => Math.Max(0, Math.Ceiling((TouchLeast - glyph) / 2));
}
