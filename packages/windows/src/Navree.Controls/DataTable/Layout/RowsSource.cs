using System.Collections;
using System.Collections.Specialized;
using Microsoft.UI.Xaml.Controls;

namespace Navree.Controls;

/// <summary>What the repeater is given for a row: only the row's key. The row's data stays with the model.</summary>
internal sealed class RowHandle(string key)
{
    /// <summary>The row's key.</summary>
    internal string Key { get; } = key;
}

/// <summary>
/// The rows as the repeater reads them: a list that cannot be changed through the interface, tells
/// the repeater about every change as one event, and maps keys to places and back so a reset keeps
/// the elements it can.
/// </summary>
internal sealed partial class RowsSource : IList, INotifyCollectionChanged, IKeyIndexMapping
{
    private readonly List<RowHandle> items = [];
    private readonly Dictionary<string, int> places = new(StringComparer.Ordinal);

    /// <inheritdoc />
    public event NotifyCollectionChangedEventHandler? CollectionChanged;

    /// <inheritdoc />
    public int Count => items.Count;

    /// <inheritdoc />
    public bool IsFixedSize => true;

    /// <inheritdoc />
    public bool IsReadOnly => true;

    /// <inheritdoc />
    public bool IsSynchronized => false;

    /// <inheritdoc />
    public object SyncRoot => items;

    /// <inheritdoc />
    public object? this[int index]
    {
        get => items[index];
        set => throw new NotSupportedException();
    }

    /// <summary>The handle at a place.</summary>
    /// <param name="index">The place.</param>
    /// <returns>The handle.</returns>
    internal RowHandle At(int index) => items[index];

    /// <summary>Replaces every row, as a new query does.</summary>
    /// <param name="rows">The new rows.</param>
    internal void Reset(IEnumerable<RowHandle> rows)
    {
        items.Clear();
        items.AddRange(rows);
        Reindex();
        CollectionChanged?.Invoke(this, new NotifyCollectionChangedEventArgs(NotifyCollectionChangedAction.Reset));
    }

    /// <summary>Adds rows at a place.</summary>
    /// <param name="at">The place of the first new row.</param>
    /// <param name="rows">The rows.</param>
    internal void InsertRows(int at, IReadOnlyList<RowHandle> rows)
    {
        if (rows.Count == 0)
        {
            return;
        }

        at = Math.Clamp(at, 0, items.Count);
        bool atEnd = at == items.Count;
        items.InsertRange(at, rows);
        if (atEnd)
        {
            for (int i = 0; i < rows.Count; i++)
            {
                places[rows[i].Key] = at + i;
            }
        }
        else
        {
            Reindex();
        }

        CollectionChanged?.Invoke(this, new NotifyCollectionChangedEventArgs(NotifyCollectionChangedAction.Add, rows.ToList(), at));
    }

    /// <summary>Adds rows at the end.</summary>
    /// <param name="rows">The rows.</param>
    internal void Append(IReadOnlyList<RowHandle> rows) => InsertRows(items.Count, rows);

    /// <summary>Adds rows at the start.</summary>
    /// <param name="rows">The rows.</param>
    internal void Prepend(IReadOnlyList<RowHandle> rows) => InsertRows(0, rows);

    /// <summary>Takes rows out.</summary>
    /// <param name="at">The place of the first.</param>
    /// <param name="count">How many.</param>
    internal void RemoveRange(int at, int count)
    {
        if (count <= 0)
        {
            return;
        }

        List<RowHandle> removed = items.GetRange(at, count);
        items.RemoveRange(at, count);
        Reindex();
        CollectionChanged?.Invoke(this, new NotifyCollectionChangedEventArgs(NotifyCollectionChangedAction.Remove, removed, at));
    }

    /// <summary>Puts another row where one stood, as when the server gives a row another key.</summary>
    /// <param name="at">The place.</param>
    /// <param name="row">The new handle.</param>
    internal void Replace(int at, RowHandle row)
    {
        RowHandle old = items[at];
        items[at] = row;
        if (places.TryGetValue(old.Key, out int held) && held == at)
        {
            places.Remove(old.Key);
        }

        places[row.Key] = at;
        CollectionChanged?.Invoke(this, new NotifyCollectionChangedEventArgs(NotifyCollectionChangedAction.Replace, row, old, at));
    }

    /// <summary>Draws one row again with other data: the key stays.</summary>
    /// <param name="at">The place.</param>
    internal void Redraw(int at)
    {
        RowHandle row = items[at];
        CollectionChanged?.Invoke(this, new NotifyCollectionChangedEventArgs(NotifyCollectionChangedAction.Replace, row, row, at));
    }

    /// <summary>The place of a key, or minus one.</summary>
    /// <param name="key">The key.</param>
    /// <returns>The place.</returns>
    internal int PlaceOf(string key) => places.TryGetValue(key, out int at) ? at : -1;

    /// <inheritdoc />
    public string KeyFromIndex(int index) => items[index].Key;

    /// <inheritdoc />
    public int IndexFromKey(string key) => PlaceOf(key);

    /// <inheritdoc />
    public IEnumerator GetEnumerator() => items.GetEnumerator();

    /// <inheritdoc />
    public void CopyTo(Array array, int index) => ((ICollection)items).CopyTo(array, index);

    /// <inheritdoc />
    public bool Contains(object? value) => value is RowHandle row && places.ContainsKey(row.Key);

    /// <inheritdoc />
    public int IndexOf(object? value) => value is RowHandle row ? PlaceOf(row.Key) : -1;

    /// <inheritdoc />
    public int Add(object? value) => throw new NotSupportedException();

    /// <inheritdoc />
    public void Clear() => throw new NotSupportedException();

    /// <inheritdoc />
    public void Insert(int index, object? value) => throw new NotSupportedException();

    /// <inheritdoc />
    public void Remove(object? value) => throw new NotSupportedException();

    /// <inheritdoc />
    public void RemoveAt(int index) => throw new NotSupportedException();

    private void Reindex()
    {
        places.Clear();
        for (int i = 0; i < items.Count; i++)
        {
            places[items[i].Key] = i;
        }
    }
}
