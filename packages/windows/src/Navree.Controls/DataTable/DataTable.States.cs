using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Navree.Design.Tables;

namespace Navree.Controls;

public sealed partial class DataTable
{
    private Grid? statePanel;
    private StackPanel? emptyPanel;
    private StackPanel? loadingPanel;
    private InfoBar? failedBar;
    private TextBlock? emptyText;
    private TextBlock? loadingText;
    private Grid? footer;
    private TextBlock? summaryText;
    private ProgressRing? footerRing;
    private TextBlock? footerStatus;
    private HyperlinkButton? footerRetry;
    private UIElement? customEmpty;
    private bool stateWired;

    private void WireStateParts()
    {
        statePanel = GetTemplateChild("PART_State") as Grid;
        emptyPanel = GetTemplateChild("PART_Empty") as StackPanel;
        loadingPanel = GetTemplateChild("PART_Loading") as StackPanel;
        failedBar = GetTemplateChild("PART_Failed") as InfoBar;
        emptyText = GetTemplateChild("PART_EmptyText") as TextBlock;
        loadingText = GetTemplateChild("PART_LoadingText") as TextBlock;
        footer = GetTemplateChild("PART_Footer") as Grid;
        summaryText = GetTemplateChild("PART_Summary") as TextBlock;
        footerRing = GetTemplateChild("PART_FooterRing") as ProgressRing;
        footerStatus = GetTemplateChild("PART_FooterStatus") as TextBlock;
        footerRetry = GetTemplateChild("PART_FooterRetry") as HyperlinkButton;
        if (stateWired)
        {
            return;
        }

        stateWired = true;
        if (emptyText is not null)
        {
            emptyText.Text = DataTableStrings.Get("DataTable_Empty_Title");
        }

        if (loadingText is not null)
        {
            loadingText.Text = DataTableStrings.Get("DataTable_Loading");
        }

        if (failedBar is not null)
        {
            Button retry = new() { Content = DataTableStrings.Get("DataTable_Retry") };
            retry.Click += (_, _) => RaiseRetryRequested(RetryKind.Initial);
            failedBar.ActionButton = retry;
        }

        if (footerRetry is not null)
        {
            footerRetry.Content = DataTableStrings.Get("DataTable_Retry");
            footerRetry.Click += (_, _) => RaiseRetryRequested(RetryKind.More);
        }
    }

    // What the body shows when there are no rows: nothing here, loading, or why the first rows could not be had.
    private void UpdateState()
    {
        if (statePanel is null)
        {
            return;
        }

        IDataTableModel? model = Model;
        if (model is null || model.RowCount > 0)
        {
            statePanel.Visibility = Visibility.Collapsed;
            return;
        }

        statePanel.Visibility = Visibility.Visible;
        bool empty = model.State == DataTableLoadState.Ready;
        bool custom = empty && EmptyContent is not null;
        if (emptyPanel is not null)
        {
            emptyPanel.Visibility = empty && !custom ? Visibility.Visible : Visibility.Collapsed;
        }

        if (customEmpty is not null && !ReferenceEquals(customEmpty, EmptyContent))
        {
            statePanel.Children.Remove(customEmpty);
            customEmpty = null;
        }

        if (custom && customEmpty is null && EmptyContent is { } content)
        {
            customEmpty = content;
            statePanel.Children.Add(content);
        }

        if (loadingPanel is not null)
        {
            loadingPanel.Visibility = model.State == DataTableLoadState.Loading ? Visibility.Visible : Visibility.Collapsed;
        }

        if (failedBar is not null)
        {
            bool failed = model.State == DataTableLoadState.Failed;
            failedBar.Visibility = failed ? Visibility.Visible : Visibility.Collapsed;
            failedBar.Message = model.FailureMessage ?? string.Empty;
            failedBar.IsOpen = failed;
        }
    }

    // The footer: the server's summary at the start, and what is happening to the rows after the last at the end.
    private void UpdateFooter()
    {
        if (footer is null || Model is not { } model)
        {
            return;
        }

        string summary = model.Summary ?? string.Empty;
        bool limit = model.RowCount >= RowGeometry.MaxHeldRows(rowHeight) && model.RowCount > 0;
        if (limit)
        {
            summary = DataTableStrings.Get("DataTable_HeldLimit", Choosing.FormatCount(model.RowCount, Culture));
        }

        if (summaryText is not null)
        {
            summaryText.Text = summary;
        }

        CursorState later = model.Later.State;
        if (footerRing is not null)
        {
            footerRing.Visibility = later == CursorState.Loading ? Visibility.Visible : Visibility.Collapsed;
        }

        if (footerStatus is not null)
        {
            footerStatus.Text = later switch
            {
                CursorState.Loading => DataTableStrings.Get("DataTable_LoadingMore"),
                CursorState.Failed => DataTableStrings.Get("DataTable_LoadMoreFailed"),
                _ => string.Empty,
            };
        }

        if (footerRetry is not null)
        {
            footerRetry.Visibility = later == CursorState.Failed ? Visibility.Visible : Visibility.Collapsed;
        }

        bool show = model.RowCount > 0 && (summary.Length > 0 || later != CursorState.Idle);
        footer.Visibility = show ? Visibility.Visible : Visibility.Collapsed;
    }
}
