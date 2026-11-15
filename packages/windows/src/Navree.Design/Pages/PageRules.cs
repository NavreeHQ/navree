using System.Buffers;
using System.Runtime.CompilerServices;
using System.Security.Cryptography;
using System.Text.Json;
using Navree.Protocol;

namespace Navree.Design.Pages;

/// <summary>
/// What the screen holds about the parts of a page, as words that can be compared. The renderer asks the screen for these when it draws a
/// part, so a part that is drawn from the same tree but whose states changed is not the same part. Each answer is empty when there is nothing.
/// </summary>
public interface IScreenFacts
{
    /// <summary>The states a part has (hidden, disabled, busy), by the part's id.</summary>
    /// <param name="id">The part's id.</param>
    /// <returns>The states written out, or an empty text.</returns>
    string StatesOf(string id);

    /// <summary>What a list or table that loads by cursor holds about itself (loading, failed, every row chosen), by id.</summary>
    /// <param name="id">The list's id.</param>
    /// <returns>Its state written out, or an empty text.</returns>
    string ListOf(string id);

    /// <summary>What a region holds about itself (loading, why it failed), by id.</summary>
    /// <param name="id">The region's id.</param>
    /// <returns>Its state written out, or an empty text.</returns>
    string RegionOf(string id);

    /// <summary>What a field holds (its value, what is wrong with it, the options a search found), by the form it is in and its id.</summary>
    /// <param name="form">The form's id, or an empty text when the field is in none.</param>
    /// <param name="field">The field's id.</param>
    /// <returns>The field written out, or an empty text.</returns>
    string FieldOf(string form, string field);

    /// <summary>Whether a sheet, dialog or popover of the tree is open.</summary>
    /// <param name="id">The overlay's id.</param>
    /// <returns>Whether it is.</returns>
    bool IsOpen(string id);
}

/// <summary>How the children of a container were matched with the children it had.</summary>
/// <param name="Old">For each new child, the place of the old child it continues, or minus one when it is new.</param>
/// <param name="Gone">The places of the old children that have no new child, in order.</param>
/// <param name="Moved">For each new child, whether it continues an old child that stood in another order among the others (a child that is new is not moved).</param>
/// <param name="Refused">Whether the matching was not made because two children of one list have one id: the container is drawn again.</param>
public sealed record Pairing(IReadOnlyList<int> Old, IReadOnlyList<int> Gone, IReadOnlyList<bool> Moved, bool Refused);

/// <summary>
/// When a page that is on the screen is brought up to date by an answer, what stands and what does not: a part stands if what it was drawn from
/// did not change, and only the parts that changed are touched. These are the rules, with nothing of the screen's controls in them.
/// </summary>
public static class PageRules
{
    private static readonly ConditionalWeakTable<Models.Node, byte[]> Digests = new();
    private static readonly ConditionalWeakTable<Models.TableRow, byte[]> RowDigests = new();

    /// <summary>
    /// A short stamp of a node's written form: two nodes that write the same have the same stamp. The models are records whose lists compare by
    /// reference, and every answer is read into new objects, so the protocol's own written form is what is compared; it is the form the
    /// server's tree is read from, so no part of a node that the client holds is left out of it.
    /// </summary>
    /// <param name="node">The node.</param>
    /// <returns>The stamp.</returns>
    public static byte[] Digest(Models.Node node)
    {
        if (Digests.TryGetValue(node, out byte[]? held))
        {
            return held;
        }

        ArrayBufferWriter<byte> buffer = new(1024);
        using (Utf8JsonWriter writer = new(buffer))
        {
            Models.WriteNode(writer, node);
        }

        byte[] stamp = SHA256.HashData(buffer.WrittenSpan);
        Digests.AddOrUpdate(node, stamp);
        return stamp;
    }

    /// <summary>Whether two nodes are equal by value, all the way down.</summary>
    /// <param name="a">One node.</param>
    /// <param name="b">The other.</param>
    /// <returns>True when they write the same.</returns>
    public static bool Same(Models.Node a, Models.Node b)
    {
        ArgumentNullException.ThrowIfNull(a);
        ArgumentNullException.ThrowIfNull(b);
        return ReferenceEquals(a, b) || (a.GetType() == b.GetType() && Digest(a).AsSpan().SequenceEqual(Digest(b)));
    }

