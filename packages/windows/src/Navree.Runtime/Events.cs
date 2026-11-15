using System.Text;
using System.Text.Json;
using Navree.Protocol;

namespace Navree.Runtime;

/// <summary>
/// The events a client tells the core, one for each thing that happened. Each is the
/// JSON the core reads, as text; the names and fields are the core's.
/// </summary>
public static class Events
{
    /// <summary>The app started, or the person asked to try again after it failed to.</summary>
    public static string Start { get; } = Make("start", null);

    /// <summary>The system's back.</summary>
    public static string Back { get; } = Make("back", null);

    /// <summary>The app came to the front.</summary>
    public static string Foreground { get; } = Make("foreground", null);

    /// <summary>The app left the front.</summary>
    public static string Background { get; } = Make("background", null);

    /// <summary>The client ended the session itself.</summary>
    public static string SignedOut { get; } = Make("signed_out", null);

    /// <summary>The person agreed to get the newer app.</summary>
    public static string OpenUpgrade { get; } = Make("open_upgrade", null);

    /// <summary>The person chose a top-level destination.</summary>
    /// <param name="destination">Its index.</param>
    /// <returns>The event.</returns>
    public static string Select(int destination) => Make("select", w => w.WriteNumber("destination", destination));

    /// <summary>
    /// Something with an action was tapped. <paramref name="form"/> and <paramref name="overlay"/> are
    /// the ids of the nearest form and of the sheet, dialog or popover around it.
    /// </summary>
    /// <param name="screen">The screen.</param>
    /// <param name="action">The action, as it came.</param>
    /// <param name="form">The form around it.</param>
    /// <param name="overlay">The overlay around it.</param>
    /// <param name="text">Text the action is given.</param>
    /// <param name="now">The time in seconds since 1970.</param>
    /// <param name="ask">The question the control asks first.</param>
    /// <param name="from">The id of the part tapped.</param>
    /// <returns>The event.</returns>
    public static string Act(
        long? screen,
        Models.Action action,
        string? form = null,
        string? overlay = null,
        string? text = null,
        long? now = null,
        Models.Question? ask = null,
        string? from = null) =>
        Make("act", w =>
        {
            if (screen is long id)
            {
                w.WriteNumber("screen", id);
            }

            w.WritePropertyName("action");
            Models.WriteAction(w, action);
            Optional(w, "form", form);
            Optional(w, "overlay", overlay);
            Optional(w, "text", text);
            w.WriteNumber("now", now ?? DateTimeOffset.UtcNow.ToUnixTimeSeconds());
            if (ask is not null)
            {
                w.WritePropertyName("ask");
                Models.WriteQuestion(w, ask);
            }

            Optional(w, "from", from);
        });

    /// <summary>The person answered the core's question.</summary>
    /// <param name="screen">The screen.</param>
    /// <param name="question">The question's number.</param>
    /// <param name="yes">Whether it was its confirming button.</param>
    /// <returns>The event.</returns>
    public static string Answer(long screen, long question, bool yes) =>
        Make("answer", w =>
        {
            w.WriteNumber("screen", screen);
            w.WriteNumber("question", question);
            w.WriteBoolean("yes", yes);
        });

    /// <summary>A row of a table that can be chosen was ticked or cleared; without a row, every row.</summary>
    /// <param name="screen">The screen.</param>
    /// <param name="table">The table's id.</param>
    /// <param name="row">The row's id.</param>
    /// <param name="on">Ticked or cleared.</param>
    /// <param name="across">Every record that matches the table's query.</param>
    /// <returns>The event.</returns>
    public static string Choose(long screen, string table, string? row, bool on, bool across = false) =>
        Make("choose", w =>
        {
            w.WriteNumber("screen", screen);
            w.WriteString("table", table);
            Optional(w, "row", row);
            w.WriteBoolean("on", on);
            if (across)
            {
                w.WriteBoolean("across", true);
            }
        });

    /// <summary>A tab of a set of tabs that has an id was chosen.</summary>
    /// <param name="screen">The screen.</param>
    /// <param name="id">The tabs' id.</param>
    /// <param name="index">The tab's index.</param>
    /// <returns>The event.</returns>
    public static string Tab(long screen, string id, int index) =>
        Make("tab", w =>
        {
            w.WriteNumber("screen", screen);
            w.WriteString("id", id);
            w.WriteNumber("index", index);
        });

