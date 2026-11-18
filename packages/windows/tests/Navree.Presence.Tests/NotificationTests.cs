using Navree.Protocol;

namespace Navree.Presence.Tests;

[TestClass]
public sealed class NotificationTests
{
    // Marks as the app keeps them for the people who sign in: each tells one person from another and says nothing of either. The first
    // two are made of the key the core gives for a person, the third from a name and an email.
    private const string Simran = "key:0a9b8c7d6e5f40312233445566778899aabbccddeeff00112233445566778899";
    private const string Manpreet = "key:6b1f0d2c9a7e4853b1c6d0e2f4a8975c3e2d1b0a9f8e7d6c5b4a39281706f5e4";
    private const string MadeFromAName = "A1B2C3D4E5F60718293A4B5C6D7E8F90A1B2C3D4E5F60718293A4B5C6D7E8F90";
    private static readonly TimeZoneInfo India = TimeZoneInfo.FindSystemTimeZoneById("India Standard Time");
    private static readonly DateTimeOffset Now = new(2026, 11, 15, 12, 0, 0, TimeSpan.FromHours(5.5));

    private static Models.Notify Ask(Models.When when, string id = "n1", string? href = "/orders/1") =>
        new() { Id = id, Title = "Order CS-20481 has shipped", Body = "Tanvi Sharma, Pune", Href = href, At = when };

    [TestMethod]
    public void AMomentIsDueAtThatMoment()
    {
        PendingNotification plan = NotifyRules.Plan(Ask(new Models.WhenAt { Unix = Now.AddHours(2).ToUnixTimeSeconds() }), Now, India)!;
        Assert.AreEqual(Now.AddHours(2).ToUnixTimeSeconds(), plan.At.ToUnixTimeSeconds());
        Assert.IsFalse(plan.Repeats);
        Assert.AreEqual("/orders/1", plan.Href);
    }

    [TestMethod]
    public void AMomentThatPassedAMinuteAgoIsStillDueNowButOneFromADayAgoIsNotNews()
    {
        Assert.IsNotNull(NotifyRules.Plan(Ask(new Models.WhenAt { Unix = Now.AddMinutes(-1).ToUnixTimeSeconds() }), Now, India));
        Assert.IsNull(NotifyRules.Plan(Ask(new Models.WhenAt { Unix = Now.AddDays(-2).ToUnixTimeSeconds() }), Now, India));
    }

    [TestMethod]
    public void AfterNoTimeStillWaitsOneSecond()
    {
        PendingNotification zero = NotifyRules.Plan(Ask(new Models.WhenAfter { Secs = 0 }), Now, India)!;
        Assert.AreEqual(Now.AddSeconds(1), zero.At);
        PendingNotification ten = NotifyRules.Plan(Ask(new Models.WhenAfter { Secs = 600 }), Now, India)!;
        Assert.AreEqual(Now.AddMinutes(10), ten.At);
    }

    [TestMethod]
    public void DailyIsDueAtThatHourAndMinuteWhereThePersonIsAndRepeats()
    {
        PendingNotification later = NotifyRules.Plan(Ask(new Models.WhenDaily { Hour = 18, Minute = 30 }), Now, India)!;
        Assert.AreEqual(new DateTimeOffset(2026, 11, 15, 18, 30, 0, TimeSpan.FromHours(5.5)), later.At);
        Assert.IsTrue(later.Repeats);
        PendingNotification tomorrow = NotifyRules.Plan(Ask(new Models.WhenDaily { Hour = 9, Minute = 0 }), Now, India)!;
        Assert.AreEqual(new DateTimeOffset(2026, 11, 16, 9, 0, 0, TimeSpan.FromHours(5.5)), tomorrow.At);
    }

