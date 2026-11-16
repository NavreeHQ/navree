using Navree.Design.Pages;

namespace Navree.Design.Tests;

/// <summary>What a field shows is what its form will send: a control that stands across an answer shows the value the core set itself.</summary>
[TestClass]
public sealed class ShownValueTests
{
    private static readonly DateTime Start = new(2026, 11, 16, 9, 0, 0, DateTimeKind.Utc);

    [TestMethod]
    public void Ticks_are_taken_away_when_the_form_comes_back_fresh_and_the_next_tick_sends_only_itself()
    {
        // "Email me" and "SMS me" are ticked, one after the other, and the core answers each.
        TickedValues ticks = new([]);
        CollectionAssert.AreEqual(new[] { "email" }, ticks.Tick("email", true, Start).ToArray());
        CollectionAssert.AreEqual(new[] { "email", "sms" }, ticks.Tick("sms", true, Start.AddMilliseconds(200)).ToArray());
        Assert.IsFalse(ticks.Takes(["email"], Start.AddMilliseconds(250)), "the answer to the first tick undid the second");
        Assert.IsTrue(ticks.Has("sms"));
        Assert.IsFalse(ticks.Takes(["email", "sms"], Start.AddMilliseconds(400)));

        // The form is sent and the server answers the same form, fresh, to add another: the core holds no tick.
        Assert.IsTrue(ticks.Takes([], Start.AddSeconds(5)), "the boxes stay ticked though the core holds none");
        Assert.IsFalse(ticks.Has("email"));
        Assert.IsFalse(ticks.Has("sms"));
        Assert.AreEqual(0, ticks.Now.Count);

        // A third box is ticked: it is sent alone, and the two that were ticked before do not come back.
        CollectionAssert.AreEqual(new[] { "post" }, ticks.Tick("post", true, Start.AddSeconds(6)).ToArray());
    }

    [TestMethod]
    public void Ticks_the_core_sets_itself_are_shown_and_the_same_ticks_in_another_order_are_no_change()
    {
        TickedValues ticks = new(["email", "sms"]);
        Assert.IsFalse(ticks.Takes(["sms", "email"], Start), "the same values in another order were taken for a change");
        Assert.IsTrue(ticks.Takes(["post"], Start.AddSeconds(1)));
        CollectionAssert.AreEqual(new[] { "post" }, ticks.Now.ToArray());

        // Taking a tick away sends what is left, and the answer to it changes nothing.
        CollectionAssert.AreEqual(Array.Empty<string>(), ticks.Tick("post", false, Start.AddSeconds(2)).ToArray());
        Assert.IsFalse(ticks.Takes([], Start.AddSeconds(2)));
    }

    [TestMethod]
    public void A_time_the_core_sets_or_takes_away_is_shown_and_the_answer_to_a_time_just_picked_is_not()
    {
        ShownValue clock = new(ShownForms.Time("09:00"));

        // The server moves the booking to half past ten.
        Assert.IsTrue(clock.Takes(ShownForms.Time("10:30"), Start), "the picker shows 09:00 while the form would send 10:30");
        Assert.AreEqual("10:30", clock.Now);
        Assert.IsFalse(clock.Takes(ShownForms.Time("10:30:00"), Start.AddSeconds(1)), "the same time written another way is no change");

        // The person picks 11:00 and then 11:05; the answers come one behind.
        clock.Chose(ShownForms.Time(new TimeSpan(11, 0, 0)), Start.AddSeconds(2));
        clock.Chose(ShownForms.Time(new TimeSpan(11, 5, 0)), Start.AddSeconds(2.1));
        Assert.IsFalse(clock.Takes("11:00", Start.AddSeconds(2.2)), "the answer to the first pick undid the second");
        Assert.AreEqual("11:05", clock.Now);
        Assert.IsFalse(clock.Takes("11:05", Start.AddSeconds(2.3)));

        // The form comes back fresh: the core holds no time, and the picker shows none.
        Assert.IsTrue(clock.Takes(ShownForms.Time(string.Empty), Start.AddSeconds(9)));
        Assert.AreEqual(string.Empty, clock.Now);
        Assert.IsNull(ShownForms.TimeOf(clock.Now));
    }

    [TestMethod]
    public void A_day_the_core_takes_away_is_taken_out_of_the_picker_and_a_day_it_sets_is_shown()
    {
        ShownValue day = new(ShownForms.Day("2026-11-16"));
        Assert.IsFalse(day.Takes(ShownForms.Day("2026-11-16T00:00:00"), Start), "the same day written another way is no change");

        // The core clears the field (a form sent, to add another): the picker must show no day.
        Assert.IsTrue(day.Takes(ShownForms.Day(string.Empty), Start.AddSeconds(1)), "the picker keeps a day the core no longer holds");
        Assert.IsNull(ShownForms.DayOf(day.Now));

        // The person picks Lohri, and the answer to it is not written back.
        day.Chose(ShownForms.Day(new DateTimeOffset(2027, 1, 13, 0, 0, 0, TimeSpan.Zero)), Start.AddSeconds(2));
        Assert.IsFalse(day.Takes("2027-01-13", Start.AddSeconds(2.1)));

        // The server moves it to the next day.
        Assert.IsTrue(day.Takes(ShownForms.Day("2027-01-14"), Start.AddSeconds(8)));
        Assert.AreEqual("2027-01-14", day.Now);
    }

