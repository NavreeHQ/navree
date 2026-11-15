using System.Globalization;
using Microsoft.UI.Dispatching;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;
using Navree.Design.Tables;
using VirtualKey = Windows.System.VirtualKey;

namespace Navree.Controls;

public sealed partial class DataTable
{
    private int activeRow = -1;
    private int activeColumn;
    private bool activeSet;
    private bool lastInputWasKeyboard;
    private Control? activeElement;
    private string typed = string.Empty;
    private DateTime typedAt = DateTime.MinValue;
    private List<string>? rowNames;

    /// <summary>Whether a cell of a body row is the active one.</summary>
    /// <param name="row">The row's place.</param>
    /// <param name="gridColumn">The cell's column among the columns shown.</param>
    internal bool IsActiveCell(int row, int gridColumn) => activeSet && row == activeRow && gridColumn == activeColumn;

    /// <summary>Whether a heading cell is the active one.</summary>
    /// <param name="gridColumn">The cell's column among the columns shown.</param>
    internal bool IsActiveHeader(int gridColumn) => activeSet && activeRow < 0 && gridColumn == activeColumn;

    /// <summary>A cell took focus: it becomes the active one.</summary>
    internal void CellGotFocus(DataTableCell cell, RoutedEventArgs e)
    {
        if (cell.Role == CellRole.Data && cell.Row is { Index: >= 0 } row)
        {
            activeSet = true;
            activeRow = row.Index;
            activeColumn = Math.Max(cell.GridColumn, 0);
            rowsLayout.PinnedIndex = activeRow;
            MarkActive(cell);
        }
    }

    /// <summary>The pointer was pressed on a cell.</summary>
    internal void CellPressed(DataTableCell cell, PointerRoutedEventArgs e)
    {
        lastInputWasKeyboard = false;
        if (cell.Role == CellRole.Data && cell.Row is { Index: >= 0 } row)
        {
            SetActive(new Cell(row.Index, Math.Max(cell.GridColumn, 0)), focus: true);
        }
    }

    // Makes the first cell the active one when none has been.
    private void NormalizeActive()
    {
        if (activeSet)
        {
            return;
        }

        activeSet = true;
        activeColumn = 0;
        activeRow = (Model?.RowCount ?? 0) > 0 ? 0 : -1;
        rowsLayout.PinnedIndex = activeRow;
    }

    private void MarkActive(Control element)
    {
        if (!ReferenceEquals(activeElement, element))
        {
            switch (activeElement)
            {
                case DataTableCell old:
                    old.IsActive = false;
                    break;
                case DataTableHeaderCell oldHeader:
                    oldHeader.IsActive = false;
                    break;
            }

            activeElement = element;
        }

        switch (element)
        {
            case DataTableCell cell:
                cell.IsActive = true;
                break;
            case DataTableHeaderCell headerCell:
                headerCell.IsActive = true;
                break;
        }
    }

    // The control of a cell of the grid, when it is made.
    private Control? ElementOf(Cell cell)
    {
        if (cell.Row < 0)
        {
            return header is { } h && cell.Column >= 0 && cell.Column < h.Headers.Count ? h.Headers[cell.Column] : null;
        }

        if (repeater is null || Model is not { } model || cell.Row >= model.RowCount)
        {
            return null;
        }

        // The active row is kept made wherever it stands, so it is asked for by its place and made if it is not.
        DataTableRow? row = repeater.TryGetElement(cell.Row) as DataTableRow;
        if (row is null)
        {
            row = repeater.GetOrCreateElement(cell.Row) as DataTableRow;
            repeater.UpdateLayout();
        }

        return row is not null && cell.Column >= 0 && cell.Column < row.Cells.Count ? row.Cells[cell.Column] : null;
    }

