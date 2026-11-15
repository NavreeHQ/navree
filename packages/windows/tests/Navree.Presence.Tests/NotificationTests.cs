using Navree.Protocol;

namespace Navree.Presence.Tests;

[TestClass]
public sealed class NotificationTests
{
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
        Assert.AreEqual(Now.AddMinutes(1), schedule.Next());
        Assert.AreEqual(0, schedule.Take(Now, India).Count);
        IReadOnlyList<PendingNotification> due = schedule.Take(Now.AddMinutes(5), India);
        Assert.AreEqual(1, due.Count);
        Assert.AreEqual("soon", due[0].Id);
        Assert.AreEqual(1, schedule.Waiting.Count);
        Assert.AreEqual(Now.AddMinutes(10), schedule.Next());
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
        Assert.IsNull(schedule.Next());
    }

    [TestMethod]
    public void ADailyOneWaitsAgainForTomorrowAfterItIsShown()
    {
        NotificationSchedule schedule = new();
        _ = schedule.Add(new PendingNotification("d", "Daily", null, null, null, Now.AddMinutes(1), 9, 0));
        IReadOnlyList<PendingNotification> due = schedule.Take(Now.AddMinutes(2), India);
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
        NotificationSchedule back = NotificationSchedule.Read(schedule.Write());
        CollectionAssert.AreEqual(schedule.Waiting.ToArray(), back.Waiting.ToArray());
        Assert.AreEqual(schedule.Write(), back.Write());
    }

    [TestMethod]
    public void WhatDoesNotReadIsNothingAndNeverStopsTheApp()
    {
        Assert.AreEqual(0, NotificationSchedule.Read(null).Waiting.Count);
        Assert.AreEqual(0, NotificationSchedule.Read("not json").Waiting.Count);
        Assert.AreEqual(0, NotificationSchedule.Read("{\"a\":1}").Waiting.Count);
        Assert.AreEqual(0, NotificationSchedule.Read("[{\"id\":1}]").Waiting.Count);
        Assert.AreEqual(0, NotificationSchedule.Read("[{\"id\":\"a\",\"title\":\"t\",\"at\":1,\"hour\":9}]").Waiting.Count);
        Assert.AreEqual(1, NotificationSchedule.Read("[{\"id\":\"a\",\"title\":\"t\",\"at\":1,\"href\":\"//evil.example\"}]").Waiting.Count);
        Assert.IsNull(NotificationSchedule.Read("[{\"id\":\"a\",\"title\":\"t\",\"at\":1,\"href\":\"//evil.example\"}]").Waiting[0].Href);
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

    [TestMethod]
    public void OnlyTheCoresOwnWordsThatItStoppedAreAStop()
    {
        Assert.IsTrue(CoreFault.IsStop("the core refused an event: the core stopped after an earlier failure"));
        Assert.IsTrue(CoreFault.IsStop("the core refused an event: the core failed on this event"));
        Assert.IsTrue(CoreFault.IsStop("the core refused a view: the core failed on this query"));
        Assert.IsFalse(CoreFault.IsStop("the core refused an event: the event does not read: expected value"));
        Assert.IsFalse(CoreFault.IsStop("the core refused a view: no core"));
        Assert.IsFalse(CoreFault.IsStop(null));
    }
}