    [TestMethod]
    public void ADailyTimeIsToldInThePersonsZoneNotTheDevicesOffsetAtSomeOtherTime()
    {
        TimeZoneInfo utc = TimeZoneInfo.Utc;
        DateTimeOffset at = NotifyRules.NextDaily(new DateTimeOffset(2026, 11, 15, 12, 0, 0, TimeSpan.Zero), 12, 0, utc);
        Assert.AreEqual(new DateTimeOffset(2026, 11, 16, 12, 0, 0, TimeSpan.Zero), at);
        TimeZoneInfo newYork = TimeZoneInfo.FindSystemTimeZoneById("Eastern Standard Time");
        DateTimeOffset summer = NotifyRules.NextDaily(new DateTimeOffset(2026, 7, 1, 0, 0, 0, TimeSpan.Zero), 8, 0, newYork);
        Assert.AreEqual(TimeSpan.FromHours(-4), summer.Offset);
        Assert.AreEqual(8, summer.Hour);
    }

    [TestMethod]
    public void ATimeThisBuildDoesNotKnowOrAnHourThatDoesNotExistIsNotScheduled()
    {
        Assert.IsNull(NotifyRules.Plan(Ask(new Models.WhenUnknown { Tag = "lunar" }), Now, India));
        Assert.IsNull(NotifyRules.Plan(Ask(new Models.WhenDaily { Hour = 24, Minute = 0 }), Now, India));
        Assert.IsNull(NotifyRules.Plan(Ask(new Models.WhenDaily { Hour = 3, Minute = 60 }), Now, India));
    }

    [TestMethod]
    public void AnIdThatIsEmptyOrLongIsRefusedAndAnAddressThatIsNotAPathIsDropped()
    {
        Assert.IsNull(NotifyRules.Plan(Ask(new Models.WhenAfter { Secs = 1 }, id: string.Empty), Now, India));
        Assert.IsNull(NotifyRules.Plan(Ask(new Models.WhenAfter { Secs = 1 }, id: new string('x', 129)), Now, India));
        PendingNotification plan = NotifyRules.Plan(Ask(new Models.WhenAfter { Secs = 1 }, href: "https://evil.example/"), Now, India)!;
        Assert.IsNull(plan.Href);
    }

    [TestMethod]
    public void TextIsClippedToALineOrTwo()
    {
        Models.Notify notify = new() { Id = "n", Title = new string('t', 1000), Body = new string('b', 1000), At = new Models.WhenAfter { Secs = 1 } };
        PendingNotification plan = NotifyRules.Plan(notify, Now, India)!;
        Assert.AreEqual(NotifyRules.LongestText, plan.Title.Length);
        Assert.AreEqual(NotifyRules.LongestText, plan.Body!.Length);
    }

    [TestMethod]
    public void AScheduleGivesWhatIsDueInOrderAndForgetsIt()
    {
        NotificationSchedule schedule = new();
        _ = schedule.Add(new PendingNotification("late", "Late", null, null, null, Now.AddMinutes(10), null, null));
        _ = schedule.Add(new PendingNotification("soon", "Soon", null, null, null, Now.AddMinutes(1), null, null));
        Assert.AreEqual(Now.AddMinutes(1), schedule.Next(null));
        Assert.AreEqual(0, schedule.Take(Now, India, null).Count);
        IReadOnlyList<PendingNotification> due = schedule.Take(Now.AddMinutes(5), India, null);
        Assert.AreEqual(1, due.Count);
        Assert.AreEqual("soon", due[0].Id);
        Assert.AreEqual(1, schedule.Waiting.Count);
        Assert.AreEqual(Now.AddMinutes(10), schedule.Next(null));
    }

    [TestMethod]
    public void AnotherWithTheSameIdTakesThePlaceAndCancelTakesOneBack()
    {
        NotificationSchedule schedule = new();
        _ = schedule.Add(new PendingNotification("a", "First", null, null, null, Now.AddMinutes(1), null, null));
        _ = schedule.Add(new PendingNotification("a", "Second", null, null, null, Now.AddMinutes(2), null, null));
        Assert.AreEqual(1, schedule.Waiting.Count);
        Assert.AreEqual("Second", schedule.Waiting[0].Title);
        Assert.IsTrue(schedule.Cancel("a"));
        Assert.IsFalse(schedule.Cancel("a"));
        Assert.IsNull(schedule.Next(null));
    }

