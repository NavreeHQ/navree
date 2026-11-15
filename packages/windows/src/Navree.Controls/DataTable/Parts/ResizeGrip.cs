using Microsoft.UI.Input;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;

namespace Navree.Controls;

/// <summary>The strip at the end of a heading that a pointer drags to change the column's width.</summary>
public sealed class ResizeGrip : Control
{
    private bool dragging;
    private bool over;
    private double startX;

    /// <summary>Makes a grip.</summary>
    public ResizeGrip()
    {
        DefaultStyleKey = typeof(ResizeGrip);
        ProtectedCursor = InputSystemCursor.Create(InputSystemCursorShape.SizeWestEast);
        Microsoft.UI.Xaml.Automation.AutomationProperties.SetAccessibilityView(this, Microsoft.UI.Xaml.Automation.Peers.AccessibilityView.Raw);
    }

    /// <summary>Raised when a drag begins.</summary>
    internal event Action? Started;

    /// <summary>Raised as the pointer moves, with how far it is from where it began, in the window's coordinates.</summary>
    internal event Action<double>? Moved;

    /// <summary>Raised when the drag ends: true when it is kept, false when it was cancelled.</summary>
    internal event Action<bool>? Ended;

    /// <summary>Raised on a double tap, which fits the column to what it holds.</summary>
    internal event Action? FitRequested;

    /// <inheritdoc />
    protected override void OnApplyTemplate()
    {
        base.OnApplyTemplate();
        UpdateState();
    }

    /// <inheritdoc />
    protected override void OnPointerEntered(PointerRoutedEventArgs e)
    {
        base.OnPointerEntered(e);
        over = true;
        UpdateState();
    }

    /// <inheritdoc />
    protected override void OnPointerExited(PointerRoutedEventArgs e)
    {
        base.OnPointerExited(e);
        over = false;
        UpdateState();
    }

    /// <inheritdoc />
    protected override void OnPointerPressed(PointerRoutedEventArgs e)
    {
        base.OnPointerPressed(e);
        Microsoft.UI.Input.PointerPoint point = e.GetCurrentPoint(null);
        if (e.Pointer.PointerDeviceType == PointerDeviceType.Mouse && !point.Properties.IsLeftButtonPressed)
        {
            return;
        }

        if (CapturePointer(e.Pointer))
        {
            dragging = true;
            startX = point.Position.X;
            UpdateState();
            Started?.Invoke();
        }

        e.Handled = true;
    }

    /// <inheritdoc />
    protected override void OnPointerMoved(PointerRoutedEventArgs e)
    {
        base.OnPointerMoved(e);
        if (dragging)
        {
            Moved?.Invoke(e.GetCurrentPoint(null).Position.X - startX);
            e.Handled = true;
        }
    }

    /// <inheritdoc />
    protected override void OnPointerReleased(PointerRoutedEventArgs e)
    {
        base.OnPointerReleased(e);
        if (dragging)
        {
            dragging = false;
            ReleasePointerCapture(e.Pointer);
            UpdateState();
            Ended?.Invoke(true);
            e.Handled = true;
        }
    }

    /// <inheritdoc />
    protected override void OnPointerCaptureLost(PointerRoutedEventArgs e)
    {
        base.OnPointerCaptureLost(e);
        if (dragging)
        {
            dragging = false;
            UpdateState();
            Ended?.Invoke(false);
        }
    }

    /// <inheritdoc />
    protected override void OnDoubleTapped(DoubleTappedRoutedEventArgs e)
    {
        base.OnDoubleTapped(e);
        FitRequested?.Invoke();
        e.Handled = true;
    }

    /// <inheritdoc />
    protected override void OnTapped(TappedRoutedEventArgs e)
    {
        base.OnTapped(e);
        e.Handled = true;
    }

    private void UpdateState() =>
        VisualStateManager.GoToState(this, dragging ? "Pressed" : over ? "PointerOver" : "Normal", false);
}
