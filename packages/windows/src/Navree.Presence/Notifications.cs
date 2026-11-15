using System.Text.Json;
using Navree.Protocol;

namespace Navree.Presence;

/// <summary>A notification the app has been asked to show later, or every day.</summary>
/// <param name="Id">What the server calls it: another with the same id takes its place, and <c>cancel_notification</c> names it.</param>
/// <param name="Title">The first line.</param>
/// <param name="Body">The line under it.</param>
/// <param name="Href">The address of the app a click on it opens.</param>
/// <param name="Thread">The group it belongs to.</param>
/// <param name="At">When it is next due.</param>
/// <param name="Hour">For a daily one, the hour (0 to 23) it is due.</param>
/// <param name="Minute">For a daily one, the minute.</param>
public sealed record PendingNotification(
    string Id,
    string Title,
    string? Body,
    string? Href,
    string? Thread,
    DateTimeOffset At,
    int? Hour,
    int? Minute)
{
    /// <summary>Whether it comes again after it is shown.</summary>
    public bool Repeats => Hour is not null && Minute is not null;
}

/// <summary>When a notification the core asked for is due, as plain rules.</summary>
public static class NotifyRules
{
    /// <summary>The most a title or a line is given: a notification is a line or two.</summary>
    public const int LongestText = 256;

    /// <summary>How late a moment that has passed may be and still be shown: a notification asked for a time already gone a day ago is not news.</summary>
    public static readonly TimeSpan Stale = TimeSpan.FromDays(1);

    /// <summary>The most notifications waiting at once.</summary>
    public const int MostWaiting = 64;

    /// <summary>What the core asked for, as something that waits for its time.</summary>
    /// <param name="notify">The call.</param>
    /// <param name="now">The time.</param>
    /// <param name="zone">The person's time zone, which a daily time is told in.</param>
    /// <returns>What to wait for, or null for a time this build does not know or a time too long gone.</returns>
    public static PendingNotification? Plan(Models.Notify notify, DateTimeOffset now, TimeZoneInfo zone)
    {
        ArgumentNullException.ThrowIfNull(notify);
        if (string.IsNullOrEmpty(notify.Id) || notify.Id.Length > 128)
        {
            return null;
        }

        string title = Clip(notify.Title);
        string? body = notify.Body is null ? null : Clip(notify.Body);
        string? href = OpeningRules.IsAppPath(notify.Href) ? notify.Href : null;
        string? thread = notify.Thread is null ? null : Clip(notify.Thread);
        switch (notify.At)
        {
            case Models.WhenAt at:
                DateTimeOffset moment;
                try
                {
                    moment = DateTimeOffset.FromUnixTimeSeconds(at.Unix);
                }
                catch (ArgumentOutOfRangeException)
                {
                    return null;
                }

                return now - moment > Stale ? null : new PendingNotification(notify.Id, title, body, href, thread, moment, null, null);
            case Models.WhenAfter after:
                return new PendingNotification(notify.Id, title, body, href, thread, now + TimeSpan.FromSeconds(Math.Clamp(after.Secs, 1, 366L * 24 * 3600)), null, null);
            case Models.WhenDaily daily when daily.Hour is >= 0 and <= 23 && daily.Minute is >= 0 and <= 59:
                return new PendingNotification(notify.Id, title, body, href, thread, NextDaily(now, daily.Hour, daily.Minute, zone), daily.Hour, daily.Minute);
            default:
                return null;
        }
    }

    /// <summary>The next moment, after <paramref name="now"/>, it is this hour and minute where the person is.</summary>
    /// <param name="now">The time.</param>
    /// <param name="hour">The hour, 0 to 23.</param>
    /// <param name="minute">The minute, 0 to 59.</param>
    /// <param name="zone">The person's time zone.</param>
    /// <returns>The moment.</returns>
    public static DateTimeOffset NextDaily(DateTimeOffset now, int hour, int minute, TimeZoneInfo zone)
    {
        DateTimeOffset local = TimeZoneInfo.ConvertTime(now, zone);
        DateTime day = local.Date;
        for (int step = 0; step < 3; step++)
        {
            DateTime wall = day.AddDays(step).AddHours(hour).AddMinutes(minute);
            if (zone.IsInvalidTime(wall))
            {
                wall = wall.AddHours(1);
            }

            DateTimeOffset candidate = new(wall, zone.GetUtcOffset(wall));
            if (candidate > now)
            {
                return candidate;
            }
        }

        return now.AddDays(1);
    }