    [TestMethod]
    public void ADailyOneWaitsAgainForTomorrowAfterItIsShown()
    {
        NotificationSchedule schedule = new();
        _ = schedule.Add(new PendingNotification("d", "Daily", null, null, null, Now.AddMinutes(1), 9, 0));
        IReadOnlyList<PendingNotification> due = schedule.Take(Now.AddMinutes(2), India, null);
        Assert.AreEqual(1, due.Count);
        Assert.AreEqual(1, schedule.Waiting.Count);
        Assert.AreEqual(new DateTimeOffset(2026, 11, 16, 9, 0, 0, TimeSpan.FromHours(5.5)), schedule.Waiting[0].At);
    }

    [TestMethod]
    public void NoMoreThanSixtyFourWaitAtOnce()
    {
        NotificationSchedule schedule = new();
        for (int at = 0; at < NotifyRules.MostWaiting; at++)
        {
            Assert.IsTrue(schedule.Add(new PendingNotification("n" + at, "t", null, null, null, Now.AddMinutes(at + 1), null, null)));
        }

        Assert.IsFalse(schedule.Add(new PendingNotification("one-more", "t", null, null, null, Now.AddMinutes(1), null, null)));
        Assert.IsTrue(schedule.Add(new PendingNotification("n3", "again", null, null, null, Now.AddMinutes(1), null, null)));
    }

    [TestMethod]
    public void WhatWaitsIsWrittenAndReadBackTheSame()
    {
        NotificationSchedule schedule = new();
        _ = schedule.Add(new PendingNotification("a", "Title", "Body", "/orders/1", "orders", Now.AddMinutes(3), null, null));
        _ = schedule.Add(new PendingNotification("b", "Daily", null, null, null, Now.AddHours(5), 9, 30));
        _ = schedule.Add(new PendingNotification("c", "Hers", null, null, null, Now.AddHours(6), null, null) { Owner = Simran });
        NotificationSchedule back = NotificationSchedule.Read(schedule.Write());
        CollectionAssert.AreEqual(schedule.Waiting.ToArray(), back.Waiting.ToArray());
        Assert.AreEqual(schedule.Write(), back.Write());
        Assert.AreEqual(Simran, back.Waiting.Single(item => item.Id == "c").Owner, "whose a notification is was not kept with it");
        Assert.IsNull(back.Waiting.Single(item => item.Id == "a").Owner, "a notification that is nobody's was read as somebody's");
    }

    [TestMethod]
    public void WhatDoesNotReadIsNothingAndNeverStopsTheApp()
    {
        Assert.AreEqual(0, NotificationSchedule.Read(null).Waiting.Count);
        Assert.AreEqual(0, NotificationSchedule.Read("not json").Waiting.Count);
        Assert.AreEqual(0, NotificationSchedule.Read("{\"a\":1}").Waiting.Count);
        Assert.AreEqual(0, NotificationSchedule.Read("[{\"id\":1}]").Waiting.Count);
        Assert.AreEqual(0, NotificationSchedule.Read("[{\"id\":\"a\",\"title\":\"t\",\"at\":1,\"hour\":9,\"for\":null}]").Waiting.Count);
        Assert.AreEqual(1, NotificationSchedule.Read("[{\"id\":\"a\",\"title\":\"t\",\"at\":1,\"href\":\"//evil.example\",\"for\":null}]").Waiting.Count);
        Assert.IsNull(NotificationSchedule.Read("[{\"id\":\"a\",\"title\":\"t\",\"at\":1,\"href\":\"//evil.example\",\"for\":null}]").Waiting[0].Href);
    }

