using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Navree.Design;
using Navree.Protocol;
using Navree.Runtime;

namespace Navree.Shell;

/// <summary>
/// What is shown over the window: the question a control asked before it acts, a screen presented as a sheet, a
/// dialog or a sheet of the page itself. One at a time, as a <see cref="ContentDialog"/> is; the core says which, and
/// when the person closes one the core is told. A dialog the core took away is closed without telling it again.
/// </summary>
internal sealed class Dialogs(ShellHost host, FrameworkElement root)
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

    private Wanted? Decide(ViewState state, RenderContext? context, bool modalInFront)
    {
        ScreenView? front = state.FrontScreen;
        if (front?.Question is ScreenQuestion question)
        {
            return new Wanted(
                "question:" + front.Id + ":" + question.Id,
                () =>
                {
                    ContentDialog ask = new()
                    {
                        Title = question.Title,
                        Content = question.Text is null ? null : Look.Label(question.Text),
                        PrimaryButtonText = question.Confirm,
                        CloseButtonText = question.Cancel ?? "Cancel",
                        DefaultButton = ContentDialogButton.Close,
                    };
                    return ask;
                },
                null,
                result => host.Send(Events.Answer(front.Id, question.Id, result == ContentDialogResult.Primary)));
        }

        if (modalInFront && front is not null)
        {
            long id = front.Id;
            ScreenPresenter presenter = new(host);
            return new Wanted(
                "screen:" + id,
                () =>
                {
                    presenter.Show(front, WindowRules.ClassOf(root.ActualWidth));
                    return new ContentDialog { Title = front.Screen?.Title ?? front.Title ?? string.Empty, Content = presenter, CloseButtonText = "Close", MinWidth = 520 };
                },
                () => presenter.Show(host.State.Screens.TryGetValue(id, out ScreenView? held) ? held : front, WindowRules.ClassOf(root.ActualWidth)),
                _ => host.Send(Events.Dismissed(id)));
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
                return new Wanted(
                    "overlay:" + screen.Id + ":" + overlay.Id,
                    () => OverlayDialog(overlay, context),
                    null,
                    _ => host.Send(Events.Overlay(screen.Id, overlay.Id, false)));
            }
        }

        return null;
    }

    private static ContentDialog OverlayDialog(OverlayRequest overlay, RenderContext context)
    {
        NodeRenderer renderer = new(context);
        StackPanel body = new() { Spacing = 12 };
        string? title = null;
        switch (overlay.Node)
        {
            case Models.Sheet sheet:
                title = sheet.Title;
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
            case Models.Dialog dialog:
                title = dialog.Title;
                if (dialog.Text is not null)
                {
                    body.Children.Add(Look.Label(dialog.Text));
                }

                foreach (Models.Node child in dialog.Children)
                {
                    body.Children.Add(renderer.RenderInOverlay(child, overlay.Id));
                }

                StackPanel actions = new() { Orientation = Orientation.Horizontal, Spacing = 8, HorizontalAlignment = HorizontalAlignment.Right };
                foreach (Models.Node action in dialog.Actions)
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

        return new ContentDialog
        {
            Title = title ?? string.Empty,
            Content = new ScrollViewer { Content = body },
            CloseButtonText = "Close",
            MinWidth = 480,
        };
    }

    // One dialog is on screen at a time: what is wanted replaces what is showing, and the latest wish wins.
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

                if (want is null)
                {
                    return;
                }

                ContentDialog next = want.Build();
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

    private sealed record Wanted(string Key, Func<ContentDialog> Build, Action? Refresh, Action<ContentDialogResult> OnClosed);
}