    /// <summary>Makes a cell the active one, scrolls it into view, and gives it focus.</summary>
    /// <param name="cell">The cell.</param>
    /// <param name="focus">Whether to give it focus.</param>
    internal void SetActive(Cell cell, bool focus)
    {
        activeSet = true;
        activeRow = cell.Row;
        activeColumn = cell.Column;
        rowsLayout.PinnedIndex = activeRow;
        if (cell.Row >= 0)
        {
            RevealRow(cell.Row);
        }

        RevealColumn(cell.Column);
        TryActivate(focus, 0);
    }

    private void TryActivate(bool focus, int attempt)
    {
        if (ElementOf(new Cell(activeRow, activeColumn)) is { } element)
        {
            MarkActive(element);
            if (focus)
            {
                element.Focus(lastInputWasKeyboard ? FocusState.Keyboard : FocusState.Pointer);
            }

            return;
        }

        if (attempt < 8)
        {
            rowsLayout.Reflow();
            DispatcherQueue.TryEnqueue(DispatcherQueuePriority.Low, () => TryActivate(focus, attempt + 1));
        }
    }

    private void FocusActive()
    {
        NormalizeActive();
        lastInputWasKeyboard = true;
        TryActivate(focus: true, 0);
    }

    private void RevealRow(int row)
    {
        if (scroll is null || row < 0)
        {
            return;
        }

        Geometry geometry = rowsLayout.Geometry;
        double? offset = GridNavigation.ScrollOffsetToReveal(geometry.RowTop(row), geometry.HeightAt(row), scroll.VerticalOffset, scroll.ViewportHeight);
        if (offset is { } y)
        {
            scroll.ScrollTo(scroll.HorizontalOffset, y, new ScrollingScrollOptions(ScrollingAnimationMode.Disabled, ScrollingSnapPointsMode.Ignore));
        }
    }

    // A cell that scrolls is brought clear of the frozen columns; a frozen one is always in view.
    private void RevealColumn(int gridColumn)
    {
        if (scroll is null || IsList || gridColumn < 0 || gridColumn >= shown.Count || layout is null)
        {
            return;
        }

        PlacedColumn placed = shown[gridColumn].Placed;
        if (placed.Group != ColumnGroup.Scroll)
        {
            return;
        }

        double? offset = GridNavigation.ScrollOffsetToRevealX(placed.X, placed.Width, scroll.HorizontalOffset, scroll.ViewportWidth, layout.Fs, layout.Fe);
        if (offset is { } x)
        {
            scroll.ScrollTo(x, scroll.VerticalOffset, new ScrollingScrollOptions(ScrollingAnimationMode.Disabled, ScrollingSnapPointsMode.Ignore));
        }
    }