    [TestMethod]
    public void OneThatDoesNotSayWhoseItIsIsNotRead()
    {
        // Whose it is, is said for each: a person's mark, or null for nobody's.
        Assert.AreEqual(Simran, NotificationSchedule.Read("[{\"id\":\"a\",\"title\":\"t\",\"at\":1,\"for\":\"" + Simran + "\"}]").Waiting.Single().Owner);
        NotificationSchedule nobodys = NotificationSchedule.Read("[{\"id\":\"a\",\"title\":\"t\",\"at\":1,\"for\":null}]");
        Assert.AreEqual(1, nobodys.Waiting.Count);
        Assert.IsNull(nobodys.Waiting[0].Owner);

        // One that does not say, as a build that knew no owners wrote it, may be anybody's: it is not read, so it is shown to nobody.
        Assert.AreEqual(0, NotificationSchedule.Read("[{\"id\":\"a\",\"title\":\"Call Mr. Arora about the insulin refill\",\"at\":1}]").Waiting.Count, "a notification that does not say whose it is was read: it would be shown to whoever is there");
        Assert.AreEqual(0, NotificationSchedule.Read("[{\"id\":\"a\",\"title\":\"t\",\"at\":1,\"for\":\"\"}]").Waiting.Count, "an empty mark was read as a person's or as nobody's");
        Assert.AreEqual(0, NotificationSchedule.Read("[{\"id\":\"a\",\"title\":\"t\",\"at\":1,\"for\":7}]").Waiting.Count);
        Assert.AreEqual(0, NotificationSchedule.Read("[{\"id\":\"a\",\"title\":\"t\",\"at\":1,\"for\":false}]").Waiting.Count);

        // The others beside it are still read.
        NotificationSchedule mixed = NotificationSchedule.Read("[{\"id\":\"a\",\"title\":\"t\",\"at\":1},{\"id\":\"b\",\"title\":\"t\",\"at\":2,\"for\":null}]");
        Assert.AreEqual("b", mixed.Waiting.Single().Id);
    }

    [TestMethod]
    public void OneThatIsAPersonsIsHeldWhileTheyAreNotTheOneSignedInAndShownWhenTheyAreAgain()
    {
        // At the pharmacy counter in Model Town, Simran asks to be reminded at nine. The counter's own notice is nobody's.
        NotificationSchedule schedule = new();
        PendingNotification hers = new("insulin", "Call Mr. Arora about the insulin refill", null, "/orders/1", null, Now.AddMinutes(1), null, null) { Owner = Simran };
        _ = schedule.Add(hers);
        _ = schedule.Add(new PendingNotification("opens", "The counter opens at 9", null, null, null, Now.AddMinutes(2), null, null));

        // While she is signed in, hers is the next one due.
        Assert.AreEqual(Now.AddMinutes(1), schedule.Next(Simran));

        // Her session runs out overnight and the sign-in is up: nobody is signed in. Hers sets no timer and is not given, though it is
        // due; what is nobody's is.
        Assert.AreEqual(Now.AddMinutes(2), schedule.Next(null), "a notification held for a person sets the timer while she is not signed in");
        IReadOnlyList<PendingNotification> due = schedule.Take(Now.AddMinutes(5), India, null);
        Assert.AreEqual("opens", due.Single().Id, "a person's notification was shown while nobody was signed in");
        Assert.AreEqual(1, schedule.HeldBack(Now.AddMinutes(5), null));
        Assert.IsNull(schedule.Next(null));
        Assert.AreEqual(0, schedule.Take(Now.AddHours(3), India, null).Count);

        // Manpreet starts work and is the one signed in: it is not his. (The app takes everything that waits away when another person
        // signs in; held by the schedule alone, it is still not given to him.)
        Assert.AreEqual(0, schedule.Take(Now.AddHours(3), India, Manpreet).Count, "a person's notification was shown to somebody else");
        Assert.IsNull(schedule.Next(Manpreet));
        Assert.AreEqual(1, schedule.HeldBack(Now.AddHours(3), Manpreet));
        Assert.AreEqual(hers, schedule.Waiting.Single(), "what was held did not wait as it was");

        // Simran signs in again: what came due while she was away is given now, and is gone.
        Assert.AreEqual(Now.AddMinutes(1), schedule.Next(Simran));
        Assert.AreEqual(0, schedule.HeldBack(Now.AddHours(4), Simran));
        Assert.AreEqual("insulin", schedule.Take(Now.AddHours(4), India, Simran).Single().Id, "what came due while she was not signed in was never shown to her");
        Assert.AreEqual(0, schedule.Waiting.Count);
    }

