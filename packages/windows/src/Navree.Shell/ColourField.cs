using Microsoft.UI;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using Navree.Design.Inputs;
using Navree.Runtime;
using Windows.UI;

namespace Navree.Shell;

/// <summary>
/// A colour field: a field-shaped button that shows a swatch of the colour and its value in words, and opens a flyout that holds the system's own colour
/// picker with OK and Cancel (the way the platform's guidance has a picker in a flyout commit: a change of the spectrum alone does not choose). What
/// is sent to the core is <c>#rrggbb</c>, as the protocol says. The colour is told by its value and its name as well as by the swatch.
/// </summary>
internal sealed class ColourFieldView
{
    private readonly string label;
    private readonly Func<bool> enabled;
    private readonly Action<FieldValue> send;
    private readonly Button button;
    private readonly Border swatch;
    private readonly TextBlock words;
    private readonly Flyout flyout;
    private readonly ColorPicker picker;
    private string value;
    private bool open;

    internal ColourFieldView(string header, string label, string value, Func<bool> enabled, Action<FieldValue> send)
    {
        this.label = label;
        this.value = ColourRules.Normalise(value) ?? string.Empty;
        this.enabled = enabled;
        this.send = send;

        swatch = new Border
        {
            Width = 24,
            Height = 24,
            CornerRadius = new CornerRadius(4),
            BorderThickness = new Thickness(1),
            BorderBrush = Look.Resource("ControlStrongStrokeDefaultBrush"),
            VerticalAlignment = VerticalAlignment.Center,
        };
        words = Look.Label(string.Empty, "BodyTextBlockStyle", keepEnd: false);
        words.VerticalAlignment = VerticalAlignment.Center;
        Grid content = new() { ColumnSpacing = 10 };
        content.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        content.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        Grid.SetColumn(words, 1);
        content.Children.Add(swatch);
        content.Children.Add(words);

        // The platform's own colour picker, as it is: its spectrum, its slider and its hex box, with the channel boxes behind its More button.
        picker = new ColorPicker { IsHexInputVisible = true, IsAlphaEnabled = false };
        Button ok = new() { Content = "OK", HorizontalAlignment = HorizontalAlignment.Stretch, Margin = new Thickness(0, 12, 2, 0), Style = Look.Ramp("AccentButtonStyle") };
        Button cancel = new() { Content = "Cancel", HorizontalAlignment = HorizontalAlignment.Stretch, Margin = new Thickness(2, 12, 0, 0) };
        Grid buttons = new();
        buttons.ColumnDefinitions.Add(new ColumnDefinition());
        buttons.ColumnDefinitions.Add(new ColumnDefinition());
        Grid.SetColumn(cancel, 1);
        buttons.Children.Add(ok);
        buttons.Children.Add(cancel);
        StackPanel panel = new();
        panel.Children.Add(picker);
        panel.Children.Add(buttons);
        flyout = new Flyout { Content = panel, Placement = Microsoft.UI.Xaml.Controls.Primitives.FlyoutPlacementMode.Bottom };

        button = new Button
        {
            Content = content,
            HorizontalAlignment = HorizontalAlignment.Stretch,
            HorizontalContentAlignment = HorizontalAlignment.Stretch,
            MinHeight = 32,
            Flyout = flyout,
        };
        AutomationProperties.SetHelpText(button, "Opens the colour picker");

        flyout.Opening += (_, _) =>
        {
            open = true;
            picker.Color = ColourRules.TryRead(this.value, out byte r, out byte g, out byte b) ? Color.FromArgb(255, r, g, b) : Accent();
        };
        flyout.Opened += (_, _) => picker.Focus(FocusState.Keyboard);
        flyout.Closed += (_, _) => open = false;
        ok.Click += (_, _) =>
        {
            Color chosen = picker.Color;
            this.value = ColourRules.Hex(chosen.R, chosen.G, chosen.B);
            flyout.Hide();
            Draw();
            this.send(new TextValue(this.value));
        };
        cancel.Click += (_, _) => flyout.Hide();

        Root = new StackPanel { Spacing = 4 };
        if (header.Length > 0)
        {
            Root.Children.Add(Look.Label(header, "BodyTextBlockStyle"));
        }

        Root.Children.Add(button);
        Draw();
    }

    /// <summary>What the field is drawn as.</summary>
    internal StackPanel Root { get; }

    /// <summary>The button that opens the picker.</summary>
    internal Button PickerButton => button;

    /// <summary>The picker inside the flyout.</summary>
    internal ColorPicker Picker => picker;

    /// <summary>The flyout the picker is in.</summary>
    internal Flyout PickerFlyout => flyout;

    /// <summary>The colour the field holds, as the protocol writes it, or empty.</summary>
    internal string Value => value;

    /// <summary>Shows what the core holds for the field.</summary>
    /// <param name="view">What the screen holds for the field.</param>
    internal void Show(FieldView? view)
    {
        if (view?.Value is TextValue echoed && !open)
        {
            value = ColourRules.Normalise(echoed.Text) ?? string.Empty;
        }

        Draw();
    }

    private static Color Accent() =>
        Application.Current.Resources.TryGetValue("SystemAccentColor", out object? found) && found is Color accent ? accent : Color.FromArgb(255, 0, 120, 212);

    private void Draw()
    {
        button.IsEnabled = enabled();
        words.Text = ColourRules.Shown(value);
        swatch.Background = ColourRules.TryRead(value, out byte r, out byte g, out byte b) ? new SolidColorBrush(Color.FromArgb(255, r, g, b)) : null;
        AutomationProperties.SetName(button, ColourRules.Spoken(label, value));
    }
}
