using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Navree.Design.Tables;

namespace Navree.Controls;

/// <summary>The box that chooses a row, drawn from the table's own brushes.</summary>
public sealed class DataTableCheck : Control
{
    private static Windows.UI.ViewManagement.AccessibilitySettings? access;
    private HeaderBoxState state;
    private bool available = true;

    /// <summary>Makes a box.</summary>
    public DataTableCheck()
    {
        DefaultStyleKey = typeof(DataTableCheck);
        Microsoft.UI.Xaml.Automation.AutomationProperties.SetAccessibilityView(this, Microsoft.UI.Xaml.Automation.Peers.AccessibilityView.Raw);
    }

    /// <summary>Whether none, every one or some of the rows it stands for are chosen.</summary>
    public HeaderBoxState State
    {
        get => state;
        set
        {
            state = value;
            Update();
        }
    }

    /// <summary>Whether the box works; a box that does not is drawn faint.</summary>
    public bool Available
    {
        get => available;
        set
        {
            available = value;
            Update();
        }
    }

    /// <inheritdoc />
    protected override void OnApplyTemplate()
    {
        base.OnApplyTemplate();
        Update();
    }

    private void Update()
    {
        string check = state switch
        {
            HeaderBoxState.On => "Checked",
            HeaderBoxState.Indeterminate => "Indeterminate",
            _ => "Unchecked",
        };
        VisualStateManager.GoToState(this, check, false);
        VisualStateManager.GoToState(this, available ? "Enabled" : "Unavailable", false);
        VisualStateManager.GoToState(this, HighContrast && state != HeaderBoxState.Off ? "ContrastOn" : "ContrastOff", false);
    }

    // Whether the system's high contrast colours are on: read each time, as a person may turn them on while the table is open.
    private static bool HighContrast
    {
        get
        {
            try
            {
                access ??= new Windows.UI.ViewManagement.AccessibilitySettings();
                return access.HighContrast;
            }
            catch (Exception)
            {
                return false;
            }
        }
    }
}

/// <summary>A small glyph that can be pressed: the chevron that opens a detail, the dots of a menu.</summary>
public sealed class DataTableGlyph : Control
{
    /// <summary>The glyph, a character of the symbol font.</summary>
    public static readonly DependencyProperty GlyphProperty = DependencyProperty.Register(
        nameof(Glyph),
        typeof(string),
        typeof(DataTableGlyph),
        new PropertyMetadata(string.Empty));

    private bool over;
    private bool pressed;

    /// <summary>Makes a glyph.</summary>
    public DataTableGlyph()
    {
        DefaultStyleKey = typeof(DataTableGlyph);
        Microsoft.UI.Xaml.Automation.AutomationProperties.SetAccessibilityView(this, Microsoft.UI.Xaml.Automation.Peers.AccessibilityView.Raw);
    }

    /// <summary>The glyph, a character of the symbol font.</summary>
    public string Glyph
    {
        get => (string)GetValue(GlyphProperty);
        set => SetValue(GlyphProperty, value);
    }

    /// <inheritdoc />
    protected override void OnApplyTemplate()
    {
        base.OnApplyTemplate();
        UpdateState();
    }

    /// <inheritdoc />
    protected override void OnPointerEntered(Microsoft.UI.Xaml.Input.PointerRoutedEventArgs e)
    {
        base.OnPointerEntered(e);
        over = e.Pointer.PointerDeviceType != Microsoft.UI.Input.PointerDeviceType.Touch;
        UpdateState();
    }

    /// <inheritdoc />
    protected override void OnPointerExited(Microsoft.UI.Xaml.Input.PointerRoutedEventArgs e)
    {
        base.OnPointerExited(e);
        over = false;
        pressed = false;
        UpdateState();
    }

    /// <inheritdoc />
    protected override void OnPointerPressed(Microsoft.UI.Xaml.Input.PointerRoutedEventArgs e)
    {
        base.OnPointerPressed(e);
        pressed = true;
        UpdateState();
    }

    /// <inheritdoc />
    protected override void OnPointerReleased(Microsoft.UI.Xaml.Input.PointerRoutedEventArgs e)
    {
        base.OnPointerReleased(e);
        pressed = false;
        UpdateState();
    }

    private void UpdateState() =>
        VisualStateManager.GoToState(this, pressed ? "Pressed" : over ? "PointerOver" : "Normal", false);
}
