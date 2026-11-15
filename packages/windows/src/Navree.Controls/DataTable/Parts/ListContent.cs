using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;

namespace Navree.Controls;

/// <summary>What a row of a narrow table holds: a headline, up to two lines under it, the states, a value at the end, and the dots of a menu.</summary>
public sealed class DataTableListContent : Control
{
    private TextBlock? headline;
    private TextBlock? firstLine;
    private TextBlock? secondLine;
    private TextBlock? states;
    private TextBlock? trailing;
    private DataTableGlyph? more;
    private string headlineText = string.Empty;
    private string first = string.Empty;
    private string second = string.Empty;
    private string stateText = string.Empty;
    private string trailingText = string.Empty;
    private bool hasMenu;

    /// <summary>Makes the content.</summary>
    public DataTableListContent()
    {
        DefaultStyleKey = typeof(DataTableListContent);
    }

    /// <summary>Sets what the content says.</summary>
    /// <param name="headlineWords">The first line.</param>
    /// <param name="lines">The lines under it; the first two are drawn.</param>
    /// <param name="statesWords">The states, in one line.</param>
    /// <param name="trailingWords">The value at the end.</param>
    /// <param name="menu">Whether the row has a menu.</param>
    internal void Set(string headlineWords, IReadOnlyList<string> lines, string statesWords, string trailingWords, bool menu)
    {
        headlineText = headlineWords;
        first = lines.Count > 0 ? lines[0] : string.Empty;
        second = lines.Count > 1 ? lines[1] : string.Empty;
        stateText = statesWords;
        trailingText = trailingWords;
        hasMenu = menu;
        Refresh();
    }

    /// <inheritdoc />
    protected override void OnApplyTemplate()
    {
        base.OnApplyTemplate();
        headline = GetTemplateChild("PART_Headline") as TextBlock;
        firstLine = GetTemplateChild("PART_Line1") as TextBlock;
        secondLine = GetTemplateChild("PART_Line2") as TextBlock;
        states = GetTemplateChild("PART_States") as TextBlock;
        trailing = GetTemplateChild("PART_Trailing") as TextBlock;
        more = GetTemplateChild("PART_More") as DataTableGlyph;
        if (more is not null)
        {
            more.Glyph = Glyphs.More;
        }

        Refresh();
    }

    private static void Show(TextBlock? block, string text)
    {
        if (block is null)
        {
            return;
        }

        block.Text = text;
        block.Visibility = text.Length == 0 ? Visibility.Collapsed : Visibility.Visible;
    }

    private void Refresh()
    {
        if (headline is not null)
        {
            headline.Text = headlineText;
        }

        Show(firstLine, first);
        Show(secondLine, second);
        Show(states, stateText);
        Show(trailing, trailingText);
        if (more is not null)
        {
            more.Visibility = hasMenu ? Visibility.Visible : Visibility.Collapsed;
        }
    }
}