    [TestMethod]
    public void ADailyOneThatIsAPersonsStaysAtItsTimeWhileHeldAndWaitsForTheNextDayOnceItIsShown()
    {
        NotificationSchedule schedule = new();
        DateTimeOffset nine = new(2026, 11, 16, 9, 0, 0, TimeSpan.FromHours(5.5));
        _ = schedule.Add(new PendingNotification("stock", "Stock count", null, null, null, nine, 9, 0) { Owner = Simran });

        // Three mornings pass with nobody signed in: it is not given, and not moved on to the next day.
        for (int day = 0; day < 3; day++)
        {
            Assert.AreEqual(0, schedule.Take(nine.AddDays(day).AddMinutes(1), India, null).Count);
        }

        Assert.AreEqual(nine, schedule.Waiting.Single().At, "a notification that was held was moved on as if it had been shown");

        // She signs in on the third day at noon: it is given once, and waits again for the next morning.
        DateTimeOffset back = nine.AddDays(2).AddHours(3);
        Assert.AreEqual(1, schedule.Take(back, India, Simran).Count);
        Assert.AreEqual(nine.AddDays(3), schedule.Waiting.Single().At);
        Assert.AreEqual(Simran, schedule.Waiting.Single().Owner, "a daily notification lost whose it is when it was shown");
        Assert.AreEqual(0, schedule.Take(back, India, Simran).Count, "what was missed for three days was shown more than once");
    }

    [TestMethod]
    public void AStartKeepsOfAPersonsOnlyThoseOfThePersonItCanSayWhatIsKeptIsKeptFor()
    {
        string Kept()
        {
            NotificationSchedule schedule = new();
            _ = schedule.Add(new PendingNotification("insulin", "Call Mr. Arora about the insulin refill", null, null, null, Now.AddMinutes(-5), null, null) { Owner = Simran });
            _ = schedule.Add(new PendingNotification("stock", "Stock count", null, null, null, Now.AddHours(1), 9, 0) { Owner = Simran });
            _ = schedule.Add(new PendingNotification("opens", "The counter opens at 9", null, null, null, Now.AddMinutes(-5), null, null));
            return schedule.Write();
        }

        // The start read Simran's mark: hers wait on, held until she is named as signed in.
        NotificationSchedule known = NotificationSchedule.Read(Kept());
        Assert.AreEqual(0, known.KeepFor(Simran));
        Assert.AreEqual(3, known.Waiting.Count);
        Assert.AreEqual("opens", known.Take(Now, India, null).Single().Id, "a person's notification that was due at the start was shown before anybody was named");

        // The start read no mark (it was never written, or cannot be opened): it cannot say whose hers are, and lets go of them before
        // anything is armed. What is nobody's stays: an app nobody signs in to never has a mark, and its notifications are nobody's.
        NotificationSchedule unknown = NotificationSchedule.Read(Kept());
        Assert.AreEqual(2, unknown.KeepFor(null), "a start that cannot say whose a person's notifications are kept them");
        Assert.AreEqual("opens", unknown.Waiting.Single().Id);
        Assert.AreEqual(0, unknown.Take(Now.AddDays(2), India, Simran).Count(item => item.Owner is not null), "a notification of hers still waits after a start that could not say whose it is");

        // The start read another person's mark (a change of person that was cut short): hers are not his.
        NotificationSchedule another = NotificationSchedule.Read(Kept());
        Assert.AreEqual(2, another.KeepFor(Manpreet));
        Assert.AreEqual(0, another.Waiting.Count(item => item.Owner is not null));

        // A mark of the other kind, made from a name and an email, is another person's, whoever it was made for.
        NotificationSchedule otherKind = NotificationSchedule.Read(Kept());
        Assert.AreEqual(2, otherKind.KeepFor(MadeFromAName));
    }

