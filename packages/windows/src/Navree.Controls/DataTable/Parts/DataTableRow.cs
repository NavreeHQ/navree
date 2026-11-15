using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;
using Navree.Design.Tables;

namespace Navree.Controls;

/// <summary>
/// A row of the table, and the row of headings, which is one too. Its parts are five layers from
/// back to front: the chrome, the scrolling cells, the frozen cells at the start, the frozen cells
/// at the end, and under them the detail. The table moves each layer against the scroll.
/// </summary>
public sealed class DataTableRow : Control
{
    private Grid? root;
    private Grid? chrome;
    private Canvas? scrollCells;
    private Canvas? frozenStart;
    private Canvas? frozenEnd;
    private Border? detailCard;
    private readonly List<DataTableCell> cells = [];
    private readonly List<DataTableHeaderCell> headers = [];
    private DataTableCell? checkCell;
    private DataTableCell? chevronCell;
    private DataTableCheck? check;
    private DataTableGlyph? chevron;
    private int builtVersion = -1;
    private bool over;
    private bool pressed;
    private bool chosen;
    private UIElement? detailContent;

    /// <summary>Makes a row.</summary>
    public DataTableRow()
    {
        DefaultStyleKey = typeof(DataTableRow);
    }

    /// <summary>The table the row is in.</summary>
    internal DataTable? Owner { get; set; }

    /// <summary>Whether the row is the row of headings.</summary>
    internal bool IsHeader { get; set; }

    /// <summary>The row's place among the rows held; minus one for the headings or a row not in use.</summary>
    internal int Index { get; private set; } = -1;

    /// <summary>The row's key, or null.</summary>
    internal string? Key { get; private set; }

    /// <summary>What the model said of the row when it was bound.</summary>
    internal DataTableRecord Record { get; private set; }

    /// <summary>The layer that holds the row's fill, which stays at the start edge.</summary>
    internal FrameworkElement? ChromeLayer => chrome;

    /// <summary>The layer of the scrolling cells.</summary>
    internal Canvas? ScrollCells => scrollCells;

    /// <summary>The layer of the cells frozen at the start.</summary>
    internal Canvas? FrozenStart => frozenStart;

    /// <summary>The layer of the cells frozen at the end.</summary>
    internal Canvas? FrozenEnd => frozenEnd;

    /// <summary>The card that holds the detail, which stays at the start edge.</summary>
    internal FrameworkElement? DetailLayer => detailCard;

    /// <summary>The cells of the row's columns, in display order.</summary>
    internal IReadOnlyList<DataTableCell> Cells => cells;

    /// <summary>The heading cells, in display order.</summary>
    internal IReadOnlyList<DataTableHeaderCell> Headers => headers;

    /// <summary>Whether the template has been made and the layers are there.</summary>
    internal bool HasParts => root is not null;

    /// <summary>The detail's content now, or null.</summary>
    internal UIElement? DetailContent => detailContent;

    /// <summary>Builds the template now if it is not built.</summary>
    internal void EnsureParts()
    {
        if (root is null)
        {
            ApplyTemplate();
        }
    }

    /// <inheritdoc />
    protected override void OnApplyTemplate()
    {
        base.OnApplyTemplate();
        root = GetTemplateChild("PART_Root") as Grid;
        chrome = GetTemplateChild("PART_Chrome") as Grid;
        scrollCells = GetTemplateChild("PART_ScrollCells") as Canvas;
        frozenStart = GetTemplateChild("PART_FrozenStart") as Canvas;
        frozenEnd = GetTemplateChild("PART_FrozenEnd") as Canvas;
        detailCard = GetTemplateChild("PART_DetailCard") as Border;
        builtVersion = -1;
        Owner?.RowPartsMade(this);
        UpdateState();
    }

    /// <summary>Binds the row to a place among the rows held.</summary>
    /// <param name="owner">The table.</param>
    /// <param name="index">The place.</param>
    internal void Bind(DataTable owner, int index)
    {
        Owner = owner;
        IsHeader = false;
        Index = index;
        EnsureParts();
        Refresh();
    }

