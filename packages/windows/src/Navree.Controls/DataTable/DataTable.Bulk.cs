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

    private void AnnounceChoice()
    {
        (_, int chosen) = CountChoice();
        long? total = Model?.Choice.AcrossTotal;
        Announcement announcement = Announcements.Chosen(chosen, total, choice.Across);
        Announce(DataTableStrings.Get(announcement));
    }

    private void Announce(string words)
    {
        lastAnnouncement = words;
        RaiseNotification(words);
    }

    /// <summary>The last words the table said to assistive technology; a run reads them.</summary>
    internal string LastAnnouncement => lastAnnouncement;
}
