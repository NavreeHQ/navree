namespace Navree.Controls;

/// <summary>
/// The table's subscription to its model's changes, which it holds only while it is in the tree: a model that lives longer
/// than the table (one a host keeps for the next time) must not keep the table alive or run its code while it is out of the tree.
/// </summary>
internal sealed class ModelLink
{
    private IDataTableModel? model;
    private EventHandler<DataTableModelChangedEventArgs>? handler;

    /// <summary>Whether the link holds a subscription now.</summary>
    internal bool IsAttached => model is not null;

    /// <summary>Subscribes to a model's changes, letting go of the model it was subscribed to.</summary>
    /// <param name="next">The model, or null for none.</param>
    /// <param name="on">What to run when the model changes.</param>
    internal void Attach(IDataTableModel? next, EventHandler<DataTableModelChangedEventArgs> on)
    {
        ArgumentNullException.ThrowIfNull(on);
        Detach();
        if (next is null)
        {
            return;
        }

        model = next;
        handler = on;
        next.Changed += on;
    }

    /// <summary>Lets go of the model: nothing it raises reaches the table any more.</summary>
    internal void Detach()
    {
        if (model is not null && handler is not null)
        {
            model.Changed -= handler;
        }

        model = null;
        handler = null;
    }
}