    /// <summary>The answer to a <c>random</c> effect: the bytes as hex. A secret: never logged.</summary>
    /// <param name="request">The request's number.</param>
    /// <param name="hex">The bytes, two characters each.</param>
    /// <returns>The event.</returns>
    public static string Random(long request, string hex) =>
        Make("random", w =>
        {
            w.WriteNumber("request", request);
            w.WriteString("bytes", hex);
        });

    /// <summary>The system gave or changed this install's push token.</summary>
    /// <param name="token">The token.</param>
    /// <returns>The event.</returns>
    public static string PushToken(string token) => Make("push_token", w => w.WriteString("token", token));

    /// <summary>The app starts again after the system ended it, with the snapshot it stored, in place of <see cref="Start"/>.</summary>
    /// <param name="snapshot">The snapshot's JSON, as the core gave it.</param>
    /// <returns>The event.</returns>
    public static string Restore(string snapshot) =>
        Make("restore", w =>
        {
            w.WritePropertyName("snapshot");
            w.WriteRawValue(snapshot);
        });

    /// <summary>A row was dragged from one list to a place in another (or the same).</summary>
    /// <param name="screen">The screen.</param>
    /// <param name="item">The row.</param>
    /// <param name="from">The list it came from.</param>
    /// <param name="to">The list it went to.</param>
    /// <param name="before">The row it now stands before.</param>
    /// <returns>The event.</returns>
    public static string Move(long screen, string item, string from, string to, string? before) =>
        Make("move", w =>
        {
            w.WriteNumber("screen", screen);
            w.WriteString("item", item);
            w.WriteString("from", from);
            w.WriteString("to", to);
            Optional(w, "before", before);
        });

    /// <summary>The words a searched choice is searched with changed, or the end of its options is near.</summary>
    /// <param name="screen">The screen.</param>
    /// <param name="form">The form.</param>
    /// <param name="field">The field.</param>
    /// <param name="text">The words.</param>
    /// <param name="more">The end of the options is near.</param>
    /// <returns>The event.</returns>
    public static string Options(long screen, string? form, string field, string? text = null, bool more = false) =>
        Make("options", w =>
        {
            w.WriteNumber("screen", screen);
            Optional(w, "form", form);
            w.WriteString("field", field);
            Optional(w, "text", text);
            if (more)
            {
                w.WriteBoolean("more", true);
            }
        });

    /// <summary>The person changed a field.</summary>
    /// <param name="screen">The screen.</param>
    /// <param name="form">The form.</param>
    /// <param name="field">The field.</param>
    /// <param name="value">What it holds now.</param>
    /// <returns>The event.</returns>
    public static string Field(long screen, string? form, string field, FieldValue value) =>
        Make("field", w =>
        {
            w.WriteNumber("screen", screen);
            Optional(w, "form", form);
            w.WriteString("field", field);
            w.WritePropertyName("value");
            ViewWriting.WriteFieldValue(w, value);
        });

    /// <summary>The person searched with the screen's own search.</summary>
    /// <param name="screen">The screen.</param>
    /// <param name="text">The words.</param>
    /// <returns>The event.</returns>
    public static string Search(long screen, string text) =>
        Make("search", w =>
        {
            w.WriteNumber("screen", screen);
            w.WriteString("text", text);
        });

    /// <summary>The person went back to a screen further down the stack.</summary>
    /// <param name="screen">The screen.</param>
    /// <returns>The event.</returns>
    public static string PopTo(long screen) => Make("pop_to", w => w.WriteNumber("screen", screen));

    /// <summary>The person took a sheet or cover away.</summary>
    /// <param name="screen">The screen.</param>
    /// <returns>The event.</returns>
    public static string Dismissed(long screen) => Make("dismissed", w => w.WriteNumber("screen", screen));

    /// <summary>The person opened or closed a sheet, dialog, popover or disclosure.</summary>
    /// <param name="screen">The screen.</param>
    /// <param name="id">Its id.</param>
    /// <param name="open">Open or closed.</param>
    /// <returns>The event.</returns>
    public static string Overlay(long screen, string id, bool open) =>
        Make("overlay", w =>
        {
            w.WriteNumber("screen", screen);
            w.WriteString("id", id);
            w.WriteBoolean("open", open);
        });

    /// <summary>Pull to refresh, or "try again".</summary>
    /// <param name="screen">The screen.</param>
    /// <returns>The event.</returns>
    public static string Refresh(long screen) => Make("refresh", w => w.WriteNumber("screen", screen));

    /// <summary>The person asked for a region that failed again.</summary>
    /// <param name="screen">The screen.</param>
    /// <param name="region">The region's id.</param>
    /// <returns>The event.</returns>
    public static string RegionRetry(long screen, string region) =>
        Make("region_retry", w =>
        {
            w.WriteNumber("screen", screen);
            w.WriteString("region", region);
        });

