using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Automation.Peers;

namespace Navree.Controls;

public sealed partial class DataTable
{
    /// <inheritdoc />
    protected override AutomationPeer OnCreateAutomationPeer() => new DataTableAutomationPeer(this);

    // Says something to assistive technology without moving focus.
    private void RaiseNotification(string words)
    {
        if (string.IsNullOrEmpty(words))
        {
            return;
        }

        if (FrameworkElementAutomationPeer.FromElement(this) is { } peer)
        {
            peer.RaiseNotificationEvent(
                AutomationNotificationKind.ActionCompleted,
                AutomationNotificationProcessing.ImportantMostRecent,
                words,
                "DataTable");
        }
    }
}

/// <summary>What assistive technology is told of the table.</summary>
public sealed class DataTableAutomationPeer : FrameworkElementAutomationPeer
{
    private readonly DataTable table;

    /// <summary>Makes the peer.</summary>
    /// <param name="owner">The table.</param>
    public DataTableAutomationPeer(DataTable owner)
        : base(owner)
    {
        table = owner;
    }

    /// <inheritdoc />
    protected override AutomationControlType GetAutomationControlTypeCore() => AutomationControlType.DataGrid;

    /// <inheritdoc />
    protected override string GetClassNameCore() => nameof(DataTable);

    /// <inheritdoc />
    protected override string GetNameCore() =>
        string.IsNullOrEmpty(table.Caption) ? DataTableStrings.Get("DataTable_Name_Default") : table.Caption;
}