    private void OnPreviewKeyDownCore(object sender, KeyRoutedEventArgs e)
    {
        if (e.OriginalSource is not DependencyObject source)
        {
            return;
        }

        DataTableCell? cell = FindCell(source);
        DataTableHeaderCell? headerCell = FindHeaderCell(source);
        if ((cell is null && headerCell is null) || cell is { Role: not CellRole.Data })
        {
            return;
        }

        lastInputWasKeyboard = true;
        bool ctrl = IsKeyDown(VirtualKey.Control);
        bool shift = IsKeyDown(VirtualKey.Shift);
        bool alt = IsKeyDown(VirtualKey.Menu);
        int row = headerCell is not null ? -1 : cell!.Row?.Index ?? -1;
        int column = Math.Max(headerCell?.GridColumn ?? cell!.GridColumn, 0);
        if (Model is not { } model)
        {
            return;
        }

        if (headerCell is not null && HeaderKey(headerCell, e.Key, ctrl, shift, alt))
        {
            e.Handled = true;
            return;
        }

        if (headerCell is null && row >= 0 && (e.Key == VirtualKey.Application || (shift && e.Key == VirtualKey.F10)))
        {
            e.Handled = true;
            if (cell!.Row is { } menuRow)
            {
                ShowRowMenu(menuRow, cell, null);
            }

            return;
        }

        switch (e.Key)
        {
            case VirtualKey.Left:
            case VirtualKey.Right:
            case VirtualKey.Up:
            case VirtualKey.Down:
            case VirtualKey.Home:
            case VirtualKey.End:
            case VirtualKey.PageUp:
            case VirtualKey.PageDown:
                if (alt && row >= 0 && e.Key == VirtualKey.Down)
                {
                    OpenDetailAt(row, true);
                    e.Handled = true;
                    return;
                }

                if (alt && row >= 0 && e.Key == VirtualKey.Up)
                {
                    OpenDetailAt(row, false);
                    e.Handled = true;
                    return;
                }

                e.Handled = true;
                MoveActive(new Cell(row, column), e.Key, ctrl, shift, model);
                return;
            case VirtualKey.Space:
                e.Handled = true;
                if (headerCell is not null)
                {
                    headerCell.Row?.Owner?.SortPressed(headerCell);
                }
                else if (shift)
                {
                    ChooseRangeTo(row);
                }
                else
                {
                    ToggleChoice(row);
                }

                return;
            case VirtualKey.Enter:
                e.Handled = true;
                Activate(row, column, headerCell, model);
                return;
            case VirtualKey.A when ctrl:
                e.Handled = true;
                ChooseAllHeld();
                return;
            case VirtualKey.Escape when ChosenCount > 0 || choice.Across:
                e.Handled = true;
                ClearChoice();
                return;
            case VirtualKey.F2 when row >= 0 && model.GetRow(row).HasEdit && model.GetRow(row).Key is { } editKey:
                e.Handled = true;
                RaiseEditRequested(editKey);
                return;
            case VirtualKey.C when ctrl && row >= 0:
                e.Handled = true;
                CopyActive(row, column);
                return;
            case VirtualKey.F5:
            case VirtualKey.R when ctrl:
                e.Handled = true;
                RaiseRefreshRequested();
                return;
        }
    }

    // Typing the start of a name moves to the next row whose name starts so; a pause of 800 ms starts again.
    private void OnCharacterReceivedCore(UIElement sender, CharacterReceivedRoutedEventArgs e)
    {
        if (e.Handled || Model is not { } model || Presenter is not { } presenter)
        {
            return;
        }

        if (FindCell(e.OriginalSource as DependencyObject) is not { Role: CellRole.Data, Row.Index: >= 0 } cell)
        {
            return;
        }

        char character = e.Character;
        if (char.IsControl(character) || (char.IsWhiteSpace(character) && typed.Length == 0))
        {
            return;
        }

        DateTime now = DateTime.UtcNow;
        typed = (now - typedAt).TotalMilliseconds > 800 ? character.ToString() : typed + character;
        typedAt = now;
        rowNames ??= Enumerable.Range(0, model.RowCount).Select(i => presenter.RowName(model.GetRow(i))).ToList();
        int next = TypeAhead.Next(rowNames, cell.Row!.Index, typed, Culture);
        if (next >= 0)
        {
            SetActive(new Cell(next, Math.Max(activeColumn, 0)), focus: true);
            e.Handled = true;
        }
    }