    /// <summary>
    /// Whether two nodes are equal by value except for what the fields inside them hold: the value, the flag or the values of an input is the person's
    /// and the core's, not the tree's, so a bar of fields stands while only those differ.
    /// </summary>
    /// <param name="a">One node.</param>
    /// <param name="b">The other.</param>
    /// <returns>True when they write the same once the fields' starting values are left out.</returns>
    public static bool SameButFieldValues(Models.Node a, Models.Node b)
    {
        ArgumentNullException.ThrowIfNull(a);
        ArgumentNullException.ThrowIfNull(b);
        if (a.GetType() != b.GetType())
        {
            return false;
        }

        return ReferenceEquals(a, b) || System.Text.Json.Nodes.JsonNode.DeepEquals(WithoutFieldValues(a), WithoutFieldValues(b));
    }

    private static System.Text.Json.Nodes.JsonNode? WithoutFieldValues(Models.Node node)
    {
        ArrayBufferWriter<byte> buffer = new(1024);
        using (Utf8JsonWriter writer = new(buffer))
        {
            Models.WriteNode(writer, node);
        }

        System.Text.Json.Nodes.JsonNode? root = System.Text.Json.Nodes.JsonNode.Parse(buffer.WrittenSpan);
        Strip(root);
        return root;
    }

    private static void Strip(System.Text.Json.Nodes.JsonNode? at)
    {
        switch (at)
        {
            case System.Text.Json.Nodes.JsonObject one:
                if (one["t"]?.GetValue<string>() == "field" && one["input"] is System.Text.Json.Nodes.JsonObject input)
                {
                    foreach (string key in new[] { "value", "values", "on" })
                    {
                        _ = input.Remove(key);
                    }
                }

                foreach (KeyValuePair<string, System.Text.Json.Nodes.JsonNode?> pair in one.ToList())
                {
                    Strip(pair.Value);
                }

                break;
            case System.Text.Json.Nodes.JsonArray many:
                foreach (System.Text.Json.Nodes.JsonNode? item in many)
                {
                    Strip(item);
                }

                break;
            default:
                break;
        }
    }

    /// <summary>Whether two rows of a table are equal by value, all the way down: the cells, the actions, the detail, what is chosen and what is open.</summary>
    /// <param name="a">One row.</param>
    /// <param name="b">The other.</param>
    /// <returns>True when they write the same.</returns>
    public static bool SameRow(Models.TableRow a, Models.TableRow b)
    {
        ArgumentNullException.ThrowIfNull(a);
        ArgumentNullException.ThrowIfNull(b);
        return ReferenceEquals(a, b) || RowDigest(a).AsSpan().SequenceEqual(RowDigest(b));
    }

    private static byte[] RowDigest(Models.TableRow row)
    {
        if (RowDigests.TryGetValue(row, out byte[]? held))
        {
            return held;
        }

        ArrayBufferWriter<byte> buffer = new(512);
        using (Utf8JsonWriter writer = new(buffer))
        {
            Models.WriteTableRow(writer, row);
        }

        byte[] stamp = SHA256.HashData(buffer.WrittenSpan);
        RowDigests.AddOrUpdate(row, stamp);
        return stamp;
    }

    /// <summary>The nodes inside a node, the table's row details included: every place a part that has an id can stand.</summary>
    /// <param name="node">The node.</param>
    /// <returns>Its children.</returns>
    public static IEnumerable<Models.Node> ChildrenOf(Models.Node node)
    {
        foreach (Models.Node child in Tree.ChildNodes(node))
        {
            yield return child;
        }

        if (node is Models.Table table)
        {
            foreach (Models.TableRow row in table.Rows)
            {
                foreach (Models.Node detail in row.Detail)
                {
                    yield return detail;
                }
            }
        }
    }

    /// <summary>
    /// Whether what the screen holds about the parts inside a node is the same in two views of it: the states of each part, the state of each list
    /// and region, what each field holds, and whether each overlay is open. A node stands only when it is the same and this holds.
    /// </summary>
    /// <param name="node">The node (as it is in the newer tree; the older one is equal to it when this is asked).</param>
    /// <param name="before">The screen as the node was drawn from it.</param>
    /// <param name="after">The screen now.</param>
    /// <returns>True when none of it changed.</returns>
    public static bool SameAround(Models.Node node, IScreenFacts before, IScreenFacts after)
    {
        ArgumentNullException.ThrowIfNull(node);
        ArgumentNullException.ThrowIfNull(before);
        ArgumentNullException.ThrowIfNull(after);
        Stack<(Models.Node Node, string Form)> pending = new();
        pending.Push((node, string.Empty));
        while (pending.Count > 0)
        {
            (Models.Node current, string form) = pending.Pop();
            if (Tree.IdOf(current) is { } id)
            {
                if (before.StatesOf(id) != after.StatesOf(id)
                    || before.ListOf(id) != after.ListOf(id)
                    || before.RegionOf(id) != after.RegionOf(id)
                    || before.IsOpen(id) != after.IsOpen(id))
                {
                    return false;
                }
            }

            if (current is Models.Field field)
            {
                string key = field.Id ?? field.Name;
                if (before.FieldOf(form, key) != after.FieldOf(form, key))
                {
                    return false;
                }
            }

            string inner = current is Models.Form held ? Tree.FormId(held) : form;
            foreach (Models.Node child in ChildrenOf(current))
            {
                pending.Push((child, inner));
            }
        }

        return true;
    }