    [TestMethod]
    public void OneAskedForBeforeThePersonWasNamedIsTheirsOnceTheyAreAndIsNotShownToWhoeverIsThereAfter()
    {
        // Simran signs in at the counter after somebody signed out. Her orders are on screen before the answer that names her, and she
        // asks to be reminded at five to call Mrs. Bedi about the refund: nobody has been named, so it is nobody's.
        NotificationSchedule schedule = new();
        DateTimeOffset five = new(2026, 11, 15, 17, 0, 0, TimeSpan.FromHours(5.5));
        _ = schedule.Add(new PendingNotification("bedi", "Call Mrs. Bedi about the refund", null, null, null, five, null, null));
        Assert.AreEqual(five, schedule.Next(null), "this test shows nothing: what is nobody's does not wait to be shown to whoever is there");

        // The answer names her, the first since the sign-out: what waits for nobody is hers.
        Assert.AreEqual(1, schedule.GiveTo(Simran));
        Assert.AreEqual(Simran, schedule.Waiting.Single().Owner);
        Assert.AreEqual(0, schedule.GiveTo(Simran), "what was given once was given again");
        Assert.AreEqual(five, schedule.Next(Simran), "what was given to her does not wait for her any more");

        // She leaves at half past four and the computer is shut down. Next morning Manpreet starts the app: the start read her mark,
        // and nobody is named yet. Her reminder, long due, is not shown to him.
        NotificationSchedule nextMorning = NotificationSchedule.Read(schedule.Write());
        Assert.AreEqual(0, nextMorning.KeepFor(Simran));
        DateTimeOffset morning = five.AddHours(16);
        Assert.IsNull(nextMorning.Next(null), "a reminder she asked for before she was named sets the timer for whoever is there");
        Assert.AreEqual(0, nextMorning.Take(morning, India, null).Count, "a reminder she asked for before she was named is shown to whoever starts the app");
        Assert.AreEqual(0, nextMorning.Take(morning, India, Manpreet).Count);
        Assert.AreEqual(1, nextMorning.HeldBack(morning, null));

        // A start that read no mark cannot say it is hers, and lets go of it.
        NotificationSchedule noMark = NotificationSchedule.Read(schedule.Write());
        Assert.AreEqual(1, noMark.KeepFor(null));

        // When she is back and signed in, it is shown to her.
        Assert.AreEqual("bedi", nextMorning.Take(morning, India, Simran).Single().Id);

        // What is a person's already is not given to another, and an app nobody signs in to never gives: its reminders stay nobody's.
        NotificationSchedule mixed = new();
        _ = mixed.Add(new PendingNotification("hers", "Hers", null, null, null, five, null, null) { Owner = Simran });
        _ = mixed.Add(new PendingNotification("nobodys", "Nobody's", null, null, null, five, 9, 0));
        Assert.AreEqual(1, mixed.GiveTo(Manpreet));
        Assert.AreEqual(Simran, mixed.Waiting.Single(item => item.Id == "hers").Owner, "a person's reminder was given to somebody else");
        PendingNotification given = mixed.Waiting.Single(item => item.Id == "nobodys");
        Assert.AreEqual(Manpreet, given.Owner);
        Assert.AreEqual((five, 9, 0), (given.At, given.Hour!.Value, given.Minute!.Value), "giving a reminder to a person changed when it is due");

        // A start that read a person's mark gives what is nobody's to that person: what is kept is kept for them.
        NotificationSchedule leftOver = NotificationSchedule.Read("[{\"id\":\"bedi\",\"title\":\"Call Mrs. Bedi about the refund\",\"at\":1,\"for\":null}]");
        Assert.AreEqual(0, leftOver.KeepFor(Simran));
        Assert.AreEqual(1, leftOver.GiveTo(Simran));
        Assert.AreEqual(0, leftOver.Take(morning, India, null).Count, "what was nobody's at a start that read a person's mark is still shown to whoever is there");
    }

