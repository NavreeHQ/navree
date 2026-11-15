using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Navree.Design.Tables;

namespace Navree.Controls;

public sealed partial class DataTable
{
    private DataTableBulkBar? bulkBar;
    private FrameworkElement? toolbarPresenter;
    private int bulkSynced = -1;
    private string lastAnnouncement = string.Empty;
    private Microsoft.UI.Dispatching.DispatcherQueueTimer? choiceTimer;
    private Microsoft.UI.Dispatching.DispatcherQueueTimer? addedTimer;
    private int addedLater;
    private int addedEarlier;
    private DataTableLoadState lastLoadState = DataTableLoadState.Ready;

    private void WireBulkBar()
    {
        bulkBar = GetTemplateChild("PART_BulkBar") as DataTableBulkBar;
        toolbarPresenter = GetTemplateChild("PART_Toolbar") as FrameworkElement;
        if (bulkBar is not null)
        {
            bulkBar.ClearPressed -= ClearChoice;
            bulkBar.ClearPressed += ClearChoice;
            bulkBar.ChooseAllPressed -= ChooseAcross;
            bulkBar.ChooseAllPressed += ChooseAcross;
        }
    }

    // The bar shows while anything is chosen, over the toolbar, and says how many.
    private void UpdateBulkBar()
    {
        if (bulkBar is null)
        {
            return;
        }

        IDataTableModel? model = Model;
        bool any = model is not null && (choice.Across || choice.Keys.Count > 0);
        bulkBar.Visibility = any ? Visibility.Visible : Visibility.Collapsed;
        if (toolbarPresenter is not null)
        {
            toolbarPresenter.Visibility = any ? Visibility.Collapsed : Visibility.Visible;
        }

        if (!any || model is null)
        {
            return;
        }

        SyncBulkActions();
        (int held, int chosen) = CountChoice();
        long? total = model.Choice.AcrossTotal;
        string countWords = choice.Across
            ? total is { } all
                ? DataTableStrings.Get("DataTable_Chosen_All", Choosing.FormatCount(all, Culture))
                : DataTableStrings.Get("DataTable_Chosen_AllUnknown")
            : DataTableStrings.Get("DataTable_Chosen_Count", Choosing.FormatCount(chosen, Culture));
        string? offer = null;
        if (Choosing.OfferAcross(model.Choice.AcrossAvailable, choice.Across, held, chosen))
        {
            offer = total is { } n
                ? DataTableStrings.Get("DataTable_ChooseAll", Choosing.FormatCount(n, Culture))
                : DataTableStrings.Get("DataTable_ChooseAllMatching");
        }

        bulkBar.Set(countWords, offer);
    }

    private void SyncBulkActions()
    {
        CommandBar? bar = bulkBar?.Commands;
        if (bar is null || bulkSynced == BulkActions.Count)
        {
            return;
        }

        bar.PrimaryCommands.Clear();
        foreach (ICommandBarElement element in BulkActions)
        {
            bar.PrimaryCommands.Add(element);
        }

        bulkSynced = BulkActions.Count;
    }

    // The count of rows chosen is said once the person has stopped for 400 ms, so a held key does not talk.
    private void AnnounceChoice()
    {
        choiceTimer ??= DispatcherQueue.CreateTimer();
        choiceTimer.Stop();
        choiceTimer.Interval = TimeSpan.FromMilliseconds(400);
        choiceTimer.IsRepeating = false;
        choiceTimer.Tick -= OnChoiceTimer;
        choiceTimer.Tick += OnChoiceTimer;
        choiceTimer.Start();
    }

    private void OnChoiceTimer(Microsoft.UI.Dispatching.DispatcherQueueTimer sender, object args)
    {
        (_, int chosen) = CountChoice();
        long? total = Model?.Choice.AcrossTotal;
        Announcement announcement = Announcements.Chosen(chosen, total, choice.Across);
        Announce(DataTableStrings.Get(announcement, Culture));
    }

    // Rows that join while focus is in the table are said once, a second after the last.
    private void NoteRowsAdded(int count, bool earlier)
    {
        if (!FocusIsInside())
        {
            return;
        }

        if (earlier)
        {
            addedEarlier += count;
        }
        else
        {
            addedLater += count;
        }

        addedTimer ??= DispatcherQueue.CreateTimer();
        addedTimer.Stop();
        addedTimer.Interval = TimeSpan.FromSeconds(1);
        addedTimer.IsRepeating = false;
        addedTimer.Tick -= OnAddedTimer;
        addedTimer.Tick += OnAddedTimer;
        addedTimer.Start();
    }

    private void OnAddedTimer(Microsoft.UI.Dispatching.DispatcherQueueTimer sender, object args)
    {
        if (addedLater > 0)
        {
            Announce(DataTableStrings.Get(Announcements.RowsAdded(addedLater, earlier: false), Culture));
        }

        if (addedEarlier > 0)
        {
            Announce(DataTableStrings.Get(Announcements.RowsAdded(addedEarlier, earlier: true), Culture));
        }

        addedLater = 0;
        addedEarlier = 0;
    }

    private bool FocusIsInside()
    {
        DependencyObject? at = Microsoft.UI.Xaml.Input.FocusManager.GetFocusedElement(XamlRoot) as DependencyObject;
        while (at is not null)
        {
            if (ReferenceEquals(at, this))
            {
                return true;
            }

            at = Microsoft.UI.Xaml.Media.VisualTreeHelper.GetParent(at);
        }

        return false;
    }

    /// <summary>Makes a row that is far from the view, as assistive technology asks.</summary>
    /// <param name="index">The row's place.</param>
    internal void RealizeRow(int index)
    {
        if (repeater is null || Model is not { } model || index < 0 || index >= model.RowCount)
        {
            return;
        }

        rowsLayout.PinnedIndex = index;
        rowsLayout.Reflow();
        _ = repeater.GetOrCreateElement(index);
        repeater.UpdateLayout();
        ReleasePinSoon();
    }

    private void Announce(string words)
    {
        lastAnnouncement = words;
        RaiseNotification(words);
    }

    /// <summary>The last words the table said to assistive technology; a run reads them.</summary>
    internal string LastAnnouncement => lastAnnouncement;
}