    /// <summary>What the screen holds around the parts of a row of a table (its cells, its actions, its detail) is the same in two views of it.</summary>
    /// <param name="row">The row.</param>
    /// <param name="before">The screen as the row was drawn from it.</param>
    /// <param name="after">The screen now.</param>
    /// <returns>True when none of it changed.</returns>
    public static bool SameAroundRow(Models.TableRow row, IScreenFacts before, IScreenFacts after)
    {
        ArgumentNullException.ThrowIfNull(row);
        return row.Cells.Concat(row.Actions).Concat(row.Detail).All(node => SameAround(node, before, after));
    }

    /// <summary>
    /// Matches the children of a container with the children it had: by id where both have one (and are of one kind), else by place and kind among
    /// the children that have none. A child that has moved to another place is reported as moved, not as gone and new.
    /// </summary>
    /// <param name="old">The children as drawn.</param>
    /// <param name="now">The children now.</param>
    /// <returns>How they match; refused when two children of one list have one id.</returns>
    public static Pairing Pair(IReadOnlyList<Models.Node> old, IReadOnlyList<Models.Node> now)
    {
        ArgumentNullException.ThrowIfNull(old);
        ArgumentNullException.ThrowIfNull(now);
        Dictionary<string, int> oldIds = new(StringComparer.Ordinal);
        for (int at = 0; at < old.Count; at++)
        {
            if (Tree.IdOf(old[at]) is { } id && !oldIds.TryAdd(id, at))
            {
                return Refuse(now.Count);
            }
        }

        HashSet<string> nowIds = new(StringComparer.Ordinal);
        foreach (Models.Node child in now)
        {
            if (Tree.IdOf(child) is { } id && !nowIds.Add(id))
            {
                return Refuse(now.Count);
            }
        }

        int[] match = new int[now.Count];
        bool[] used = new bool[old.Count];
        for (int at = 0; at < now.Count; at++)
        {
            match[at] = -1;
            if (Tree.IdOf(now[at]) is { } id && oldIds.TryGetValue(id, out int found) && old[found].GetType() == now[at].GetType())
            {
                match[at] = found;
                used[found] = true;
            }
        }

        for (int at = 0; at < now.Count; at++)
        {
            if (match[at] < 0 && at < old.Count && !used[at]
                && Tree.IdOf(now[at]) is null && Tree.IdOf(old[at]) is null
                && old[at].GetType() == now[at].GetType())
            {
                match[at] = at;
                used[at] = true;
            }
        }

        List<int> gone = [];
        for (int at = 0; at < old.Count; at++)
        {
            if (!used[at])
            {
                gone.Add(at);
            }
        }

        return new Pairing(match, gone, MovedAmong(match), false);
    }

    private static Pairing Refuse(int count) => new(Enumerable.Repeat(-1, count).ToArray(), [], new bool[count], true);

    // The children that stand in the order the old ones did are the longest run of rising old places; every other matched child has moved.
    private static bool[] MovedAmong(int[] match)
    {
        List<int> tails = [];
        int[] before = new int[match.Length];
        List<int> tailAt = [];
        for (int at = 0; at < match.Length; at++)
        {
            before[at] = -1;
            if (match[at] < 0)
            {
                continue;
            }

            int low = 0;
            int high = tails.Count;
            while (low < high)
            {
                int middle = (low + high) / 2;
                if (tails[middle] < match[at])
                {
                    low = middle + 1;
                }
                else
                {
                    high = middle;
                }
            }

            if (low == tails.Count)
            {
                tails.Add(match[at]);
                tailAt.Add(at);
            }
            else
            {
                tails[low] = match[at];
                tailAt[low] = at;
            }

            before[at] = low > 0 ? tailAt[low - 1] : -1;
        }

        bool[] moved = new bool[match.Length];
        for (int at = 0; at < match.Length; at++)
        {
            moved[at] = match[at] >= 0;
        }

        for (int at = tails.Count > 0 ? tailAt[^1] : -1; at >= 0; at = before[at])
        {
            moved[at] = false;
        }

        return moved;
    }
}