    private static string Clip(string text) => text.Length <= LongestText ? text : text[..LongestText];
}

/// <summary>
/// The notifications that wait for their time. Nothing in it keeps a clock or a timer: the app asks what is due and when to
/// ask again, so each rule is the same on the runner as on a person's machine.
/// </summary>
public sealed class NotificationSchedule
{
    private readonly List<PendingNotification> waiting = [];

    /// <summary>What waits, soonest first.</summary>
    public IReadOnlyList<PendingNotification> Waiting => [.. waiting.OrderBy(item => item.At)];

    /// <summary>Starts to wait for one; another with the same id is replaced.</summary>
    /// <param name="item">The notification.</param>
    /// <returns>Whether it is waiting (it is not when too many already are).</returns>
    public bool Add(PendingNotification item)
    {
        ArgumentNullException.ThrowIfNull(item);
        waiting.RemoveAll(held => held.Id == item.Id);
        if (waiting.Count >= NotifyRules.MostWaiting)
        {
            return false;
        }

        waiting.Add(item);
        return true;
    }

    /// <summary>Stops waiting for one.</summary>
    /// <param name="id">Its id.</param>
    /// <returns>Whether it was waiting.</returns>
    public bool Cancel(string id) => waiting.RemoveAll(held => held.Id == id) > 0;

    /// <summary>Takes what is due. One that repeats waits again for its next time; the rest are gone.</summary>
    /// <param name="now">The time.</param>
    /// <param name="zone">The person's time zone.</param>
    /// <returns>What to show now.</returns>
    public IReadOnlyList<PendingNotification> Take(DateTimeOffset now, TimeZoneInfo zone)
    {
        List<PendingNotification> due = [.. waiting.Where(item => item.At <= now).OrderBy(item => item.At)];
        foreach (PendingNotification item in due)
        {
            waiting.Remove(item);
            if (item.Repeats)
            {
                waiting.Add(item with { At = NotifyRules.NextDaily(now, item.Hour!.Value, item.Minute!.Value, zone) });
            }
        }

        return due;
    }

    /// <summary>When something is next due.</summary>
    /// <returns>The moment, or null when nothing waits.</returns>
    public DateTimeOffset? Next() => waiting.Count == 0 ? null : waiting.Min(item => item.At);

    /// <summary>Writes what waits, to be kept between launches.</summary>
    /// <returns>JSON text.</returns>
    public string Write()
    {
        using MemoryStream stream = new();
        using (Utf8JsonWriter w = new(stream))
        {
            w.WriteStartArray();
            foreach (PendingNotification item in waiting)
            {
                w.WriteStartObject();
                w.WriteString("id", item.Id);
                w.WriteString("title", item.Title);
                if (item.Body is not null)
                {
                    w.WriteString("body", item.Body);
                }

                if (item.Href is not null)
                {
                    w.WriteString("href", item.Href);
                }

                if (item.Thread is not null)
                {
                    w.WriteString("thread", item.Thread);
                }

                w.WriteNumber("at", item.At.ToUnixTimeSeconds());
                if (item.Hour is int hour && item.Minute is int minute)
                {
                    w.WriteNumber("hour", hour);
                    w.WriteNumber("minute", minute);
                }

                w.WriteEndObject();
            }

            w.WriteEndArray();
        }

        return System.Text.Encoding.UTF8.GetString(stream.ToArray());
    }