    [TestMethod]
    public void A_number_the_core_sets_or_takes_away_moves_the_slider_and_the_answers_to_a_drag_do_not()
    {
        ShownValue slider = new(ShownForms.Number("40"));
        Assert.IsFalse(slider.Takes(ShownForms.Number("40.0"), Start), "the same number written another way is no change");

        // A drag tells the core many values; the answers come behind it.
        slider.Chose(ShownForms.Number("41"), Start.AddSeconds(1));
        slider.Chose(ShownForms.Number("45"), Start.AddSeconds(1.05));
        slider.Chose(ShownForms.Number("52.5"), Start.AddSeconds(1.1));
        Assert.IsFalse(slider.Takes("41", Start.AddSeconds(1.15)));
        Assert.IsFalse(slider.Takes("45", Start.AddSeconds(1.2)));
        Assert.AreEqual("52.5", slider.Now, "an answer to an earlier place of the drag moved the slider back");
        Assert.IsFalse(slider.Takes("52.5", Start.AddSeconds(1.3)));

        // The core clears it, then sets it.
        Assert.IsTrue(slider.Takes(ShownForms.Number(string.Empty), Start.AddSeconds(7)), "the slider keeps a number the core no longer holds");
        Assert.IsTrue(slider.Takes(ShownForms.Number("10"), Start.AddSeconds(8)));
        Assert.AreEqual("10", slider.Now);
    }

    [TestMethod]
    public void A_searched_choice_shows_the_option_the_core_holds_and_typing_a_search_is_not_a_choice()
    {
        // The box shows Amit Kumar of Ludhiana, whose value the core holds.
        ShownValue choice = new("customer-17");

        // The person types to search: options come and go, and the core still holds the same value. Nothing is written over the words being typed.
        Assert.IsFalse(choice.Takes("customer-17", Start));
        Assert.IsFalse(choice.Takes("customer-17", Start.AddSeconds(1)));

        // The person chooses another; the answer to it is not written back.
        choice.Chose("customer-42", Start.AddSeconds(2));
        Assert.IsFalse(choice.Takes("customer-17", Start.AddSeconds(2.05)), "a state made before the choice was heard undid it");
        Assert.IsFalse(choice.Takes("customer-42", Start.AddSeconds(2.2)));

        // The form comes back fresh: the core holds no customer, and the box is emptied.
        Assert.IsTrue(choice.Takes(string.Empty, Start.AddSeconds(9)), "the box names a customer the form would not send");
        Assert.AreEqual(string.Empty, choice.Now);
    }

    [TestMethod]
    public void A_choice_the_core_never_hears_of_gives_way_to_what_the_core_holds()
    {
        // The pick was told and no answer took it: after a while the core's value is what the form would send, so it is what is shown.
        ShownValue clock = new("09:00");
        clock.Chose("11:00", Start);
        Assert.IsFalse(clock.Takes("09:00", Start.AddSeconds(1)), "the pick is still on its way");
        Assert.IsTrue(clock.Takes("09:00", Start.AddSeconds(30)), "the picker shows a time the core does not hold");
        Assert.AreEqual("09:00", clock.Now);
    }

    [TestMethod]
    public void Words_that_are_no_time_are_no_time_and_never_an_error()
    {
        Assert.AreEqual(new TimeSpan(9, 5, 0), ShownForms.TimeOf("09:05"));
        Assert.AreEqual(new TimeSpan(9, 5, 30), ShownForms.TimeOf("9:05:30"));
        Assert.AreEqual(new TimeSpan(18, 30, 0), ShownForms.TimeOf("2026-11-16T18:30:00"));
        Assert.AreEqual("18:30", ShownForms.Time("2026-11-16T18:30:00"));

        // A day with no time, a bare number, a time past the day's end, and words: each is no time.
        foreach (string none in new[] { "2026-11-16", "9", "24:00", "25:00", "1.09:00", "-01:00", "soon", " ", string.Empty })
        {
            Assert.IsNull(ShownForms.TimeOf(none), none);
            Assert.AreEqual(string.Empty, ShownForms.Time(none), none);
        }

        Assert.IsNull(ShownForms.TimeOf(null));
    }

    [TestMethod]
    public void A_day_a_number_and_several_values_each_have_one_form()
    {
        Assert.AreEqual("2026-11-16", ShownForms.Day("2026-11-16"));
        Assert.AreEqual("2026-11-16", ShownForms.Day("2026-11-16T10:00:00"));
        Assert.AreEqual(string.Empty, ShownForms.Day("the day after Diwali"));
        Assert.AreEqual(string.Empty, ShownForms.Day(null));

        Assert.AreEqual(ShownForms.Number("5"), ShownForms.Number("5.0"));
        Assert.AreEqual("52.5", ShownForms.Number("52.50"));
        Assert.AreEqual(string.Empty, ShownForms.Number("many"));
        Assert.AreEqual(string.Empty, ShownForms.Number("NaN"));
        Assert.AreEqual(string.Empty, ShownForms.Number(null));

        Assert.AreEqual(ShownForms.Many(["sms", "email"]), ShownForms.Many(["email", "sms", "email"]));
        Assert.AreNotEqual(ShownForms.Many(["a,b"]), ShownForms.Many(["a", "b"]));
        Assert.AreNotEqual(ShownForms.Many([]), ShownForms.Many([string.Empty]));
    }
}
