using System.Text.Json;
using Navree.Protocol;

namespace Navree.Runtime;

/// <summary>A part of a multipart body, in order.</summary>
public interface BodyPart
{
}

/// <summary>A field of text.</summary>
/// <param name="Name">The field's name.</param>
/// <param name="Value">Its text.</param>
public sealed record TextPart(string Name, string Value) : BodyPart;

/// <summary>A file; its bytes are the ones the client keeps under <paramref name="Token"/>.</summary>
/// <param name="Name">The field's name.</param>
/// <param name="Token">What the client keeps the file under.</param>
/// <param name="Filename">The file's name.</param>
/// <param name="MediaType">Its type.</param>
public sealed record FilePart(string Name, string Token, string Filename, string MediaType) : BodyPart;

/// <summary>What a request sends.</summary>
public interface RequestBody
{
}

/// <summary>Sent as the body bytes, even when empty; the content type is in the headers.</summary>
/// <param name="Text">The body.</param>
public sealed record FormBody(string Text) : RequestBody;

/// <summary>JSON, sent as the body bytes; the content type is in the headers.</summary>
/// <param name="Text">The body.</param>
public sealed record JsonBody(string Text) : RequestBody;

/// <summary>A multipart body.</summary>
/// <param name="Parts">Its parts, in order.</param>
public sealed record MultipartBody(IReadOnlyList<BodyPart> Parts) : RequestBody;

/// <summary>
/// What the core asks the client to do, read from the core's JSON. An effect this
/// build does not know is an <see cref="UnknownEffect"/>, reported and passed over.
/// </summary>
public interface Effect
{
}

/// <summary>Sends a request with its cookies and follows no redirect; the answer is a <c>response</c> event.</summary>
/// <param name="Request">Its number.</param>
/// <param name="Method">get, post or delete.</param>
/// <param name="Url">A whole address on one of the app's origins.</param>
/// <param name="Headers">Its headers, as the core wrote them.</param>
/// <param name="Body">What it sends.</param>
/// <param name="TimeoutMs">How long to wait.</param>
/// <param name="Xsrf">Whether the session's token goes in the xsrf header too.</param>
public sealed record FetchEffect(
    long Request,
    string Method,
    string Url,
    IReadOnlyList<KeyValuePair<string, string>> Headers,
    RequestBody? Body,
    long TimeoutMs,
    bool Xsrf) : Effect;

/// <summary>May drop a request: its answer is no longer wanted.</summary>
/// <param name="Request">Its number.</param>
public sealed record CancelEffect(long Request) : Effect;

/// <summary>Opens an address outside the app.</summary>
/// <param name="Url">The address.</param>
/// <param name="Inside">In the system's browser inside the app.</param>
public sealed record OpenUrlEffect(string Url, bool Inside) : Effect;

/// <summary>Opens the system's sign-in session.</summary>
/// <param name="Url">Where it starts.</param>
/// <param name="Callback">Where it comes back.</param>
/// <param name="Ephemeral">Whether it shares nothing with the system's browser.</param>
public sealed record SignInEffect(string Url, string? Callback, bool Ephemeral) : Effect;

/// <summary>A message to show; the same list is the view state's messages.</summary>
/// <param name="Id">The message's number.</param>
public sealed record ShowEffect(long Id) : Effect;

/// <summary>Plays a haptic.</summary>
/// <param name="Kind">Which.</param>
public sealed record HapticEffect(string Kind) : Effect;

/// <summary>A device call.</summary>
/// <param name="Call">Its number.</param>
/// <param name="Device">The protocol's call as JSON.</param>
/// <param name="Posts">Whether its result is answered with an event.</param>
/// <param name="Options">What the server gave for the call to start with.</param>
public sealed record DeviceEffect(long Call, JsonElement Device, bool Posts, JsonElement? Options) : Effect;

/// <summary>Reads what is kept under a key; answered with a <c>cache</c> event.</summary>
/// <param name="Key">The key.</param>
public sealed record CacheReadEffect(string Key) : Effect;

/// <summary>Keeps a value under a key; without a value, takes the key away.</summary>
/// <param name="Key">The key.</param>
/// <param name="Value">The value.</param>
public sealed record CacheWriteEffect(string Key, string? Value) : Effect;