    /// <summary>The end of a list or table is near; with <paramref name="before"/>, the start of a list.</summary>
    /// <param name="screen">The screen.</param>
    /// <param name="id">The list's id.</param>
    /// <param name="before">The start, not the end.</param>
    /// <returns>The event.</returns>
    public static string More(long screen, string id, bool before = false) =>
        Make("more", w =>
        {
            w.WriteNumber("screen", screen);
            w.WriteString("id", id);
            if (before)
            {
                w.WriteBoolean("before", true);
            }
        });

    /// <summary>A table goes to another address of itself: a sort, another page.</summary>
    /// <param name="screen">The screen.</param>
    /// <param name="region">The region's id.</param>
    /// <param name="href">The address.</param>
    /// <returns>The event.</returns>
    public static string RegionGo(long screen, string region, string href) =>
        Make("region_go", w =>
        {
            w.WriteNumber("screen", screen);
            w.WriteString("region", region);
            w.WriteString("href", href);
        });

    /// <summary>A message went away.</summary>
    /// <param name="id">The message's number.</param>
    /// <returns>The event.</returns>
    public static string MessageShown(long id) => Make("message_shown", w => w.WriteNumber("id", id));

    /// <summary>A message's button was used.</summary>
    /// <param name="id">The message's number.</param>
    /// <returns>The event.</returns>
    public static string MessageAction(long id) => Make("message_action", w => w.WriteNumber("id", id));

    /// <summary>A link was tapped in the web view.</summary>
    /// <param name="screen">The screen.</param>
    /// <param name="url">The address.</param>
    /// <returns>The event.</returns>
    public static string WebLink(long screen, string url) =>
        Make("web_link", w =>
        {
            w.WriteNumber("screen", screen);
            w.WriteString("url", url);
        });

    /// <summary>The app was opened with an address.</summary>
    /// <param name="url">The address.</param>
    /// <returns>The event.</returns>
    public static string OpenLink(string url) => Make("open_link", w => w.WriteString("url", url));

    /// <summary>A notification was tapped.</summary>
    /// <param name="href">Where it leads.</param>
    /// <returns>The event.</returns>
    public static string Notification(string href) => Make("notification", w => w.WriteString("href", href));

    /// <summary>The window's width changed class.</summary>
    /// <param name="width">The new class.</param>
    /// <returns>The event.</returns>
    public static string Window(Width width) =>
        Make("window", w => w.WriteString("width", width switch
        {
            Width.Medium => "medium",
            Width.Expanded => "expanded",
            _ => "compact",
        }));

    /// <summary>The person's settings changed.</summary>
    /// <param name="dark">Dark, or light.</param>
    /// <param name="contrast">The contrast asked for.</param>
    /// <param name="textPercent">The text size, in percent of the default.</param>
    /// <param name="locale">The locale.</param>
    /// <param name="tz">The time zone.</param>
    /// <returns>The event.</returns>
    public static string Environment(bool? dark = null, string? contrast = null, int? textPercent = null, string? locale = null, string? tz = null) =>
        Make("environment", w =>
        {
            if (dark is bool isDark)
            {
                w.WriteString("scheme", isDark ? "dark" : "light");
            }

            Optional(w, "contrast", contrast);
            if (textPercent is int percent)
            {
                w.WriteNumber("text_percent", percent);
            }

            Optional(w, "locale", locale);
            Optional(w, "tz", tz);
        });

    /// <summary>A timer the core asked for ran out.</summary>
    /// <param name="timer">Its number.</param>
    /// <returns>The event.</returns>
    public static string Timer(long timer) => Make("timer", w => w.WriteNumber("timer", timer));

    /// <summary>A stream opened.</summary>
    /// <param name="stream">Its number.</param>
    /// <returns>The event.</returns>
    public static string StreamOpened(long stream) =>
        Make("stream", w =>
        {
            w.WriteNumber("stream", stream);
            w.WriteString("state", "open");
        });

    /// <summary>A stream spoke.</summary>
    /// <param name="stream">Its number.</param>
    /// <param name="name">The event's name.</param>
    /// <returns>The event.</returns>
    public static string StreamEvent(long stream, string? name) =>
        Make("stream", w =>
        {
            w.WriteNumber("stream", stream);
            w.WriteString("state", "event");
            Optional(w, "name", name);
        });

