using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Markup;

namespace Navree.Gallery;

/// <summary>
/// The box that chooses a row, tried on a page of its own in the colours the system gives in high contrast, in two ways: by states
/// that set the system's colours when high contrast is on ("states"), and by a dictionary of brushes with a theme dictionary for
/// high contrast ("dictionary"). Each way is run in a process of its own, so a way that stops the app costs one page.
/// What each way drew, and what it said when it could not, is in the report and in the pictures.
/// </summary>
internal static class ContrastScenario
{
    private const string Head = """
        <ControlTemplate xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation" xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml" TargetType="ContentControl">
          <Grid x:Name="PART_Root" Width="60" Height="60">
            <Border x:Name="PART_Box" CornerRadius="9" BorderThickness="3" BORDER BACKGROUND />
            <FontIcon x:Name="PART_Glyph" FontFamily="{ThemeResource SymbolThemeFontFamily}" FontSize="36" Glyph="&#xE73E;" GLYPH Visibility="Collapsed" />
            <VisualStateManager.VisualStateGroups>
              <VisualStateGroup x:Name="CheckStates">
                <VisualState x:Name="Unchecked" />
                <VisualState x:Name="Checked">
                  <VisualState.Setters>
                    CHECKED
                    <Setter Target="PART_Glyph.Visibility" Value="Visible" />
                  </VisualState.Setters>
                </VisualState>
                <VisualState x:Name="Indeterminate">
                  <VisualState.Setters>
                    CHECKED
                    <Setter Target="PART_Glyph.Glyph" Value="&#xE738;" />
                    <Setter Target="PART_Glyph.Visibility" Value="Visible" />
                  </VisualState.Setters>
                </VisualState>
              </VisualStateGroup>
              CONTRAST
            </VisualStateManager.VisualStateGroups>
          </Grid>
        </ControlTemplate>
        """;

    private const string StatesBorder = "BorderBrush=\"{ThemeResource ControlStrongStrokeColorDefaultBrush}\"";
    private const string StatesBackground = "Background=\"{ThemeResource ControlAltFillColorSecondaryBrush}\"";
    private const string StatesGlyph = "Foreground=\"{ThemeResource TextOnAccentFillColorPrimaryBrush}\"";
    private const string StatesChecked = """
        <Setter Target="PART_Box.Background" Value="{ThemeResource AccentFillColorDefaultBrush}" />
                            <Setter Target="PART_Box.BorderBrush" Value="{ThemeResource AccentFillColorDefaultBrush}" />
        """;

    // When high contrast is on, the box takes the system's colours for a chosen state: the highlight, its text, and the window's text for the edge.
    private const string StatesContrast = """
        <VisualStateGroup x:Name="ContrastStates">
                <VisualState x:Name="ContrastOff" />
                <VisualState x:Name="ContrastOn">
                  <VisualState.Setters>
                    <Setter Target="PART_Box.Background" Value="{ThemeResource SystemColorHighlightColorBrush}" />
                    <Setter Target="PART_Box.BorderBrush" Value="{ThemeResource SystemColorWindowTextColorBrush}" />
                    <Setter Target="PART_Glyph.Foreground" Value="{ThemeResource SystemColorHighlightTextColorBrush}" />
                  </VisualState.Setters>
                </VisualState>
              </VisualStateGroup>
        """;

    private const string DictionaryBorder = "BorderBrush=\"{ThemeResource DataTableCheckBoxBorder}\"";
    private const string DictionaryBackground = "Background=\"{ThemeResource DataTableCheckBoxFill}\"";
    private const string DictionaryGlyph = "Foreground=\"{ThemeResource DataTableCheckBoxGlyph}\"";
    private const string DictionaryChecked = """
        <Setter Target="PART_Box.Background" Value="{ThemeResource DataTableCheckBoxFillChecked}" />
                            <Setter Target="PART_Box.BorderBrush" Value="{ThemeResource DataTableCheckBoxBorderChecked}" />
        """;

    internal static async Task RunAsync(MainWindow window, string way)
    {
        Report.Line("== contrast " + way);
        bool contrast = false;
        try
        {
            contrast = new Windows.UI.ViewManagement.AccessibilitySettings().HighContrast;
        }
        catch (Exception problem)
        {
            Report.Line("[contrast] high contrast could not be read: " + problem.Message);
        }

        string xaml = way == "dictionary"
            ? Head.Replace("BORDER", DictionaryBorder).Replace("BACKGROUND", DictionaryBackground).Replace("GLYPH", DictionaryGlyph).Replace("CHECKED", DictionaryChecked).Replace("CONTRAST", string.Empty)
            : Head.Replace("BORDER", StatesBorder).Replace("BACKGROUND", StatesBackground).Replace("GLYPH", StatesGlyph).Replace("CHECKED", StatesChecked).Replace("CONTRAST", StatesContrast);

        ElementTheme[] themes = contrast ? [ElementTheme.Default] : [ElementTheme.Light, ElementTheme.Dark];
        foreach (ElementTheme theme in themes)
        {
            window.ThemeHost.RequestedTheme = theme;
            string name = contrast ? "contrast" : theme.ToString().ToLowerInvariant();
            try
            {
                StackPanel row = new() { Orientation = Orientation.Horizontal, Spacing = 40, Margin = new Thickness(40), HorizontalAlignment = HorizontalAlignment.Left, VerticalAlignment = VerticalAlignment.Top };
                if (way == "dictionary")
                {
                    ResourceDictionary brushes = new() { Source = new Uri("ms-appx:///Navree.Controls/Themes/DataTable.Resources.xaml") };
                    row.Resources.MergedDictionaries.Add(brushes);
                    Report.Line($"[contrast] {way}: the dictionary loaded, with {brushes.ThemeDictionaries.Count} theme dictionaries");
                }

                ControlTemplate template = (ControlTemplate)XamlReader.Load(xaml);
                foreach ((string state, string label) in new[] { ("Unchecked", "not chosen"), ("Checked", "chosen"), ("Indeterminate", "some chosen") })
                {
                    ContentControl box = new() { Template = template, IsTabStop = false };
                    StackPanel one = new() { Spacing = 12 };
                    one.Children.Add(box);
                    one.Children.Add(new TextBlock { Text = label, HorizontalAlignment = HorizontalAlignment.Center });
                    row.Children.Add(one);
                    box.Loaded += (_, _) =>
                    {
                        bool a = VisualStateManager.GoToState(box, state, false);
                        bool b = way == "dictionary" || VisualStateManager.GoToState(box, contrast && state != "Unchecked" ? "ContrastOn" : "ContrastOff", false);
                        Report.Line($"[contrast] {way} {name}: {label}: state {state} {a}, contrast state {b}");
                    };
                }

                window.StageHost.Child = row;
                await Task.Delay(1200);
                await Handshake.StepAsync($"contrast-{way}-{name}", $"the box that chooses a row, by {way}, {name}");
            }
            catch (Exception problem)
            {
                Report.Line($"[contrast] {way} {name}: could not be drawn: {problem}");
            }
        }

        window.StageHost.Child = null;
    }
}