    private void MoveActive(Cell current, VirtualKey key, bool ctrl, bool shift, IDataTableModel model)
    {
        NavKey nav = key switch
        {
            VirtualKey.Left => NavKey.Left,
            VirtualKey.Right => NavKey.Right,
            VirtualKey.Up => NavKey.Up,
            VirtualKey.Down => NavKey.Down,
            VirtualKey.Home => NavKey.Home,
            VirtualKey.End => NavKey.End,
            VirtualKey.PageUp => NavKey.PageUp,
            _ => NavKey.PageDown,
        };
        NavMods mods = (ctrl ? NavMods.Ctrl : NavMods.None) | (shift ? NavMods.Shift : NavMods.None);
        int page = GridNavigation.PageRows(scroll?.ViewportHeight ?? 0, rowHeight);
        bool rtl = FlowDirection == FlowDirection.RightToLeft;

        if (shift && current.Row >= 0 && key is VirtualKey.Up or VirtualKey.Down && current.Row < model.RowCount)
        {
            // A range grows from the row the chain began on.
            DataTableRecord at = model.GetRow(current.Row);
            if (anchorKey is null || model.IndexOfKey(anchorKey) < 0)
            {
                if (at.Key is not null && !IsChosen(at.Key, at.Choosable))
                {
                    ToggleChoice(current.Row);
                }

                anchorKey = at.Key;
            }

            Cell? stepped = GridNavigation.Move(current, nav, NavMods.None, model.RowCount, GridColumnCount, !IsList, page, rtl);
            if (stepped is { } to && to.Row >= 0)
            {
                SetActive(to, focus: true);
                ChooseRangeTo(to.Row);
            }

            return;
        }

        Cell? next = GridNavigation.Move(current, nav, mods, model.RowCount, GridColumnCount, !IsList, page, rtl);
        if (next is { } target)
        {
            SetActive(target, focus: true);
        }
    }

    private void Activate(int row, int column, DataTableHeaderCell? headerCell, IDataTableModel model)
    {
        if (headerCell is not null)
        {
            headerCell.Row?.Owner?.SortPressed(headerCell);
            return;
        }

        if (row < 0 || row >= model.RowCount)
        {
            return;
        }

        DataTableRecord record = model.GetRow(row);
        IDataTablePresenter? presenter = Presenter;
        int controls = presenter is null || IsList || column >= shown.Count ? 0 : presenter.ControlCount(record, shown[column].ModelIndex);
        bool nameCell = IsList || (column < shown.Count && shown[column].Column.Role == ColumnRole.Title);
        switch (GridNavigation.Activation(controls, nameCell && record.HasAction, record.HasAction, record.HasDetail))
        {
            case ActivationTarget.RowAction when record.Key is { } key:
                RaiseRowInvoked(key);
                break;
            case ActivationTarget.ToggleDetail when repeater?.TryGetElement(row) is DataTableRow detailRow:
                ToggleDetail(detailRow);
                break;
        }
    }

    private void OpenDetailAt(int row, bool open)
    {
        if (repeater?.TryGetElement(row) is DataTableRow target && target.Record.HasDetail && IsDetailOpen(target.Key) != open)
        {
            ToggleDetail(target);
        }
    }

    private void CopyActive(int row, int column)
    {
        if (Model is not { } model || Presenter is not { } presenter || row >= model.RowCount || (!IsList && column >= shown.Count))
        {
            return;
        }

        string text;
        if (IsList)
        {
            text = ClipboardText.ForCell(ListWords(model.GetRow(row)));
        }
        else if (ChosenCount > 0)
        {
            List<string> headers = shown.Select(s => s.Column.Header).ToList();
            List<IReadOnlyList<string>> lines = [];
            for (int i = 0; i < model.RowCount; i++)
            {
                DataTableRecord record = model.GetRow(i);
                if (record.Key is { } key && IsChosen(key, record.Choosable))
                {
                    lines.Add(shown.Select(s => presenter.CellText(record, s.ModelIndex)).ToList());
                }
            }

            text = ClipboardText.ForRows(headers, lines);
        }
        else
        {
            text = ClipboardText.ForCell(presenter.CellText(model.GetRow(row), shown[column].ModelIndex));
        }

        Windows.ApplicationModel.DataTransfer.DataPackage package = new();
        package.SetText(text);
        Windows.ApplicationModel.DataTransfer.Clipboard.SetContent(package);
    }

    private static DataTableHeaderCell? FindHeaderCell(DependencyObject? source)
    {
        while (source is not null)
        {
            if (source is DataTableHeaderCell cell)
            {
                return cell;
            }

            if (source is DataTableRow)
            {
                return null;
            }

            source = Microsoft.UI.Xaml.Media.VisualTreeHelper.GetParent(source);
        }

        return null;
    }
}
