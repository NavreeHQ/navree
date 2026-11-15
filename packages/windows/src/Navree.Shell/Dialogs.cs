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
    private readonly Dictionary<(long Screen, bool Pane), ScreenPresenter> modals = [];
    private ContentDialog? dialog;
    private Flyout? flyout;
    private readonly HashSet<Flyout> closingByUs = [];
    private FrameworkElement? flyoutAnchor;
    private bool repositioning;
    private bool closedByEscape;
    private TaskCompletionSource closing = new();
    private bool closedByUs;
    private bool pumping;
    private bool hooked;

    // Escape takes away a popover that is open, wherever the focus is: in the popover or still on the button that opened it.
    private void CloseFlyoutOnEscape(object sender, Microsoft.UI.Xaml.Input.KeyRoutedEventArgs e)
    {
        if (e.Key == Windows.System.VirtualKey.Escape && flyout is { IsOpen: true } open)
        {
            e.Handled = true;
            closedByEscape = true;
            open.Hide();
        }
    }

    /// <summary>What is over the window now: a question, a modal screen, or an overlay of the screen in front.</summary>
    internal void Update(ViewState state, RenderContext? context, bool modalInFront)
    {
        foreach ((long Screen, bool Pane) gone in modals.Keys.Where(key => !state.Screens.ContainsKey(key.Screen)).ToList())
        {
            _ = modals.Remove(gone);
        }

        if (!hooked)
        {
            hooked = true;
            root.AddHandler(UIElement.KeyDownEvent, new Microsoft.UI.Xaml.Input.KeyEventHandler(CloseFlyoutOnEscape), true);
        }

        if (context is not null)
        {
            // The core's request for the focus may name a part of what is open over the page; the bodies that are no longer open are let go of.
            context.OverlayRoots = Roots;
            OverlayBodyView? open = showing?.Holder?.View;
            _ = context.Extra.RemoveAll(record => open is null || !ReferenceEquals(record, open.Owner));
        }

        latest = Decide(state, context, modalInFront);
        if (!pumping)
        {
            _ = PumpAsync();
        }
    }

    // What holds the sheet, dialog or popover that is open.
    private IEnumerable<FrameworkElement> Roots()
    {
        if (flyout?.Content is FrameworkElement inFlyout)
        {
            yield return inFlyout;
        }

        if (dialog?.Content is FrameworkElement inDialog)
        {
            yield return inDialog;
        }

        if (pane.IsShowing)
        {
            yield return pane;
        }
    }

    /// <summary>What is over the window now, by name; null when nothing is.</summary>
    internal string? ShowingKey => showing?.Key;

    /// <summary>The screen a window of one screen shows, or none for the main window, which follows the screen in front.</summary>
    internal long? Own { get; init; }

    private WidthClass Width => WindowRules.ClassOf(root.ActualWidth);

    // A window of one screen has no pane at its side: what would be a pane is a dialog there.
    private bool AsPane(bool large) => Own is null && WindowRules.UsesSidePane(root.ActualWidth, large);

    private Wanted? Decide(ViewState state, RenderContext? context, bool modalInFront)
    {
        // The main window shows the questions and sheets of the screen in front; a window of one screen shows those of its own screen (unless the
        // main window is showing that same screen, which would be two dialogs for one question).
        ScreenView? front = Own is long own
            ? (state.FrontScreen?.Id != own && state.Screens.TryGetValue(own, out ScreenView? ownScreen) ? ownScreen : null)
            : state.FrontScreen;
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
            string heading = front.Screen?.Title ?? front.Title ?? string.Empty;
            Models.Present present = state.Nav.Modals.FirstOrDefault(modal => modal.Stack.Contains(id))?.Present ?? Models.Present.Sheet;
            bool large = present != Models.Present.Full;

            // One presenter for each screen and each way of showing it, kept while the screen is held: its later states are shown in the
            // one that is on the screen, and a dialog and a pane never hold the same element.
            ScreenPresenter PresenterOf(bool pane)
            {
                if (!modals.TryGetValue((id, pane), out ScreenPresenter? held))
                {
                    held = new ScreenPresenter(host);
                    modals[(id, pane)] = held;
                }

                held.Show(host.State.Screens.TryGetValue(id, out ScreenView? latestScreen) ? latestScreen : front, Width);
                return held;
            }

            return new Wanted(
                "screen:" + id + (AsPane(large) ? ":pane" : ":dialog"),
                () => new ContentDialog { Title = heading, Content = PresenterOf(false), CloseButtonText = "Close", MinWidth = 520 },
                new PaneBody(heading, () => PresenterOf(true)),
                () =>
                {
                    foreach (bool pane in new[] { false, true })
                    {
                        if (modals.ContainsKey((id, pane)))
                        {
                            _ = PresenterOf(pane);
                        }
                    }
                },
                _ => host.Send(Events.Dismissed(id)),
                large: large);
        }

        if (context is not null)
        {
            ScreenView screen = context.Screen;
            // The core keeps the order they were opened in (the last opened last): the one opened last is the one shown.
            List<string> opened = [.. screen.Overlays];
            foreach (OverlayRequest request in context.Overlays.OrderByDescending(candidate => opened.IndexOf(candidate.Id)))
            {
                if (!screen.IsOpen(request.Id))
                {
                    continue;
                }

                OverlayRequest overlay = request;
                bool large = overlay.Node is Models.Sheet { Size: Models.SheetSize.Large };

                // The body is drawn when what is wanted is shown, and kept: later answers bring it up to date where it stands.
                OverlayHolder holder = new();
                UIElement Body()
                {
                    holder.View = new OverlayBodyView(context, overlay);
                    return holder.View.Element;
                }

                // A popover opens at the control that opened it, where that control is found again on the screen.
                FrameworkElement? anchor = overlay.Node is Models.Popover && Triggers.Last is string key && context.Triggers.TryGetValue(key, out FrameworkElement? found) ? found : null;
                if (anchor is not null)
                {
                    return new Wanted(
                        "overlay:" + screen.Id + ":" + overlay.Id + ":flyout",
                        () => OverlayDialog(overlay, Body()),
                        null,
                        null,
                        _ => host.Send(Events.Overlay(screen.Id, overlay.Id, false)),
                        large: false)
                    {
                        Flyout = () => new Flyout { Content = new ScrollViewer { Content = Body(), MaxHeight = 420 } },
                        Anchor = anchor,
                        Rebuild = Body,
                        Context = context,
                        Holder = holder,
                        Overlay = overlay,
                    };
                }

                return new Wanted(
                    "overlay:" + screen.Id + ":" + overlay.Id + (AsPane(large) ? ":pane" : ":dialog"),
                    () => OverlayDialog(overlay, Body()),
                    new PaneBody(Titled(overlay), Body),
                    null,
                    _ => host.Send(Events.Overlay(screen.Id, overlay.Id, false)),
                    large: large)
                {
                    Rebuild = Body,
                    Context = context,
                    Holder = holder,
                    Overlay = overlay,
                };
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

    private static ContentDialog OverlayDialog(OverlayRequest overlay, UIElement body) =>
        new()
        {
            Title = Titled(overlay),
            Content = new ScrollViewer { Content = body },
            CloseButtonText = "Close",
            MinWidth = 480,
        };

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
                    if (want.Rebuild is not null)
                    {
                        // An answer changed the screen: what a sheet, dialog or popover of the page holds is brought up to date where it stands, so a
                        // server's error stands at its field, a button that is busy says so, and the one that is open stays the one that is open. Only a
                        // body that cannot be (the overlay is another kind, or the screen was drawn again) is drawn again, with the place in it put back.
                        OverlayBodyView? view = showing.Holder?.View;
                        bool kept = view is not null && ReferenceEquals(view.Context, want.Context) && want.Overlay is not null && view.Update(want.Overlay);
                        if (kept)
                        {
                            want.Holder!.View = view;
                            want.Context?.Rebuild();
                        }
                        else
                        {
                            BringUpToDate(want);
                        }

                        if (flyout is not null && want.Anchor is { XamlRoot: not null } moved && !ReferenceEquals(moved, showing.Anchor))
                        {
                            // The control it opened at was drawn again: it stays beside the new one.
                            repositioning = true;
                            try
                            {
                                flyoutAnchor = moved;
                                flyout.ShowAt(moved);
                            }
                            catch (Exception problem)
                            {
                                System.Diagnostics.Debug.WriteLine("a popover could not be moved: " + problem.Message);
                            }
                            finally
                            {
                                repositioning = false;
                            }
                        }

                        showing = want;
                    }

                    return;
                }

                if (flyout is not null)
                {
                    _ = closingByUs.Add(flyout);
                    flyout.Hide();
                    flyout = null;
                    showing = null;
                    continue;
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

                if (want.Flyout is not null && want.Anchor is { XamlRoot: null } notYet)
                {
                    // The control that opened it was drawn a moment ago and is not in the window yet: it is waited for, and the wish may change meanwhile.
                    for (int waited = 0; waited < 20 && notYet.XamlRoot is null && ReferenceEquals(latest, want); waited++)
                    {
                        await Task.Delay(50);
                    }

                    if (!ReferenceEquals(latest, want))
                    {
                        continue;
                    }
                }

                if (want.Flyout is not null && want.Anchor is { XamlRoot: not null } anchor)
                {
                    // Light dismiss (a click away, Escape) closes it and tells the core; the focus goes back to the control that opened it.
                    Flyout shown = want.Flyout();
                    if (shown.Content is FrameworkElement shownContent)
                    {
                        shownContent.FlowDirection = root.FlowDirection;
                    }

                    flyout = shown;
                    flyoutAnchor = anchor;
                    showing = want;
                    closedByEscape = false;
                    shown.Closed += (_, _) =>
                    {
                        if (repositioning)
                        {
                            return;
                        }

                        bool byUs = closingByUs.Remove(shown);
                        if (ReferenceEquals(flyout, shown))
                        {
                            flyout = null;
                            showing = null;
                        }

                        FrameworkElement openedAt = flyoutAnchor ?? anchor;
                        if (!byUs && openedAt.XamlRoot is null)
                        {
                            // It went with the control it was open at, which the page drawing again took away: it is shown again at the new one.
                            if (!pumping)
                            {
                                _ = PumpAsync();
                            }
                        }
                        else if (!byUs)
                        {
                            want.OnClosed(ContentDialogResult.None);
                            _ = openedAt.Focus(closedByEscape ? FocusState.Keyboard : FocusState.Programmatic);
                        }
                    };
                    shown.Opened += (_, _) =>
                    {
                        // The focus goes into the popover, and Escape in it takes it away.
                        if (shown.Content is FrameworkElement body)
                        {
                            body.KeyDown += CloseFlyoutOnEscape;
                            if (Microsoft.UI.Xaml.Input.FocusManager.FindFirstFocusableElement(body) is Control first)
                            {
                                _ = first.Focus(FocusState.Programmatic);
                            }
                        }
                    };
                    shown.ShowAt(anchor);
                    return;
                }

                if (want.Pane is PaneBody body && AsPane(want.Large))
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
                next.FlowDirection = root.FlowDirection;
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

    private void BringUpToDate(Wanted want)
    {
        FrameworkElement? old = flyout?.Content as FrameworkElement ?? dialog?.Content as FrameworkElement ?? (pane.IsShowing ? pane.Content as FrameworkElement : null);
        PageKeeper.Held? held = old is null ? null : PageKeeper.Capture(old);
        UIElement fresh = want.Rebuild!();
        FrameworkElement? placed = null;
        string heading = want.Overlay is { } overlay ? Titled(overlay) : string.Empty;
        if (flyout is not null)
        {
            ScrollViewer wrapped = new() { Content = fresh, MaxHeight = 420 };
            flyout.Content = wrapped;
            placed = wrapped;
        }
        else if (dialog is not null)
        {
            ScrollViewer wrapped = new() { Content = fresh };
            dialog.Content = wrapped;
            dialog.Title = heading;
            placed = wrapped;
        }
        else if (pane.IsShowing)
        {
            pane.Replace(fresh);
            pane.Retitle(heading);
            placed = fresh as FrameworkElement;
        }

        if (held is not null && placed is not null)
        {
            PageKeeper.Restore(placed, held);
        }
    }

    private async Task ShowAsync(ContentDialog shown, Wanted want)
    {
        ContentDialogResult result;
        try
        {
            result = await shown.ShowAsync();
        }
        catch (Exception problem)
        {
            // A dialog that could not be shown (another is open on the window, the window is not laid out yet) is not shown; the ones after it still are.
            System.Diagnostics.Debug.WriteLine("a dialog could not be shown: " + problem.Message);
            result = ContentDialogResult.None;
        }

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

    /// <summary>The body of an overlay that is open, once it is drawn.</summary>
    private sealed class OverlayHolder
    {
        internal OverlayBodyView? View { get; set; }
    }

    private sealed record Wanted(string Key, Func<ContentDialog> Dialog, PaneBody? Pane, Action? Refresh, Action<ContentDialogResult> OnClosed, bool large = false)
    {
        internal bool Large { get; } = large;

        /// <summary>Makes the flyout, for a popover that has the control it opens at.</summary>
        internal Func<Flyout>? Flyout { get; init; }

        /// <summary>The control a flyout opens at.</summary>
        internal FrameworkElement? Anchor { get; init; }

        /// <summary>Draws the body again from the page's tree as it is now.</summary>
        internal Func<UIElement>? Rebuild { get; init; }

        /// <summary>The page's drawing the body was drawn from.</summary>
        internal RenderContext? Context { get; init; }

        /// <summary>The body of the overlay, when it has been drawn.</summary>
        internal OverlayHolder? Holder { get; init; }

        /// <summary>The overlay, as the tree has it.</summary>
        internal OverlayRequest? Overlay { get; init; }
    }
}

/// <summary>The key of the button pressed last, so a popover it opens is shown at it even when the screen was drawn again for the opening.</summary>
internal static class Triggers
{
    /// <summary>The key, or none.</summary>
    internal static string? Last { get; set; }
}
