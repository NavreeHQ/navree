using System.Globalization;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Navree.Runtime;

namespace Navree.Demo;

// The fields that are not boxes of words, across an answer: the walk's own form of ticks, a time, a day and a number is answered again, fresh,
// with other values (as a server answers a form that was saved, to add another). The controls stand, and must then show what the core holds:
// no tick, the other time, no day, the other number. A tick made after it is sent alone.
internal sealed partial class PageWalk
{
    private const string PreferencesForm = "preferences";

    private FieldValue? PreferenceHeld(string field) =>
        host.State.FrontScreen is { } front && front.Forms.TryGetValue(PreferencesForm, out FormView? form) && form.Fields.TryGetValue(field, out FieldView? view)
            ? view.Value
            : null;

    private static string Said(FieldValue? value) => value switch
    {
        ManyValue many => "[" + string.Join(", ", many.Values) + "]",
        TextValue text => "'" + text.Text + "'",
        null => "nothing (no such field)",
        _ => value.GetType().Name,
    };

    private CheckBox? Tick(string words) =>
        Below(window.Surface).OfType<CheckBox>().FirstOrDefault(box => box.ActualWidth > 0 && box.Content as string == words);

    private string Ticked() =>
        string.Join(", ", Below(window.Surface).OfType<CheckBox>().Where(box => box.ActualWidth > 0 && box.IsChecked == true).Select(box => box.Content as string));

    private async Task StandingFieldsAsync()
    {
        Say("== fields that stand across an answer show what the core holds");
        window.Size(1280, 900);
        host.Send(Events.Select(0));
        await SettleAsync();
        await Task.Delay(1500);
        Button? open = ButtonStarting("Preferences");
        Check("the walk's own screen has the button that opens the preferences", open is not null, open is null ? "none" : "found");
        if (open is null || served is null)
        {
            return;
        }

        Foreground();
        _ = open.Focus(FocusState.Keyboard);
        await Task.Delay(300);
        await PressAsync(EnterKey);
        for (int waited = 0; waited < 10000 && host.State.FrontScreen?.Title != "Preferences"; waited += 250)
        {
            await Task.Delay(250);
        }

        await SettleAsync();
        await Task.Delay(1000);
        CheckBox? email = Tick("Email me");
        CheckBox? sms = Tick("SMS me");
        CheckBox? post = Tick("Write to me");
        TimePicker? clock = Below(window.Surface).OfType<TimePicker>().FirstOrDefault(picker => picker.ActualWidth > 0);
        CalendarDatePicker? day = Below(window.Surface).OfType<CalendarDatePicker>().FirstOrDefault(picker => picker.ActualWidth > 0);
        Slider? number = Below(window.Surface).OfType<Slider>().FirstOrDefault(slider => slider.ActualWidth > 0);
        bool drawn = email is not null && sms is not null && post is not null && clock is not null && day is not null && number is not null;
        Check("the form's three boxes, its time, its day and its number are on the screen", drawn, "boxes " + (email is not null) + " " + (sms is not null) + " " + (post is not null) + ", time " + (clock is not null) + ", day " + (day is not null) + ", number " + (number is not null));
        if (email is null || sms is null || post is null || clock is null || day is null || number is null)
        {
            return;
        }

        Check(
            "they show what the first answer holds",
            email.IsChecked == true && sms.IsChecked == true && post.IsChecked != true && clock.SelectedTime == new TimeSpan(9, 0, 0) && day.Date?.Date == new DateTime(2026, 11, 20) && Math.Abs(number.Value - 40) < 0.001,
            "ticked: " + Ticked() + "; time " + clock.SelectedTime + "; day " + day.Date?.Date.ToString("yyyy-MM-dd", CultureInfo.InvariantCulture) + "; number " + number.Value.ToString(CultureInfo.InvariantCulture));

        // The answer that follows is the same form, fresh: the core holds no tick, another time, no day and another number.
        host.Send(Events.Refresh(host.State.Nav.Front ?? 0));
        await SettleAsync();
        await Task.Delay(1500);
        Check(
            "the answer brought the form fresh: the core holds no tick, 10:30, no day and 10",
            PreferenceHeld("contact") is ManyValue { Values.Count: 0 } && PreferenceHeld("call-at") is TextValue { Text: "10:30" } && PreferenceHeld("visit-on") is TextValue { Text: "" } && PreferenceHeld("discount") is TextValue { Text: "10" },
            "contact " + Said(PreferenceHeld("contact")) + ", call-at " + Said(PreferenceHeld("call-at")) + ", visit-on " + Said(PreferenceHeld("visit-on")) + ", discount " + Said(PreferenceHeld("discount")));
        Check(
            "the fields stood: they are the controls that were there before the answer",
            ReferenceEquals(email, Tick("Email me")) && ReferenceEquals(clock, Below(window.Surface).OfType<TimePicker>().FirstOrDefault(picker => picker.ActualWidth > 0)) && ReferenceEquals(number, Below(window.Surface).OfType<Slider>().FirstOrDefault(slider => slider.ActualWidth > 0)),
            "the first box the same: " + ReferenceEquals(email, Tick("Email me")));
        Check("the boxes show no tick, as the core holds none", email.IsChecked != true && sms.IsChecked != true && post.IsChecked != true, "ticked: " + Ticked());
        Check("the time shows 10:30, which the core holds", clock.SelectedTime == new TimeSpan(10, 30, 0), "the picker shows " + clock.SelectedTime);
        Check("the day shows none, as the core holds none", day.Date is null, "the picker shows " + day.Date?.Date.ToString("yyyy-MM-dd", CultureInfo.InvariantCulture));
        Check("the number shows 10, which the core holds", Math.Abs(number.Value - 10) < 0.001, "the slider stands at " + number.Value.ToString(CultureInfo.InvariantCulture));
        await ShotAsync("preferences-fresh");

        // A tick made now is sent alone: the two that were ticked before the answer do not come back.
        Foreground();
        _ = post.Focus(FocusState.Keyboard);
        await Task.Delay(300);
        await PressAsync(SpaceKey);
        await SettleAsync();
        await Task.Delay(1000);
        Check(
            "a box ticked after the answer is sent alone",
            PreferenceHeld("contact") is ManyValue { Values: ["post"] } && post.IsChecked == true && email.IsChecked != true && sms.IsChecked != true,
            "the core holds " + Said(PreferenceHeld("contact")) + "; ticked: " + Ticked());
        host.Send(Events.Back);
        await SettleAsync();
        await Task.Delay(800);
    }
}