    [TestMethod]
    public void OneAskedForWhileNobodyIsSignedInIsNobodysAndNotThePersonsWhoWasSignedInBefore()
    {
        // Simran's session ran out at the counter in Model Town, and the sign-in went up over her work. Her reminder of nine is hers,
        // and held.
        NotificationSchedule schedule = new();
        DateTimeOffset nine = new(2026, 11, 16, 9, 0, 0, TimeSpan.FromHours(5.5));
        DateTimeOffset eleven = nine.AddHours(2);
        _ = schedule.Add(new PendingNotification("insulin", "Call Mr. Arora about the insulin refill", null, null, null, nine, null, null) { Owner = Simran });

        // At twenty past nine somebody signs in at it, and the shop's line drops before the shell has said who. On the orders page they
        // ask to be reminded at eleven. Nobody is signed in that the app knows of, so it is kept by who is signed in: nobody. It is
        // not kept as Simran's, though what is kept is still kept for her.
        string? signedIn = null;
        _ = schedule.Add(new PendingNotification("daughter", "Call Mr. Arora's daughter about the refill", null, null, null, eleven, null, null) { Owner = signedIn });
        Assert.IsNull(schedule.Waiting.Single(item => item.Id == "daughter").Owner);

        // At eleven it is shown to whoever is there, who asked for it. Simran's is still held: it is not theirs to see.
        Assert.AreEqual(eleven, schedule.Next(null), "a reminder asked for while nobody is signed in waits for the person who was signed in before");
        Assert.AreEqual("daughter", schedule.Take(eleven.AddMinutes(1), India, null).Single().Id, "a reminder asked for while nobody is signed in is held with those of the person who was signed in before");
        Assert.AreEqual("insulin", schedule.Waiting.Single().Id);
        Assert.AreEqual(1, schedule.HeldBack(eleven.AddMinutes(1), null));

        // When it was Simran herself who signed in again, and the shell then names her, what she asked for meanwhile is given to her
        // with no change of person: it is hers from there on, shown to her and no longer to whoever is there.
        NotificationSchedule hers = new();
        _ = hers.Add(new PendingNotification("insulin", "Call Mr. Arora about the insulin refill", null, null, null, nine, null, null) { Owner = Simran });
        _ = hers.Add(new PendingNotification("lunch", "Order lunch for the counter", null, null, null, eleven, null, null) { Owner = signedIn });
        Assert.AreEqual(1, hers.GiveTo(Simran));
        Assert.AreEqual(0, hers.Take(eleven.AddMinutes(1), India, null).Count, "what the same person asked for before the shell named her is shown to whoever is there after it has");
        CollectionAssert.AreEqual(new[] { "insulin", "lunch" }, hers.Take(eleven.AddMinutes(1), India, Simran).Select(item => item.Id).ToArray());
    }

    [TestMethod]
    public void ASnapshotThatHoldsNobodyIsNotKept()
    {
        Assert.IsFalse(SnapshotRules.ShouldKeep("{\"version\":1,\"person_unknown\":true}"));
        Assert.IsTrue(SnapshotRules.ShouldKeep("{\"version\":1,\"person\":\"abc\",\"destinations\":[]}"));
        Assert.IsTrue(SnapshotRules.ShouldKeep("{\"version\":1,\"person_unknown\":false}"));
        Assert.IsFalse(SnapshotRules.ShouldKeep(null));
        Assert.IsFalse(SnapshotRules.ShouldKeep(string.Empty));
        Assert.IsFalse(SnapshotRules.ShouldKeep("null"));
        Assert.IsFalse(SnapshotRules.ShouldKeep("not json"));
        Assert.IsFalse(SnapshotRules.ShouldKeep("[1,2]"));
    }
}
