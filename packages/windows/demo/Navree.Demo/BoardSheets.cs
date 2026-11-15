using System.Globalization;
using System.Text.RegularExpressions;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Controls.Primitives;
using Microsoft.UI.Xaml.Input;
using Microsoft.UI.Xaml.Media;
using Navree.Runtime;

namespace Navree.Demo;

// What a person does in a sheet of the live board and with a disclosure, with the real keys: a comment sent empty is refused by the server and
// the refusal is read where it must be (in the sheet that is on screen), the core's own request to put the focus on the field in error is read
// and answered, and a disclosure opened and closed with the keyboard keeps the focus.
internal sealed partial class BoardMoves
{
    private const byte TabKey = 0x09;
    private const byte EnterKey = 0x0D;
    private const byte SpaceKey = 0x20;

    private async Task SheetsAsync()
    {
        host.Send(Events.Select(0));
        await SettleAsync();
        await Task.Delay(1500);
        await SheetErrorAsync();
        host.Send(Events.Select(0));
        await SettleAsync();
        await Task.Delay(1500);
        await DisclosureAsync();
    }

    // ---- a server's error inside a sheet ----------------------------------------------------------------------

    private async Task SheetErrorAsync()
    {
        Say("== a comment sent empty inside the card's sheet");
        (string Lane, int Index, string Id, string Title)? card = Pick("backlog", 0);
        Button? handle = card is { } picked ? HandleOf(picked.Title) : null;
        FrameworkElement? cardElement = handle is null ? null : Up(handle);
        FrameworkElement? title = card is not { } named || cardElement is null
            ? null
            : Below(cardElement).OfType<FrameworkElement>().FirstOrDefault(element => element is Button or TextBlock && element != handle && (element is TextBlock block ? block.Text == named.Title : Name(element) == named.Title || Name(element).Contains(named.Title, StringComparison.Ordinal)));
        Check("a card with a title to press is on the board", title is not null, title is null ? "none found" : "found");
        if (title is null)
        {
            return;
        }

        await ClickAsync(title);
        FrameworkElement? sheet = null;
        TextBox? comment = null;
        for (int waited = 0; waited < 9000 && comment is null; waited += 250)
        {
            await Task.Delay(250);
            sheet = SheetOnScreen();
            comment = sheet is null ? null : Below(sheet).OfType<TextBox>().FirstOrDefault(box => Name(box).Contains("comment", StringComparison.OrdinalIgnoreCase) || box.PlaceholderText.Contains("note", StringComparison.OrdinalIgnoreCase));
        }

        Check("pressing the card opens a sheet with its comment field", comment is not null, comment is null ? "no sheet with a comment box; the sheet holds " + (sheet is null ? "nothing" : DrawnText.Of(sheet).Replace('\n', ' ')[..Math.Min(120, DrawnText.Of(sheet).Length)]) : "found in " + sheet!.GetType().Name);
        if (comment is null || sheet is null)
        {
            return;
        }

        Button? send = Below(sheet).OfType<Button>().FirstOrDefault(button => Regex.IsMatch(Name(button), "comment|send|post", RegexOptions.IgnoreCase));
        Check("the sheet has the button that sends the comment", send is not null, send is null ? "buttons: " + string.Join(", ", Below(sheet).OfType<Button>().Select(Name)) : Name(send));
        if (send is null)
        {
            return;
        }

        // The comment is left empty and the button is reached with Tab and pressed with Enter.
        await ClickAsync(comment);
        Foreground();
        for (int step = 0; step < 6 && !ReferenceEquals(FocusManager.GetFocusedElement(window.Surface.XamlRoot), send); step++)
        {
            await PressAsync(TabKey);
        }

        bool reached = ReferenceEquals(FocusManager.GetFocusedElement(window.Surface.XamlRoot), send);
        Say("the send button " + (reached ? "was reached with Tab" : "was not reached with Tab; it is pressed with the pointer") + ": " + Name(send));
        bool sawPending = false;
        bool sawDrawnPending = false;
        Focus? asked = null;
        if (reached)
        {
            await PressAsync(EnterKey);
        }
        else
        {
            await ClickAsync(send);
        }

        // Watched for a while after the press: the form pending in the core and the button drawn pending, and the core's request for the focus.
        for (int sample = 0; sample < 150; sample++)
        {
            ScreenView? front = host.State.FrontScreen;
            sawPending |= front?.Forms.Values.Any(form => form.Pending) == true;
            FrameworkElement? now = SheetOnScreen();
            sawDrawnPending |= now is not null && Below(now).OfType<Button>().Any(button => Regex.IsMatch(Name(button), "comment|send|post", RegexOptions.IgnoreCase) && !button.IsEnabled) || (now is not null && Below(now).OfType<ProgressRing>().Any(ring => ring.IsActive));
            asked ??= front?.Focus;
            await Task.Delay(15);
        }

        await SettleAsync();
        await Task.Delay(1500);
        FrameworkElement? after = SheetOnScreen();
        ScreenView? screen = host.State.FrontScreen;
        string? error = screen?.Forms.Values.SelectMany(form => form.Fields.Values).SelectMany(field => field.Errors).FirstOrDefault();
        Check("the server refused the empty comment with words for the field", error is not null, error is null ? "no field holds an error; messages " + host.State.Messages.Count : "\"" + error + "\"");
        bool drawn = error is not null && after is not null && DrawnText.Of(after).Contains(error, StringComparison.Ordinal);
        Check("the error is drawn at the field inside the sheet that is on screen", drawn, error is null ? "no error to look for" : after is null ? "no sheet is on screen" : "\"" + error + "\" looked for in the words of the sheet: " + DrawnText.Of(after).Replace('\n', ' ')[..Math.Min(160, DrawnText.Of(after).Length)]);
        Check("the send button showed pending while the form was being sent", sawPending && sawDrawnPending, "the core held the form pending: " + sawPending + "; the sheet's button was drawn pending: " + sawDrawnPending);
        await ShotAsync("moves-sheet-error");

        // The core's own request: after a form's error the first field in error is asked to be given the focus, and the core is answered.
        Check("the core asked for the focus to be put on the field in error", asked is not null, asked is null ? "no focus request was seen on the screen" : "target " + asked.Target + ", serial " + asked.Serial.ToString(CultureInfo.InvariantCulture));
        if (asked is not null)
        {
            object? focused = FocusManager.GetFocusedElement(window.Surface.XamlRoot);
            bool onField = focused is TextBox box && after is not null && Below(after).Contains(box) && Chain(box).Any(part => AutomationProperties.GetAutomationId(part) == asked.Target);
            Check("the field the core named has the focus", onField, "the target is " + asked.Target + "; focus is on " + (focused is DependencyObject element ? element.GetType().Name + " '" + Name(element) + "' id '" + AutomationProperties.GetAutomationId(element) + "'" : "nothing"));
            long? answered = host.State.FrontScreen?.Focus?.Serial;
            Check("the core was answered, so it no longer asks", answered != asked.Serial, answered is null ? "the request is gone" : "the request is still there: serial " + answered.Value.ToString(CultureInfo.InvariantCulture));
        }

        if (after is not null)
        {
            host.State.FrontScreen?.Overlays.ToList().ForEach(overlay => host.Send(Events.Overlay(host.State.FrontScreen!.Id, overlay, false)));
            await SettleAsync();
        }
    }