    /// <summary>A stream ended.</summary>
    /// <param name="stream">Its number.</param>
    /// <param name="status">The status it ended with.</param>
    /// <returns>The event.</returns>
    public static string StreamClosed(long stream, int status) =>
        Make("stream", w =>
        {
            w.WriteNumber("stream", stream);
            w.WriteString("state", "closed");
            w.WriteNumber("status", status);
        });

    /// <summary>A <c>cache_read</c> finished.</summary>
    /// <param name="key">The key.</param>
    /// <param name="value">What was kept, when anything was.</param>
    /// <returns>The event.</returns>
    public static string Cache(string key, string? value) =>
        Make("cache", w =>
        {
            w.WriteString("key", key);
            Optional(w, "value", value);
        });

    /// <summary>A device call that posts its result finished.</summary>
    /// <param name="call">The call's number.</param>
    /// <param name="fields">The result as a form's fields.</param>
    /// <returns>The event.</returns>
    public static string DeviceDone(long call, IEnumerable<KeyValuePair<string, string>> fields) =>
        Make("device_result", w =>
        {
            w.WriteNumber("call", call);
            w.WriteBoolean("ok", true);
            w.WriteStartArray("fields");
            foreach (KeyValuePair<string, string> field in fields)
            {
                w.WriteStartObject();
                w.WriteString("name", field.Key);
                w.WriteString("value", field.Value);
                w.WriteEndObject();
            }

            w.WriteEndArray();
        });

    /// <summary>A device call that posts its result could not be done: one of the protocol's refusals.</summary>
    /// <param name="call">The call's number.</param>
    /// <param name="refusal">denied, cancelled, unsupported or failed.</param>
    /// <returns>The event.</returns>
    public static string DeviceRefused(long call, string refusal) =>
        Make("device_result", w =>
        {
            w.WriteNumber("call", call);
            w.WriteBoolean("ok", false);
            w.WriteString("refusal", refusal);
        });

    /// <summary>The system's sign-in session ended.</summary>
    /// <param name="url">The address it came back to.</param>
    /// <returns>The event.</returns>
    public static string SignInDone(string? url) => Make("sign_in_done", w => Optional(w, "url", url));

    /// <summary>A focus the view state asked for was given.</summary>
    /// <param name="screen">The screen.</param>
    /// <param name="serial">The request's serial.</param>
    /// <returns>The event.</returns>
    public static string Focused(long screen, long serial) =>
        Make("focused", w =>
        {
            w.WriteNumber("screen", screen);
            w.WriteNumber("serial", serial);
        });

    /// <summary>The answer to a fetch, whatever its status. Redirects are handed over as they came.</summary>
    /// <param name="request">The request's number.</param>
    /// <param name="status">The status.</param>
    /// <param name="contentType">Its content type.</param>
    /// <param name="location">Where a redirect leads.</param>
    /// <param name="headers">The headers whose names start with x-navree-, and the cache-control.</param>
    /// <param name="body">The body as text.</param>
    /// <returns>The event.</returns>
    public static string Response(
        long request,
        int status,
        string? contentType,
        string? location,
        IReadOnlyList<KeyValuePair<string, string>> headers,
        string body) =>
        Make("response", w =>
        {
            w.WriteNumber("request", request);
            w.WriteNumber("status", status);
            Optional(w, "content_type", contentType);
            Optional(w, "location", location);
            if (headers.Count > 0)
            {
                w.WriteStartArray("headers");
                foreach (KeyValuePair<string, string> header in headers)
                {
                    w.WriteStartArray();
                    w.WriteStringValue(header.Key);
                    w.WriteStringValue(header.Value);
                    w.WriteEndArray();
                }

                w.WriteEndArray();
            }

            if (body.Length > 0)
            {
                w.WriteString("body", body);
            }
        });

    /// <summary>A fetch that got no answer.</summary>
    /// <param name="request">The request's number.</param>
    /// <param name="error">offline, timeout or other.</param>
    /// <returns>The event.</returns>
    public static string ResponseFailed(long request, string error) =>
        Make("response", w =>
        {
            w.WriteNumber("request", request);
            w.WriteString("error", error);
        });

    private static void Optional(Utf8JsonWriter w, string name, string? value)
    {
        if (value is not null)
        {
            w.WriteString(name, value);
        }
    }

    private static string Make(string t, Action<Utf8JsonWriter>? body)
    {
        using MemoryStream stream = new();
        using (Utf8JsonWriter w = new(stream))
        {
            w.WriteStartObject();
            w.WriteString("t", t);
            body?.Invoke(w);
            w.WriteEndObject();
        }

        return Encoding.UTF8.GetString(stream.ToArray());
    }
}