/// <summary>Throws away everything kept.</summary>
public sealed record CacheClearEffect : Effect;

/// <summary>Says something to assistive technology.</summary>
/// <param name="Text">What to say.</param>
/// <param name="Notice">A message of the core's own to word.</param>
/// <param name="Count">A count the notice is about.</param>
public sealed record AnnounceEffect(string? Text, Notice? Notice, int? Count) : Effect;

/// <summary>Puts text on the clipboard.</summary>
/// <param name="Text">The text.</param>
/// <param name="Sensitive">As a secret the system should not show or sync.</param>
public sealed record CopyEffect(string Text, bool Sensitive) : Effect;

/// <summary>Answers with a <c>timer</c> event after a while.</summary>
/// <param name="Timer">Its number.</param>
/// <param name="Ms">How long.</param>
public sealed record TimerEffect(long Timer, long Ms) : Effect;

/// <summary>A timer is no longer wanted.</summary>
/// <param name="Timer">Its number.</param>
public sealed record CancelTimerEffect(long Timer) : Effect;

/// <summary>Opens a stream of server-sent events.</summary>
/// <param name="Stream">Its number.</param>
/// <param name="Url">A whole address on one of the app's origins.</param>
/// <param name="Headers">Its headers.</param>
public sealed record StreamOpenEffect(long Stream, string Url, IReadOnlyList<KeyValuePair<string, string>> Headers) : Effect;

/// <summary>Closes a stream.</summary>
/// <param name="Stream">Its number.</param>
public sealed record StreamCloseEffect(long Stream) : Effect;

/// <summary>There is nothing to go back to: the system takes the back.</summary>
public sealed record ExitEffect : Effect;

/// <summary>Something the app's developers will want to know.</summary>
/// <param name="What">What kind of thing.</param>
/// <param name="Detail">Its detail.</param>
public sealed record ReportEffect(string What, string Detail) : Effect;

/// <summary>Secret bytes from the system's secure generator, answered at once and never logged.</summary>
/// <param name="Request">Its number.</param>
/// <param name="Bytes">How many.</param>
public sealed record RandomEffect(long Request, int Bytes) : Effect;

/// <summary>The person signed out: every cookie and all web storage of the app go, and the stored snapshot.</summary>
public sealed record ForgetSessionEffect : Effect;

/// <summary>
/// The session is ending and the core has not let go of the person yet. Just before this, in the same update, the core asked the server for
/// one last thing (to forget what it holds for this device), and it sends <see cref="ForgetSessionEffect"/> when that is answered. The client
/// begins the sign-out at this effect, without waiting for the answer: an app that is ended while the core waits has then begun, and finishes
/// at its next start.
/// </summary>
public sealed record SessionEndingEffect : Effect;

/// <summary>An effect this build does not know.</summary>
/// <param name="Tag">Its name.</param>
public sealed record UnknownEffect(string Tag) : Effect;