    // What is open over the board as a sheet: the side pane in the window, or the dialog in a popup.
    private FrameworkElement? SheetOnScreen()
    {
        FrameworkElement? pane = Below(window.Surface).OfType<FrameworkElement>().FirstOrDefault(element => element.GetType().Name == "SidePane" && element.ActualWidth > 0);
        if (pane is not null)
        {
            return pane;
        }

        return Popups().SelectMany(Below).OfType<ContentDialog>().FirstOrDefault();
    }

    // ---- a disclosure opened by the keyboard ------------------------------------------------------------------

    private async Task DisclosureAsync()
    {
        Say("== a disclosure opened and closed by the keyboard");
        Expander? first = Below(window.Surface).OfType<Expander>().FirstOrDefault(candidate => candidate.ActualHeight > 0);
        Check("a disclosure is on the board screen", first is not null, first is null ? "no Expander is on the screen" : "found: " + (first.Header as string));
        if (first is null)
        {
            return;
        }

        // The keyboard starts at the first card's handle and goes with Tab until it stands in the header of a disclosure.
        Button? start = Below(window.Surface).OfType<Button>().FirstOrDefault(button => Name(button).StartsWith("Move ", StringComparison.Ordinal));
        Foreground();
        _ = start?.Focus(FocusState.Keyboard);
        await Task.Delay(400);
        string? header = null;
        for (int step = 0; step < 160 && header is null; step++)
        {
            await PressAsync(TabKey);
            header = DisclosureHeaderWithFocus();
        }

        Check("Tab reaches a disclosure", header is not null, header is null ? "the focus did not stand in a disclosure's header in 160 presses; it is on " + FocusManager.GetFocusedElement(window.Surface.XamlRoot)?.GetType().Name : "\"" + header + "\"");
        if (header is null)
        {
            return;
        }

        await PressAsync(SpaceKey);
        await SettleAsync();
        await Task.Delay(1500);
        string? opened = DisclosureHeaderWithFocus();
        Check("after the disclosure is opened with Space the focus is still on it", opened == header, "the focus is in " + (opened is null ? "no disclosure's header (on " + FocusManager.GetFocusedElement(window.Surface.XamlRoot)?.GetType().Name + ")" : "\"" + opened + "\"") + ", it was in \"" + header + "\"");
        await ShotAsync("moves-disclosure-open");
        await PressAsync(SpaceKey);
        await SettleAsync();
        await Task.Delay(1500);
        string? closed = DisclosureHeaderWithFocus();
        Check("after it is closed with Space the focus is still on it", closed == header, "the focus is in " + (closed is null ? "no disclosure's header (on " + FocusManager.GetFocusedElement(window.Surface.XamlRoot)?.GetType().Name + ")" : "\"" + closed + "\"") + ", it was in \"" + header + "\"");
    }

    // The label of the disclosure whose header holds the keyboard focus, or none.
    private string? DisclosureHeaderWithFocus()
    {
        foreach (DependencyObject part in Chain(FocusManager.GetFocusedElement(window.Surface.XamlRoot) as DependencyObject))
        {
            if (part is Expander expander)
            {
                return expander.Header as string ?? Name(expander);
            }
        }

        return null;
    }

    // ---- real keys --------------------------------------------------------------------------------------------

    private async Task PressAsync(byte key)
    {
        Foreground();
        keybd_event(key, 0, 0, UIntPtr.Zero);
        await Task.Delay(50);
        keybd_event(key, 0, KeyUp, UIntPtr.Zero);
        await Task.Delay(120);
    }

    private static string Name(DependencyObject element) => AutomationProperties.GetName(element) ?? string.Empty;

    private static IEnumerable<DependencyObject> Chain(DependencyObject? from)
    {
        for (DependencyObject? at = from; at is not null; at = VisualTreeHelper.GetParent(at))
        {
            yield return at;
        }
    }
}
