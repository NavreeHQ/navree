using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;

namespace Navree.Controls;

/// <summary>The bar that stands over the toolbar while rows are chosen: the count, a way to clear, and the host's commands.</summary>
public sealed class DataTableBulkBar : Control
{
    private Button? clear;
    private TextBlock? count;
    private HyperlinkButton? chooseAll;
    private CommandBar? commands;
    private string countText = string.Empty;
    private string? chooseAllText;

    /// <summary>Makes a bar.</summary>
    public DataTableBulkBar()
    {
        DefaultStyleKey = typeof(DataTableBulkBar);
    }

    /// <summary>Raised when the clear button is pressed.</summary>
    internal event Action? ClearPressed;

    /// <summary>Raised when "choose all" is pressed.</summary>
    internal event Action? ChooseAllPressed;

    /// <summary>The bar's command bar, which holds the host's commands.</summary>
    internal CommandBar? Commands => commands;

    /// <summary>Sets what the bar says.</summary>
    /// <param name="countWords">The count of rows chosen, in words.</param>
    /// <param name="chooseAllWords">The words of the button that chooses every record that matches, or null when it is not offered.</param>
    internal void Set(string countWords, string? chooseAllWords)
    {
        countText = countWords;
        chooseAllText = chooseAllWords;
        Refresh();
    }

    /// <inheritdoc />
    protected override void OnApplyTemplate()
    {
        base.OnApplyTemplate();
        // A template applied again makes its parts again: the buttons of the parts it had are let go before the new ones are joined.
        if (clear is not null)
        {
            clear.Click -= OnClear;
        }

        if (chooseAll is not null)
        {
            chooseAll.Click -= OnChooseAll;
        }

        clear = GetTemplateChild("PART_Clear") as Button;
        count = GetTemplateChild("PART_Count") as TextBlock;
        chooseAll = GetTemplateChild("PART_ChooseAll") as HyperlinkButton;
        commands = GetTemplateChild("PART_Commands") as CommandBar;
        if (clear is not null)
        {
            AutomationProperties.SetName(clear, DataTableStrings.Get("DataTable_ClearChoice"));
            Microsoft.UI.Xaml.Controls.ToolTipService.SetToolTip(clear, DataTableStrings.Get("DataTable_ClearChoice"));
            clear.Click += OnClear;
        }

        if (chooseAll is not null)
        {
            chooseAll.Click += OnChooseAll;
        }

        Refresh();
    }

    private void OnClear(object sender, RoutedEventArgs e) => ClearPressed?.Invoke();

    private void OnChooseAll(object sender, RoutedEventArgs e) => ChooseAllPressed?.Invoke();

    private void Refresh()
    {
        if (count is not null)
        {
            count.Text = countText;
        }

        if (chooseAll is not null)
        {
            chooseAll.Content = chooseAllText ?? string.Empty;
            chooseAll.Visibility = chooseAllText is null ? Visibility.Collapsed : Visibility.Visible;
        }
    }
}
