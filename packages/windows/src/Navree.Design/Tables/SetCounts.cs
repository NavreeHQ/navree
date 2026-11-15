namespace Navree.Design.Tables;

/// <summary>What assistive technology is told of a row's place in the whole set, which is more than the rows held when the rest are still to be asked for.</summary>
public static class SetCounts
{
    /// <summary>Said where the size or the place of a row in the whole set is not known.</summary>
    public const int Unknown = -1;

    /// <summary>How many rows the whole set has.</summary>
    /// <param name="held">How many rows the table holds.</param>
    /// <param name="total">How many the server says there are, or null when it does not say.</param>
    /// <param name="moreAfter">Whether more rows can be asked for after the last held.</param>
    /// <param name="moreBefore">Whether more rows can be asked for before the first held.</param>
    /// <returns>
    /// The server's count when it gave one that is at least the rows held; unknown when rows are not all held and no count was given;
    /// else the rows held.
    /// </returns>
    public static int SizeOfSet(int held, long? total, bool moreAfter, bool moreBefore)
    {
        if (total is { } known && known >= held)
        {
            return (int)Math.Min(known, int.MaxValue);
        }

        return moreAfter || moreBefore ? Unknown : held;
    }

    /// <summary>The place of a held row in the whole set, counting from one.</summary>
    /// <param name="index">The place among the rows held, from zero.</param>
    /// <param name="moreBefore">Whether more rows can be asked for before the first held: then how many come before is not known.</param>
    /// <returns>The place, or unknown.</returns>
    public static int PositionInSet(int index, bool moreBefore) => moreBefore ? Unknown : index + 1;
}

/// <summary>The kind of an announcement, which sets how assistive technology is told of it.</summary>
public enum AnnouncementKind
{
    /// <summary>Something was added to the table.</summary>
    Added,

    /// <summary>Something was taken out of the table.</summary>
    Removed,

    /// <summary>An action was completed.</summary>
    Completed,

    /// <summary>Anything else: a count, a state.</summary>
    Other,
}

/// <summary>What UI Automation is told when the chosen rows change.</summary>
public enum SelectionNotice
{
    /// <summary>Nothing changed.</summary>
    None,

    /// <summary>Each row that changed is told of.</summary>
    PerRow,

    /// <summary>The selection changed too much to tell row by row: one event says it is to be read again.</summary>
    Invalidated,
}

/// <summary>What happened to one row's choice.</summary>
public enum RowChoiceEvent
{
    /// <summary>The row is the only one chosen now.</summary>
    Selected,

    /// <summary>The row joined the chosen rows.</summary>
    Added,

    /// <summary>The row left the chosen rows.</summary>
    Removed,
}

/// <summary>Deciding how a change of the choice is told to UI Automation.</summary>
public static class SelectionNotices
{
    /// <summary>The most rows told of one by one.</summary>
    public const int MostPerRow = 16;

    /// <summary>How a change is told.</summary>
    /// <param name="changedRows">How many rows made are chosen or not now and were not or were before.</param>
    /// <param name="wholeSet">Whether every row was chosen or cleared, or the choice is every record that matches, or it was replaced by the host.</param>
    /// <returns>Nothing, row by row, or one event that the choice is to be read again.</returns>
    public static SelectionNotice For(int changedRows, bool wholeSet)
    {
        if (wholeSet)
        {
            return SelectionNotice.Invalidated;
        }

        if (changedRows <= 0)
        {
            return SelectionNotice.None;
        }

        return changedRows > MostPerRow ? SelectionNotice.Invalidated : SelectionNotice.PerRow;
    }

    /// <summary>What to tell of a row whose choice changed.</summary>
    /// <param name="nowChosen">Whether the row is chosen now.</param>
    /// <param name="chosenRows">How many rows are chosen now.</param>
    /// <returns>Selected when the row is the only one chosen, added when it joined others, removed when it left.</returns>
    public static RowChoiceEvent EventFor(bool nowChosen, int chosenRows) =>
        !nowChosen ? RowChoiceEvent.Removed : chosenRows == 1 ? RowChoiceEvent.Selected : RowChoiceEvent.Added;
}