/// <summary>Reads effects out of the core's JSON.</summary>
public static class EffectReading
{
    /// <summary>Reads one effect.</summary>
    /// <param name="json">The effect.</param>
    /// <returns>The effect, or an <see cref="UnknownEffect"/>.</returns>
    public static Effect Read(JsonElement json)
    {
        if (json.ValueKind != JsonValueKind.Object)
        {
            return new UnknownEffect(string.Empty);
        }

        string tag = Reading.Tag(json, "t");
        return tag switch
        {
            "fetch" => new FetchEffect(
                Long(json, "request", 0),
                Text(json, "method", "get"),
                Text(json, "url", string.Empty),
                Pairs(json, "headers"),
                ReadBody(json),
                Long(json, "timeout_ms", 30_000),
                Flag(json, "xsrf")),
            "cancel" => new CancelEffect(Long(json, "request", 0)),
            "open_url" => new OpenUrlEffect(Text(json, "url", string.Empty), Flag(json, "inside")),
            "sign_in" => new SignInEffect(Text(json, "url", string.Empty), Reading.Opt<string>(json, "callback", Reading.Text), Flag(json, "ephemeral")),
            "message" => new ShowEffect(Long(json, "id", 0)),
            "haptic" => new HapticEffect(Text(json, "kind", "light")),
            "device" => new DeviceEffect(
                Long(json, "call", 0),
                Reading.TryField(json, "device", out JsonElement device) ? device.Clone() : default,
                Flag(json, "posts"),
                Reading.OptV<JsonElement>(json, "options", Reading.Any)),
            "cache_read" => new CacheReadEffect(Text(json, "key", string.Empty)),
            "cache_write" => new CacheWriteEffect(Text(json, "key", string.Empty), Reading.Opt<string>(json, "value", Reading.Text)),
            "cache_clear" => new CacheClearEffect(),
            "announce" => new AnnounceEffect(
                Reading.Opt<string>(json, "text", Reading.Text),
                Reading.OptWord<Notice>(json, "notice", ViewReading.ReadNotice),
                Reading.OptV<int>(json, "count", Reading.Int)),
            "copy" => new CopyEffect(Text(json, "text", string.Empty), Flag(json, "sensitive")),
            "timer" => new TimerEffect(Long(json, "timer", 0), Long(json, "ms", 0)),
            "cancel_timer" => new CancelTimerEffect(Long(json, "timer", 0)),
            "stream_open" => new StreamOpenEffect(Long(json, "stream", 0), Text(json, "url", string.Empty), Pairs(json, "headers")),
            "stream_close" => new StreamCloseEffect(Long(json, "stream", 0)),
            "exit" => new ExitEffect(),
            "report" => new ReportEffect(Text(json, "what", string.Empty), Text(json, "detail", string.Empty)),
            "random" => new RandomEffect(Long(json, "request", 0), (int)Long(json, "bytes", 48)),
            "forget_session" => new ForgetSessionEffect(),
            "session_ending" => new SessionEndingEffect(),
            _ => new UnknownEffect(tag),
        };
    }

    private static string Text(JsonElement json, string name, string fallback) => Reading.Or<string>(json, name, Reading.Text, fallback);

    private static long Long(JsonElement json, string name, long fallback) => Reading.Or<long>(json, name, Reading.Long, fallback);

    private static bool Flag(JsonElement json, string name) => Reading.Or<bool>(json, name, Reading.Flag, false);

    private static List<KeyValuePair<string, string>> Pairs(JsonElement json, string name)
    {
        List<KeyValuePair<string, string>> pairs = [];
        if (Reading.TryField(json, name, out JsonElement list) && list.ValueKind == JsonValueKind.Array)
        {
            foreach (JsonElement pair in list.EnumerateArray())
            {
                if (pair.ValueKind == JsonValueKind.Array
                    && pair.GetArrayLength() >= 1
                    && pair[0].ValueKind == JsonValueKind.String)
                {
                    string value = pair.GetArrayLength() >= 2 && pair[1].ValueKind == JsonValueKind.String
                        ? pair[1].GetString() ?? string.Empty
                        : string.Empty;
                    pairs.Add(new KeyValuePair<string, string>(pair[0].GetString() ?? string.Empty, value));
                }
            }
        }

        return pairs;
    }

    private static RequestBody? ReadBody(JsonElement json)
    {
        if (!Reading.TryField(json, "body", out JsonElement body) || body.ValueKind != JsonValueKind.Object)
        {
            return null;
        }

        switch (Reading.Tag(body, "t"))
        {
            case "form":
                return new FormBody(Text(body, "text", string.Empty));
            case "json":
                return new JsonBody(Text(body, "text", string.Empty));
            case "multipart":
                List<BodyPart> parts = [];
                if (Reading.TryField(body, "parts", out JsonElement list) && list.ValueKind == JsonValueKind.Array)
                {
                    foreach (JsonElement part in list.EnumerateArray())
                    {
                        if (part.ValueKind != JsonValueKind.Object)
                        {
                            continue;
                        }

                        switch (Reading.Tag(part, "t"))
                        {
                            case "text":
                                parts.Add(new TextPart(Text(part, "name", string.Empty), Text(part, "value", string.Empty)));
                                break;
                            case "file":
                                parts.Add(new FilePart(
                                    Text(part, "name", string.Empty),
                                    Text(part, "token", string.Empty),
                                    Text(part, "filename", string.Empty),
                                    Text(part, "type", "application/octet-stream")));
                                break;
                            default:
                                break;
                        }
                    }
                }

                return new MultipartBody(parts);
            default:
                return null;
        }
    }
}