    /// <summary>Gives the row's place when it changed and the row is the same.</summary>
    /// <param name="index">The new place.</param>
    internal void MoveTo(int index) => Index = index;

    /// <summary>Lets go of what the row was bound to.</summary>
    internal void Unbind()
    {
        IDataTablePresenter? presenter = Owner?.Presenter;
        if (presenter is not null)
        {
            foreach (DataTableCell cell in cells)
            {
                if (cell.Content is UIElement content)
                {
                    presenter.ClearCell(content);
                }
            }
        }

        ClearDetail();
        Key = null;
        Index = -1;
        chosen = false;
        over = false;
        pressed = false;
    }

    /// <summary>Draws the row again from the model and the table's state.</summary>
    internal void Refresh()
    {
        DataTable? table = Owner;
        if (table is null || root is null)
        {
            return;
        }

        if (IsHeader)
        {
            RefreshHeader(table);
            return;
        }

        if (Index < 0 || table.Model is not { } model || Index >= model.RowCount)
        {
            return;
        }

        Record = model.GetRow(Index);
        Key = Record.Key;
        if (builtVersion != table.LayoutVersion)
        {
            RebuildCells(table);
        }

        ApplyNumbers(table);
        IDataTablePresenter? presenter = table.Presenter;
        if (presenter is not null)
        {
            for (int i = 0; i < cells.Count; i++)
            {
                DataTableCell cell = cells[i];
                if (cell.Content is UIElement content)
                {
                    presenter.BindCell(content, Record, cell.ColumnIndex);
                }

                cell.IsActive = table.IsActiveCell(Index, cell.GridColumn);
            }
        }

        chosen = Key is not null && table.IsChosen(Key, Record.Choosable);
        if (check is not null)
        {
            check.State = chosen ? HeaderBoxState.On : HeaderBoxState.Off;
            check.Available = Record.Choosable == Choosable.Yes;
            check.Visibility = Record.Choosable == Choosable.None ? Visibility.Collapsed : Visibility.Visible;
        }

        if (chevron is not null)
        {
            chevron.Glyph = table.IsDetailOpen(Key) ? Glyphs.ChevronUp : Glyphs.ChevronDown;
            chevron.Visibility = Record.HasDetail ? Visibility.Visible : Visibility.Collapsed;
        }

        UpdateDetail(table);
        UpdateState();
    }

    /// <summary>Draws the choice again without binding the cells.</summary>
    internal void RefreshChoice()
    {
        DataTable? table = Owner;
        if (table is null || IsHeader || Key is null)
        {
            return;
        }

        chosen = table.IsChosen(Key, Record.Choosable);
        if (check is not null)
        {
            check.State = chosen ? HeaderBoxState.On : HeaderBoxState.Off;
        }

        UpdateState();
    }

    /// <summary>Sets the cells' positions and sizes from the table's layout.</summary>
    internal void ApplyNumbers(DataTable table)
    {
        ColumnLayout? layout = table.CurrentLayout;
        if (layout is null || root is null)
        {
            return;
        }

        double height = IsHeader ? table.HeaderHeight : table.RowHeight;
        root.RowDefinitions[0].Height = new GridLength(height);
        root.Width = layout.RowWidth;
        if (chrome is not null)
        {
            chrome.Width = Math.Max(layout.Viewport, 0);
            chrome.Height = height;
        }

        if (detailCard is not null)
        {
            detailCard.Width = Math.Max(0, layout.Viewport - 32);
        }

        double cellHeight = Math.Max(0, height - 1);
        foreach (DataTableCell cell in cells)
        {
            Place(cell, cell.ColumnIndex, layout, table, cellHeight);
        }

        foreach (DataTableHeaderCell header in headers)
        {
            PlaceHeader(header, layout, table, cellHeight);
        }

        if (checkCell is not null)
        {
            checkCell.Width = FrozenGutter.CheckWidth;
            checkCell.Height = cellHeight;
            Canvas.SetLeft(checkCell, 0);
        }

        if (chevronCell is not null)
        {
            chevronCell.Width = FrozenGutter.ChevronWidth;
            chevronCell.Height = cellHeight;
            Canvas.SetLeft(chevronCell, table.ChevronLeft);
        }
    }