    /// <summary>Reads what <see cref="Write"/> wrote. Anything that does not read is nothing: a damaged file never stops the app.</summary>
    /// <param name="text">The text.</param>
    /// <returns>The schedule.</returns>
    public static NotificationSchedule Read(string? text)
    {
        NotificationSchedule schedule = new();
        if (string.IsNullOrWhiteSpace(text))
        {
            return schedule;
        }

        try
        {
            using JsonDocument document = JsonDocument.Parse(text);
            if (document.RootElement.ValueKind != JsonValueKind.Array)
            {
                return schedule;
            }

            foreach (JsonElement item in document.RootElement.EnumerateArray())
            {
                if (item.ValueKind != JsonValueKind.Object
                    || !item.TryGetProperty("id", out JsonElement id) || id.ValueKind != JsonValueKind.String
                    || !item.TryGetProperty("title", out JsonElement title) || title.ValueKind != JsonValueKind.String
                    || !item.TryGetProperty("at", out JsonElement at) || at.ValueKind != JsonValueKind.Number || !at.TryGetInt64(out long unix))
                {
                    continue;
                }

                int? hour = item.TryGetProperty("hour", out JsonElement h) && h.TryGetInt32(out int hourValue) ? hourValue : null;
                int? minute = item.TryGetProperty("minute", out JsonElement m) && m.TryGetInt32(out int minuteValue) ? minuteValue : null;
                if ((hour is null) != (minute is null) || hour is < 0 or > 23 || minute is < 0 or > 59)
                {
                    continue;
                }

                _ = schedule.Add(new PendingNotification(
                    id.GetString()!,
                    title.GetString()!,
                    Text(item, "body"),
                    OpeningRules.IsAppPath(Text(item, "href")) ? Text(item, "href") : null,
                    Text(item, "thread"),
                    DateTimeOffset.FromUnixTimeSeconds(unix),
                    hour,
                    minute));
            }
        }
        catch (Exception problem) when (problem is JsonException or ArgumentOutOfRangeException or InvalidOperationException)
        {
            return new NotificationSchedule();
        }

        return schedule;
    }

    private static string? Text(JsonElement item, string name) =>
        item.TryGetProperty(name, out JsonElement value) && value.ValueKind == JsonValueKind.String ? value.GetString() : null;
}

/// <summary>What is kept of the app's state between launches, and what is not, as plain rules.</summary>
public static class SnapshotRules
{
    /// <summary>
    /// Whether the snapshot the core gave is stored. The core says <c>person_unknown</c> while it holds nothing of a person (before this
    /// launch's shell has answered, or for a person named without an id); a client does not store that, so a visitor's blank
    /// snapshot never replaces a person's.
    /// </summary>
    /// <param name="snapshot">The snapshot's JSON, as the core gave it.</param>
    /// <returns>Whether to store it.</returns>
    public static bool ShouldKeep(string? snapshot)
    {
        if (string.IsNullOrWhiteSpace(snapshot))
        {
            return false;
        }

        try
        {
            using JsonDocument document = JsonDocument.Parse(snapshot);
            JsonElement root = document.RootElement;
            if (root.ValueKind != JsonValueKind.Object)
            {
                return false;
            }

            return !(root.TryGetProperty("person_unknown", out JsonElement unknown) && unknown.ValueKind == JsonValueKind.True);
        }
        catch (JsonException)
        {
            return false;
        }
    }
}

/// <summary>What the core says when it has stopped for good.</summary>
public static class CoreFault
{
    /// <summary>
    /// Whether a problem the client was told of is the core saying it stopped: a failure inside it stops it for good, and every
    /// later call answers an error, so the app must be started again. A call whose input merely did not read is only an error: the core goes on.
    /// </summary>
    /// <param name="problem">The client's own words for what the core answered.</param>
    /// <returns>Whether the core has stopped.</returns>
    public static bool IsStop(string? problem) =>
        problem is not null
        && (problem.Contains("the core stopped after an earlier failure", StringComparison.Ordinal)
            || problem.Contains("the core failed on this", StringComparison.Ordinal));
}
