using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Navree.Design;
using Navree.Protocol;
using Navree.Runtime;

namespace Navree.Shell;

/// <summary>
/// What is shown over the window: the question a control asked before it acts (always a dialog, which is what a question
/// is), and a screen presented as a sheet or a sheet of the page itself, which is a pane at the side when the window is
/// wide and what it holds is more than a question, and a dialog otherwise. One at a time; the core says which, and when
/// the person closes one the core is told. What the core took away is closed without telling it again.
/// </summary>
internal sealed class Dialogs(ShellHost host, FrameworkElement root, SidePane pane)
{
    private Wanted? latest;
    private Wanted? showing;
    private ContentDialog? dialog;
    private TaskCompletionSource closing = new();
    private bool closedByUs;
    private bool pumping;

    /// <summary>What is over the window now: a question, a modal screen, or an overlay of the screen in front.</summary>
    internal void Update(ViewState state, RenderContext? context, bool modalInFront)
    {
        latest = Decide(state, context, modalInFront);
        if (!pumping)
        {
            _ = PumpAsync();
        }
    }

    private WidthClass Width => WindowRules.ClassOf(root.ActualWidth);

    private Wanted? Decide(ViewState state, RenderContext? context, bool modalInFront)
    {
        ScreenView? front = state.FrontScreen;
        if (front?.Question is ScreenQuestion question)
        {
            return new Wanted(
                "question:" + front.Id + ":" + question.Id,
                () => new ContentDialog
                {
                    Title = question.Title,
                    Content = question.Text is null ? null : Look.Label(question.Text),
                    PrimaryButtonText = question.Confirm,
                    CloseButtonText = question.Cancel ?? "Cancel",
                    DefaultButton = ContentDialogButton.Close,
                },
                null,
                null,
                result => host.Send(Events.Answer(front.Id, question.Id, result == ContentDialogResult.Primary)));
        }

        if (modalInFront && front is not null)
        {
            long id = front.Id;
            ScreenPresenter presenter = new(host);
            string heading = front.Screen?.Title ?? front.Title ?? string.Empty;
            Models.Present present = state.Nav.Modals.FirstOrDefault(modal => modal.Stack.Contains(id))?.Present ?? Models.Present.Sheet;
            return new Wanted(
                "screen:" + id,
                () => new ContentDialog { Title = heading, Content = presenter, CloseButtonText = "Close", MinWidth = 520 },
                new PaneBody(heading, () =>
                {
                    presenter.Show(front, Width);
                    return presenter;
                }),
                () => presenter.Show(host.State.Screens.TryGetValue(id, out ScreenView? held) ? held : front, Width),
                _ => host.Send(Events.Dismissed(id)),
                large: present != Models.Present.Full);
        }

        if (context is not null)
        {
            ScreenView screen = context.Screen;
            foreach (OverlayRequest request in context.Overlays.AsEnumerable().Reverse())
            {
                if (!screen.IsOpen(request.Id))
                {
                    continue;
                }

                OverlayRequest overlay = request;
                bool large = overlay.Node is Models.Sheet { Size: Models.SheetSize.Large };
                return new Wanted(
                    "overlay:" + screen.Id + ":" + overlay.Id,
                    () => OverlayDialog(overlay, context),
                    new PaneBody(Titled(overlay), () => OverlayBody(overlay, context)),
                    null,
                    _ => host.Send(Events.Overlay(screen.Id, overlay.Id, false)),
                    large: large);
            }
        }

        return null;
    }

    private static string Titled(OverlayRequest overlay) =>
        overlay.Node switch
        {
            Models.Sheet sheet => sheet.Title ?? string.Empty,
            Models.Dialog dialog => dialog.Title ?? string.Empty,
            _ => string.Empty,
        };

    private static ContentDialog OverlayDialog(OverlayRequest overlay, RenderContext context) =>
        new()
        {
            Title = Titled(overlay),
            Content = new ScrollViewer { Content = OverlayBody(overlay, context) },
            CloseButtonText = "Close",
            MinWidth = 480,
        };

    private static UIElement OverlayBody(OverlayRequest overlay, RenderContext context)
    {
        NodeRenderer renderer = new(context);
        StackPanel body = new() { Spacing = 12 };
        switch (overlay.Node)
        {
            case Models.Sheet sheet:
                if (sheet.Subtitle is not null)
                {
                    body.Children.Add(Look.Label(sheet.Subtitle, muted: true));
                }

                foreach (Models.Node child in sheet.Children)
                {
                    body.Children.Add(renderer.RenderInOverlay(child, overlay.Id));
                }

                foreach (Models.Node action in sheet.Actions)
                {
                    body.Children.Add(renderer.RenderInOverlay(action, overlay.Id));
                }

                break;
            case Models.Dialog inner:
                if (inner.Text is not null)
                {
                    body.Children.Add(Look.Label(inner.Text));
                }

                foreach (Models.Node child in inner.Children)
                {
                    body.Children.Add(renderer.RenderInOverlay(child, overlay.Id));
                }

                StackPanel actions = new() { Orientation = Orientation.Horizontal, Spacing = 8, HorizontalAlignment = HorizontalAlignment.Right };
                foreach (Models.Node action in inner.Actions)
                {
                    actions.Children.Add(renderer.RenderInOverlay(action, overlay.Id));
                }

                body.Children.Add(actions);
                break;
            case Models.Popover popover:
                foreach (Models.Node child in popover.Children)
                {
                    body.Children.Add(renderer.RenderInOverlay(child, overlay.Id));
                }

                break;
            default:
                break;
        }

        return body;
    }

    // One thing is over the window at a time: what is wanted replaces what is showing, and the latest wish wins.
    private async Task PumpAsync()
    {
        pumping = true;
        try
        {
            while (true)
            {
                Wanted? want = latest;
                if (want is not null && showing is not null && want.Key == showing.Key)
                {
                    want.Refresh?.Invoke();
                    return;
                }

                if (dialog is not null)
                {
                    closedByUs = true;
                    dialog.Hide();
                    await closing.Task;
                    continue;
                }

                if (pane.IsShowing)
                {
                    pane.Hide();
                    showing = null;
                    continue;
                }

                if (want is null)
                {
                    return;
                }

                if (want.Pane is PaneBody body && WindowRules.UsesSidePane(Width, want.Large))
                {
                    showing = want;
                    pane.Show(body.Title, body.Build(), () =>
                    {
                        showing = null;
                        want.OnClosed(ContentDialogResult.None);
                    });
                    return;
                }

                ContentDialog next = want.Dialog();
                next.XamlRoot = root.XamlRoot;
                dialog = next;
                showing = want;
                closing = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
                closedByUs = false;
                _ = ShowAsync(next, want);
                return;
            }
        }
        finally
        {
            pumping = false;
        }
    }

    private async Task ShowAsync(ContentDialog shown, Wanted want)
    {
        ContentDialogResult result = await shown.ShowAsync();
        bool byUs = closedByUs;
        dialog = null;
        showing = null;
        closedByUs = false;
        closing.TrySetResult();
        if (!byUs)
        {
            want.OnClosed(result);
        }
    }

    private sealed record PaneBody(string Title, Func<UIElement> Build);

    private sealed record Wanted(string Key, Func<ContentDialog> Dialog, PaneBody? Pane, Action? Refresh, Action<ContentDialogResult> OnClosed, bool large = false)
    {
        internal bool Large { get; } = large;
    }
}