    /// <inheritdoc />
    protected override void OnPointerEntered(PointerRoutedEventArgs e)
    {
        base.OnPointerEntered(e);
        over = !IsHeader && e.Pointer.PointerDeviceType != Microsoft.UI.Input.PointerDeviceType.Touch;
        UpdateState();
    }

    /// <inheritdoc />
    protected override void OnPointerExited(PointerRoutedEventArgs e)
    {
        base.OnPointerExited(e);
        over = false;
        pressed = false;
        UpdateState();
    }

    /// <inheritdoc />
    protected override void OnPointerPressed(PointerRoutedEventArgs e)
    {
        base.OnPointerPressed(e);
        pressed = !IsHeader;
        UpdateState();
    }

    /// <inheritdoc />
    protected override void OnPointerReleased(PointerRoutedEventArgs e)
    {
        base.OnPointerReleased(e);
        pressed = false;
        UpdateState();
    }

    /// <inheritdoc />
    protected override void OnPointerCaptureLost(PointerRoutedEventArgs e)
    {
        base.OnPointerCaptureLost(e);
        pressed = false;
        UpdateState();
    }

    /// <inheritdoc />
    protected override void OnTapped(TappedRoutedEventArgs e)
    {
        base.OnTapped(e);
        if (!IsHeader && !e.Handled)
        {
            Owner?.RowTapped(this, e);
        }
    }

    /// <inheritdoc />
    protected override void OnRightTapped(RightTappedRoutedEventArgs e)
    {
        base.OnRightTapped(e);
        if (!IsHeader && !e.Handled)
        {
            Owner?.RowRightTapped(this, e);
        }
    }

    private void UpdateState()
    {
        string state = (chosen, pressed, over) switch
        {
            (true, true, _) => "ChosenPressed",
            (true, false, true) => "ChosenPointerOver",
            (true, false, false) => "Chosen",
            (false, true, _) => "Pressed",
            (false, false, true) => "PointerOver",
            _ => "Normal",
        };
        VisualStateManager.GoToState(this, state, false);
    }

    private void Place(DataTableCell cell, int columnIndex, ColumnLayout layout, DataTable table, double height)
    {
        PlacedColumn? placed = table.PlacedFor(columnIndex);
        if (placed is not { } column)
        {
            return;
        }

        cell.Width = column.Width;
        cell.Height = height;
        Canvas.SetLeft(cell, column.X);
    }

    private void PlaceHeader(DataTableHeaderCell header, ColumnLayout layout, DataTable table, double height)
    {
        PlacedColumn? placed = table.PlacedFor(header.ColumnIndex);
        if (placed is not { } column)
        {
            return;
        }

        header.Width = column.Width;
        header.Height = height;
        Canvas.SetLeft(header, column.X);
    }

    private Canvas? GroupOf(ColumnGroup group) => group switch
    {
        ColumnGroup.FrozenStart => frozenStart,
        ColumnGroup.FrozenEnd => frozenEnd,
        _ => scrollCells,
    };

    private void ClearLayers()
    {
        scrollCells?.Children.Clear();
        frozenStart?.Children.Clear();
        frozenEnd?.Children.Clear();
        cells.Clear();
        headers.Clear();
        checkCell = null;
        chevronCell = null;
        check = null;
        chevron = null;
    }

