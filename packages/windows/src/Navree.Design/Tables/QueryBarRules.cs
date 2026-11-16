using Navree.Design.Pages;
using Navree.Protocol;

namespace Navree.Design.Tables;

/// <summary>
/// The bar of a table's query form: its fields drawn above the table, the search field first. The bar stands across an answer, the same controls
/// showing what the screen holds for them, only while it sends to the same form; its fields are tied to the form by the form's id (its own id, or
/// <c>@</c> and its address when it has none: <see cref="Tree.FormId"/>), so a form with another id is another form and its bar is drawn again.
/// </summary>
public static class QueryBarRules
{
    /// <summary>The nodes the bar is drawn from, in the order they are drawn: the search field, then the rest.</summary>
    /// <param name="query">The table's query form.</param>
    /// <returns>The nodes.</returns>
    public static IReadOnlyList<Models.Node> Fields(Models.Form query)
    {
        ArgumentNullException.ThrowIfNull(query);
        Models.Field? search = query.Children.OfType<Models.Field>().FirstOrDefault(field => field.Input is Models.TextInput { Kind: Models.TextKind.Search });
        return search is null ? [.. query.Children] : [search, .. query.Children.Where(child => !ReferenceEquals(child, search))];
    }

    /// <summary>Whether the bar drawn for one query form stands for the next: the same form, and the same fields but for what they hold.</summary>
    /// <param name="before">The query form the bar was drawn for.</param>
    /// <param name="after">The query form the answer brings.</param>
    /// <returns>True when the bar stands.</returns>
    public static bool Stands(Models.Form before, Models.Form after)
    {
        ArgumentNullException.ThrowIfNull(before);
        ArgumentNullException.ThrowIfNull(after);
        if (Tree.FormId(before) != Tree.FormId(after))
        {
            return false;
        }

        IReadOnlyList<Models.Node> was = Fields(before);
        IReadOnlyList<Models.Node> now = Fields(after);
        return was.Count == now.Count && was.Zip(now).All(pair => PageRules.SameButFieldValues(pair.First, pair.Second));
    }
}
