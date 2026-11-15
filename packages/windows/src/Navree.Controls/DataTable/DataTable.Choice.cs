using Navree.Design.Tables;

namespace Navree.Controls;

public sealed partial class DataTable
{
    private ChoiceState choice = new(new HashSet<string>(StringComparer.Ordinal), false);
    private string? anchorKey;

    /// <summary>Whether a row is chosen.</summary>
    /// <param name="key">The row's key.</param>
    /// <param name="choosable">Whether the row can be chosen.</param>
    internal bool IsChosen(string key, Choosable choosable) =>
        choosable == Choosable.Yes && (choice.Across || choice.Keys.Contains(key));

    // How many held rows can be chosen, and how many of those are.
    private (int Held, int Chosen) CountChoice()
    {
        if (Model is not { } model)
        {
            return (0, 0);
        }

        int held = 0;
        int chosen = 0;
        for (int i = 0; i < model.RowCount; i++)
        {
            DataTableRecord record = model.GetRow(i);
            if (record.Choosable != Choosable.Yes || record.Key is null)
            {
                continue;
            }

            held++;
            if (choice.Across || choice.Keys.Contains(record.Key))
            {
                chosen++;
            }
        }

        return (held, chosen);
    }

    private int ChoosableHeld() => CountChoice().Held;

    private List<string> ChoosableHeldKeys()
    {
        List<string> keys = [];
        if (Model is { } model)
        {
            for (int i = 0; i < model.RowCount; i++)
            {
                DataTableRecord record = model.GetRow(i);
                if (record.Choosable == Choosable.Yes && record.Key is { } key)
                {
                    keys.Add(key);
                }
            }
        }

        return keys;
    }

    /// <summary>Chooses or clears one row, as a person did.</summary>
    /// <param name="index">The row's place.</param>
    internal void ToggleChoice(int index)
    {
        if (Model is not { } model || index < 0 || index >= model.RowCount)
        {
            return;
        }

        DataTableRecord record = model.GetRow(index);
        if (record.Choosable != Choosable.Yes || record.Key is not { } key)
        {
            return;
        }

        bool on = !IsChosen(key, record.Choosable);
        if (on)
        {
            HashSet<string> keys = new(choice.Keys, StringComparer.Ordinal) { key };
            choice = new ChoiceState(keys, false);
        }
        else if (choice.Across)
        {
            choice = Choosing.AfterClear(choice, key, ChoosableHeldKeys());
        }
        else
        {
            HashSet<string> keys = new(choice.Keys, StringComparer.Ordinal);
            keys.Remove(key);
            choice = new ChoiceState(keys, false);
        }

        anchorKey = key;
        ChoiceChanged(announce: true);
        RaiseChoiceRequested([key], on, false, false);
    }

    /// <summary>Chooses the rows from the anchor to a row, as the anchor row stands now.</summary>
    /// <param name="target">The place of the row the range ends at.</param>
    internal void ChooseRangeTo(int target)
    {
        if (Model is not { } model || target < 0 || target >= model.RowCount)
        {
            return;
        }

        int anchor = anchorKey is null ? -1 : model.IndexOfKey(anchorKey);
        if (anchor < 0)
        {
            ToggleChoice(target);
            return;
        }

        // The range takes the state the anchor row has now.
        DataTableRecord anchorRow = model.GetRow(anchor);
        bool on = anchorRow.Key is { } anchored && IsChosen(anchored, anchorRow.Choosable);

        List<Choosable> all = new(model.RowCount);
        for (int i = 0; i < model.RowCount; i++)
        {
            DataTableRecord record = model.GetRow(i);
            all.Add(record.Key is null ? Choosable.None : record.Choosable);
        }

        IReadOnlyList<int> range = Choosing.Range(all, anchor, target);
        if (range.Count == 0)
        {
            return;
        }

        HashSet<string> keys = choice.Across ? new HashSet<string>(ChoosableHeldKeys(), StringComparer.Ordinal) : new HashSet<string>(choice.Keys, StringComparer.Ordinal);
        List<string> changed = [];
        foreach (int i in range)
        {
            string key = model.GetRow(i).Key!;
            if (on ? keys.Add(key) : keys.Remove(key))
            {
                changed.Add(key);
            }
        }

        choice = new ChoiceState(keys, false);
        ChoiceChanged(announce: true);
        RaiseChoiceRequested(changed, on, false, false);
    }

    /// <summary>Chooses every row held that can be chosen.</summary>
    internal void ChooseAllHeld()
    {
        choice = new ChoiceState(new HashSet<string>(ChoosableHeldKeys(), StringComparer.Ordinal), false);
        ChoiceChanged(announce: true);
        RaiseChoiceRequested([], true, true, false);
    }

    /// <summary>Clears the choice.</summary>
    internal void ClearChoice()
    {
        if (ChosenCount == 0 && !choice.Across)
        {
            return;
        }

        choice = new ChoiceState(new HashSet<string>(StringComparer.Ordinal), false);
        anchorKey = null;
        ChoiceChanged(announce: true);
        RaiseChoiceRequested([], false, true, false);
    }

    /// <summary>Chooses every record that matches, held or not.</summary>
    internal void ChooseAcross()
    {
        choice = new ChoiceState(new HashSet<string>(StringComparer.Ordinal), true);
        ChoiceChanged(announce: true);
        RaiseChoiceRequested([], true, false, true);
    }

    /// <summary>The box in the header was pressed.</summary>
    internal void HeaderBoxPressed()
    {
        if (HeaderBox() == HeaderBoxState.On)
        {
            ClearChoice();
        }
        else
        {
            ChooseAllHeld();
        }
    }

    // Rows left the held rows: the choice forgets any that are gone.
    private void RemovedFromChoice()
    {
        if (choice.Across || choice.Keys.Count == 0 || Model is not { } model)
        {
            UpdateBulkBar();
            return;
        }

        HashSet<string> keys = new(choice.Keys.Where(k => model.IndexOfKey(k) >= 0), StringComparer.Ordinal);
        if (keys.Count != choice.Keys.Count)
        {
            choice = new ChoiceState(keys, false);
            ChoiceChanged(announce: false);
        }
    }

    private void ChoiceChanged(bool announce)
    {
        foreach (DataTableRow row in liveRows)
        {
            row.RefreshChoice();
        }

        if (header is not null && header.HasParts)
        {
            header.Refresh();
        }

        UpdateBulkBar();
        if (announce)
        {
            AnnounceChoice();
        }
    }
}
