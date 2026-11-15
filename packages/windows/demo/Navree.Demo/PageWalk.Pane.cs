using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
using Navree.Runtime;

namespace Navree.Demo;

// A page opened beside the one that is open (a sheet in a wide window is a pane at the side) is waited for: while it is on its way the pane says so,
// in the words and with the indication a page that is loading gives, and when the page comes the indication goes and the page is in the pane.
internal sealed partial class PageWalk
{
    private FrameworkElement? SidePaneNow() =>
        InTree<FrameworkElement>().FirstOrDefault(element => element.GetType().Name == "SidePane" && element.Visibility == Visibility.Visible && element.ActualWidth > 0);

    private async Task PaneAsync()
    {
        Say("== a pane at the side that waits for its page");
        window.Size(1280, 900);
        host.Send(Events.Select(0));
        await SettleAsync();
        await Task.Delay(1500);
        Button? open = ButtonStarting("A slow order");
        Check("the walk's own screen has the button that opens the slow order", open is not null, open is null ? "none" : "found");
        if (open is null)
        {
            return;
        }

        Foreground();
        _ = open.Focus(FocusState.Keyboard);
        await Task.Delay(300);
        await PressAsync(EnterKey);
        bool shown = false;
        bool indicated = false;
        string said = string.Empty;
        for (int waited = 0; waited < 2400; waited += 200)
        {
            await Task.Delay(200);
            FrameworkElement? pane = SidePaneNow();
            if (pane is null)
            {
                continue;
            }

            shown = true;
            bool bar = Below(pane).OfType<ProgressBar>().Any(progress => progress.IsIndeterminate && progress.ActualWidth > 0);
            bool ring = Below(pane).OfType<ProgressRing>().Any(progress => progress.IsActive && progress.ActualWidth > 0);
            indicated |= bar || ring;
            said = "bar " + bar + ", ring " + ring + ", words: " + string.Join(" | ", Below(pane).OfType<TextBlock>().Where(block => block.ActualWidth > 0).Select(block => block.Text));
        }

        Check("the page that is waited for opens in a pane at the side", shown, shown ? "the pane is showing" : "no pane; the front screen is '" + host.State.FrontScreen?.Title + "'");
        Check("the pane shows that it is waiting for its page", indicated, said);
        await ShotAsync("pane-waiting");
        await Task.Delay(2500);
        await SettleAsync();
        FrameworkElement? after = SidePaneNow();
        string words = after is null ? string.Empty : string.Join(" | ", Below(after).OfType<TextBlock>().Where(block => block.ActualWidth > 0).Select(block => block.Text));
        Check("when the page comes it is in the pane and nothing says it is waiting", after is not null && words.Contains("CS-220481", StringComparison.Ordinal) && !Below(after).OfType<ProgressBar>().Any(progress => progress.IsIndeterminate && progress.ActualWidth > 0), "words: " + words);
        await ShotAsync("pane-arrived");
        if (host.State.Nav.Modals.Count > 0 && host.State.Nav.Front is long modal)
        {
            // The page that was waited for is closed the way the person closes it: the core is told it was dismissed.
            host.Send(Events.Dismissed(modal));
            await SettleAsync();
            await Task.Delay(800);
        }

        await EmptySheetAsync();
    }

    // A sheet that is open and holds nothing yet is waiting for its body: it says so, and when the body comes it is gone.
    private async Task EmptySheetAsync()
    {
        Say("== a sheet that opens with nothing in it and is filled by the answer that follows");
        host.Send(Events.Select(0));
        await SettleAsync();
        await Task.Delay(1500);
        Button? open = ButtonStarting("An order beside");
        Check("the walk's own screen has the button that opens the page with an empty sheet", open is not null, open is null ? "none" : "found");
        if (open is null)
        {
            return;
        }

        Foreground();
        _ = open.Focus(FocusState.Keyboard);
        await Task.Delay(300);
        await PressAsync(EnterKey);
        await SettleAsync();
        await Task.Delay(1500);
        FrameworkElement? pane = SidePaneNow();
        Check("the empty sheet is a pane at the side", pane is not null, pane is null ? "no pane" : "showing");
        if (pane is null)
        {
            return;
        }

        bool waiting = Below(pane).OfType<ProgressBar>().Any(progress => progress.IsIndeterminate && progress.ActualWidth > 0);
        Check("the empty sheet shows that it is waiting for its body", waiting, "words: " + string.Join(" | ", Below(pane).OfType<TextBlock>().Where(block => block.ActualWidth > 0).Select(block => block.Text)));
        await ShotAsync("pane-empty-sheet");
        host.Send(Events.Refresh(host.State.Nav.Front ?? 0));
        await SettleAsync();
        await Task.Delay(1500);
        FrameworkElement? after = SidePaneNow();
        bool bar = after is not null && Below(after).OfType<ProgressBar>().Any(progress => progress.IsIndeterminate && progress.ActualWidth > 0);
        bool order = after is not null && Below(after).OfType<TextBlock>().Any(block => block.Text.Contains("CS-220482", StringComparison.Ordinal));
        Check("when the body comes the order is in the pane and the indication is gone", order && !bar, "the order is shown: " + order + ", a bar is shown: " + bar);
        Check("the pane is the same pane after the body came", ReferenceEquals(pane, after), "same: " + ReferenceEquals(pane, after));
        await ShotAsync("pane-sheet-filled");
        if (host.State.FrontScreen is { } screen)
        {
            host.Send(Events.Overlay(screen.Id, "beside", false));
            await SettleAsync();
            await Task.Delay(500);
        }

        host.Send(Events.Back);
        await SettleAsync();
        await Task.Delay(800);
    }
}