    private void RebuildCells(DataTable table)
    {
        IDataTablePresenter? presenter = table.Presenter;
        if (presenter is not null)
        {
            foreach (DataTableCell cell in cells)
            {
                if (cell.Content is UIElement content)
                {
                    presenter.ClearCell(content);
                }
            }
        }

        ClearLayers();
        builtVersion = table.LayoutVersion;
        if (table.CurrentLayout is null || presenter is null)
        {
            return;
        }

        int gridColumn = 0;
        if (table.HasCheckColumn)
        {
            check = new DataTableCheck { HorizontalAlignment = HorizontalAlignment.Center, VerticalAlignment = VerticalAlignment.Center };
            checkCell = new DataTableCell { Role = CellRole.Check, Row = this, GridColumn = -1, Padding = new Thickness(0), Content = check };
            frozenStart?.Children.Add(checkCell);
        }

        if (table.HasChevronColumn)
        {
            chevron = new DataTableGlyph { Glyph = Glyphs.ChevronDown, HorizontalAlignment = HorizontalAlignment.Center, VerticalAlignment = VerticalAlignment.Center, Width = 28, Height = 28 };
            chevronCell = new DataTableCell { Role = CellRole.Chevron, Row = this, GridColumn = -1, Padding = new Thickness(0), Content = chevron };
            frozenStart?.Children.Add(chevronCell);
        }

        foreach (ShownColumn shown in table.Shown)
        {
            UIElement content = presenter.CreateCell(shown.Column, table.Shape);
            DataTableCell cell = new()
            {
                Role = CellRole.Data,
                Row = this,
                ColumnIndex = shown.ModelIndex,
                GridColumn = gridColumn++,
                Content = content,
            };
            cells.Add(cell);
            GroupOf(shown.Placed.Group)?.Children.Add(cell);
        }
    }

    private void RefreshHeader(DataTable table)
    {
        if (builtVersion != table.LayoutVersion)
        {
            ClearLayers();
            builtVersion = table.LayoutVersion;
            if (table.CurrentLayout is null)
            {
                return;
            }

            int gridColumn = 0;
            if (table.HasCheckColumn)
            {
                check = new DataTableCheck { HorizontalAlignment = HorizontalAlignment.Center, VerticalAlignment = VerticalAlignment.Center };
                checkCell = new DataTableCell { Role = CellRole.Check, Row = this, GridColumn = -1, Padding = new Thickness(0), Content = check };
                frozenStart?.Children.Add(checkCell);
            }

            if (table.HasChevronColumn)
            {
                chevronCell = new DataTableCell { Role = CellRole.Chevron, Row = this, GridColumn = -1, Padding = new Thickness(0) };
                frozenStart?.Children.Add(chevronCell);
            }

            foreach (ShownColumn shown in table.Shown)
            {
                DataTableHeaderCell header = new() { ColumnIndex = shown.ModelIndex, Row = this, GridColumn = gridColumn++ };
                headers.Add(header);
                GroupOf(shown.Placed.Group)?.Children.Add(header);
            }
        }

        foreach (DataTableHeaderCell header in headers)
        {
            DataTableColumn column = table.ColumnAt(header.ColumnIndex);
            header.Set(column.Role == ColumnRole.Actions ? string.Empty : column.Header, column.Align, column.Sort, hasMenu: false);
            header.IsActive = table.IsActiveHeader(header.GridColumn);
        }

        if (check is not null)
        {
            check.State = table.HeaderBox();
        }

        ApplyNumbers(table);
    }

    private void UpdateDetail(DataTable table)
    {
        if (detailCard is null)
        {
            return;
        }

        bool open = Record.HasDetail && table.IsDetailOpen(Key);
        if (!open)
        {
            ClearDetail();
            return;
        }

        if (detailContent is null && table.Presenter?.CreateDetail(Record) is { } content)
        {
            detailContent = content;
            detailCard.Child = content;
        }

        detailCard.Visibility = detailContent is null ? Visibility.Collapsed : Visibility.Visible;
    }

    private void ClearDetail()
    {
        detailContent = null;
        if (detailCard is not null)
        {
            detailCard.Child = null;
            detailCard.Visibility = Visibility.Collapsed;
        }
    }
}

/// <summary>The widths of the gutter's cells.</summary>
internal static class FrozenGutter
{
    /// <summary>The check box cell.</summary>
    internal const double CheckWidth = 40;

    /// <summary>The chevron cell.</summary>
    internal const double ChevronWidth = 40;
}
