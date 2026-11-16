namespace Navree.Design;

/// <summary>The shape of the mark that stands for a series at its places on a line and in its key.</summary>
public enum MarkShape
{
    /// <summary>A filled disc.</summary>
    Circle,

    /// <summary>A filled square.</summary>
    Square,

    /// <summary>A filled diamond.</summary>
    Diamond,

    /// <summary>A filled triangle with its point up.</summary>
    TriangleUp,

    /// <summary>A filled triangle with its point down.</summary>
    TriangleDown,

    /// <summary>A plus sign.</summary>
    Plus,

    /// <summary>A cross.</summary>
    Cross,

    /// <summary>A ring: a disc with nothing in it.</summary>
    Ring,
}

/// <summary>
/// How a series is told apart from the others in a contrast theme. A contrast theme has two colours a mark can stand in (the text colour and
/// the highlight) and a person who has chosen it may see no difference between hues at all, so a series differs in more than colour: whether
/// it is a solid or an outline, how its line is dashed, and the shape of its mark.
/// </summary>
/// <param name="Highlight">Whether the series is in the highlight colour (else the text colour).</param>
/// <param name="Filled">Whether its bars and slices are solid (else an outline with the ground inside).</param>
/// <param name="Dash">The dashes of its line, as lengths of a dash and a gap in turn; none for a solid line.</param>
/// <param name="Shape">The shape of its mark.</param>
public readonly record struct SeriesLook(bool Highlight, bool Filled, IReadOnlyList<double> Dash, MarkShape Shape);

/// <summary>The looks a contrast theme gives the series of a chart, in order.</summary>
public static class SeriesLooks
{
    private static readonly IReadOnlyList<double>[] Dashes = [[], [6, 3], [2, 3], [6, 3, 2, 3]];

    /// <summary>How many series have a look of their own before the looks go round again.</summary>
    public const int Distinct = 8;

    /// <summary>The look of the series at a place; two series next to each other differ in at least two ways, and the first eight never share a mark.</summary>
    /// <param name="index">The series' place, from 0.</param>
    /// <returns>The look.</returns>
    public static SeriesLook Of(int index)
    {
        int place = ((index % Distinct) + Distinct) % Distinct;
        return new SeriesLook(
            Highlight: place % 2 == 1,
            Filled: place % 4 < 2,
            Dash: Dashes[(place / 2) % Dashes.Length],
            Shape: (MarkShape)place);
    }

    /// <summary>The points of a mark of a shape, in a box of a side, round the origin (the middle of the mark); a disc, a square and a ring have none and are drawn as what they are.</summary>
    /// <param name="shape">The shape.</param>
    /// <param name="side">The box's side.</param>
    /// <returns>The corners, in order, or none for a shape that is not drawn with corners.</returns>
    public static IReadOnlyList<Pt> Corners(MarkShape shape, double side)
    {
        double half = Guard.Length(side) / 2;
        return shape switch
        {
            MarkShape.Diamond => [new Pt(0, -half), new Pt(half, 0), new Pt(0, half), new Pt(-half, 0)],
            MarkShape.TriangleUp => [new Pt(0, -half), new Pt(half, half), new Pt(-half, half)],
            MarkShape.TriangleDown => [new Pt(-half, -half), new Pt(half, -half), new Pt(0, half)],
            _ => [],
        };
    }
}
