using System.Collections.Immutable;
using System.Text.Json;
using Navree.Protocol;

namespace Navree.Runtime;

/// <summary>Reads the core's view state, a part at a time, into the immutable values the client draws from.</summary>
public static class ViewReading
{
    /// <summary>Reads the app's part.</summary>
    /// <param name="json">The part.</param>
    /// <returns>The app.</returns>
    public static AppPart ReadApp(JsonElement json)
    {
        Reading.ExpectObject(json);
        return new AppPart
        {
            Name = Reading.Or<string>(json, "name", Reading.Text, string.Empty),
            Context = Reading.Opt<string>(json, "context", Reading.Text),
            Home = Reading.Or<string>(json, "home", Reading.Text, "/"),
            Theme = Reading.Opt<Models.Theme>(json, "theme", Models.ReadTheme),
            User = Reading.Opt<Models.User>(json, "user", Models.ReadUser),
            UserWas = Reading.Opt<string>(json, "user_was", Reading.Text),
        };
    }

    /// <summary>Reads the navigation part, or the whole view state, which holds it.</summary>
    /// <param name="json">The part.</param>
    /// <returns>The navigation.</returns>
    public static NavPart ReadNav(JsonElement json)
    {
        Reading.ExpectObject(json);
        Width window = Width.Compact;
        if (Reading.TryField(json, "window", out JsonElement held))
        {
            window = Reading.Word<Width>(held, "width", ReadWidth, Width.Compact);
        }

        return new NavPart
        {
            Stage = Reading.Word<Stage>(json, "stage", ReadStage, Stage.Starting),
            Failure = Reading.OptWord<Failure>(json, "failure", ReadFailure),
            Upgrade = Reading.Opt<Models.Upgrade>(json, "upgrade", Models.ReadUpgrade),
            Destinations = Reading.Or<IReadOnlyList<Destination>>(json, "destinations", static element => Reading.ListOf<Destination>(element, ReadDestination), []),
            Selected = Reading.Or<int>(json, "selected", Reading.Int, 0),
            Modals = Reading.Or<IReadOnlyList<Modal>>(json, "modals", static element => Reading.ListOf<Modal>(element, ReadModal), []),
            Front = Reading.OptV<long>(json, "front", Reading.Long),
            CanGoBack = Reading.Or<bool>(json, "can_go_back", Reading.Flag, false),
            Window = window,
        };
    }

    /// <summary>Reads one screen.</summary>
    /// <param name="json">The part.</param>
    /// <returns>The screen.</returns>
    public static ScreenView ReadScreen(JsonElement json)
    {
        Reading.ExpectObject(json);
        return new ScreenView
        {
            Id = Reading.Required<long>(json, "id", Reading.Long),
            Url = Reading.Or<string>(json, "url", Reading.Text, string.Empty),
            Content = Reading.Or<ScreenContent>(json, "content", ReadContent, new BlankContent()),
            Loading = Reading.Or<bool>(json, "loading", Reading.Flag, false),
            Refreshing = Reading.Or<bool>(json, "refreshing", Reading.Flag, false),
            Cached = Reading.Or<bool>(json, "cached", Reading.Flag, false),
            Failure = Reading.OptWord<Failure>(json, "failure", ReadFailure),
            Status = Reading.Or<int>(json, "status", Reading.Int, 0),
            Chrome = Reading.Or<bool>(json, "chrome", Reading.Flag, true),
            Overlays = Reading.Or<IReadOnlyList<string>>(json, "overlays", static element => Reading.ListOf<string>(element, Reading.Text), []),
            States = Reading.Or<IReadOnlyDictionary<string, IReadOnlyList<Models.State>>>(
                json,
                "states",
                static element => Reading.MapOf<IReadOnlyList<Models.State>>(element, static states => Reading.ListOf<Models.State>(states, Models.ReadState)),
                ImmutableDictionary<string, IReadOnlyList<Models.State>>.Empty),
            Forms = Reading.Or<IReadOnlyDictionary<string, FormView>>(json, "forms", static element => Reading.MapOf<FormView>(element, ReadForm), ImmutableDictionary<string, FormView>.Empty),
            Regions = Reading.Or<IReadOnlyDictionary<string, RegionView>>(json, "regions", static element => Reading.MapOf<RegionView>(element, ReadRegion), ImmutableDictionary<string, RegionView>.Empty),
            Lists = Reading.Or<IReadOnlyDictionary<string, ListState>>(json, "lists", static element => Reading.MapOf<ListState>(element, ReadList), ImmutableDictionary<string, ListState>.Empty),
            Focus = Reading.Opt<Focus>(json, "focus", ReadFocus),
            Question = Reading.Opt<ScreenQuestion>(json, "question", ReadQuestion),
            Title = Reading.Opt<string>(json, "title", Reading.Text),
            From = Reading.Opt<string>(json, "from", Reading.Text),
        };
    }

    /// <summary>Reads the state of one form.</summary>
    /// <param name="json">The part.</param>
    /// <returns>The form.</returns>
    public static FormView ReadForm(JsonElement json)
    {
        Reading.ExpectObject(json);
        return new FormView
        {
            Pending = Reading.Or<bool>(json, "pending", Reading.Flag, false),
            Fields = Reading.Or<IReadOnlyDictionary<string, FieldView>>(json, "fields", static element => Reading.MapOf<FieldView>(element, ReadField), ImmutableDictionary<string, FieldView>.Empty),
        };
    }

    /// <summary>Reads the messages waiting to be shown.</summary>
    /// <param name="json">The part.</param>
    /// <returns>The messages.</returns>
    public static ImmutableList<Message> ReadMessages(JsonElement json) =>
        ImmutableList.CreateRange(Reading.ListOf<Message>(json, ReadMessage));

    /// <summary>Reads what a field holds.</summary>
    /// <param name="json">The value.</param>
    /// <returns>What it holds.</returns>
    public static FieldValue ReadFieldValue(JsonElement json)
    {
        if (json.ValueKind != JsonValueKind.Object)
        {
            return new NoValue();
        }

        return Reading.Tag(json, "t") switch
        {
            "text" => new TextValue(Reading.Or<string>(json, "text", Reading.Text, string.Empty)),
            "flag" => new FlagValue(Reading.Or<bool>(json, "on", Reading.Flag, false)),
            "many" => new ManyValue(Reading.Or<IReadOnlyList<string>>(json, "values", static element => Reading.ListOf<string>(element, Reading.Text), [])),
            "files" => new FilesValue(Reading.Or<IReadOnlyList<FileRef>>(json, "files", static element => Reading.ListOf<FileRef>(element, ReadFileRef), [])),
            "range" => new RangeValue(
                Reading.Or<string>(json, "from", Reading.Text, string.Empty),
                Reading.Or<string>(json, "to", Reading.Text, string.Empty)),
            _ => new NoValue(),
        };
    }

    private static Destination ReadDestination(JsonElement json)
    {
        Reading.ExpectObject(json);
        return new Destination
        {
            Label = Reading.Or<string>(json, "label", Reading.Text, string.Empty),
            Icon = Reading.Opt<Models.Icon>(json, "icon", Models.ReadIcon),
            Href = Reading.Or<string>(json, "href", Reading.Text, string.Empty),
            Badge = Reading.Opt<string>(json, "badge", Reading.Text),
            Group = Reading.Opt<string>(json, "group", Reading.Text),
            Items = Reading.Or<IReadOnlyList<Models.NavItem>>(json, "items", static element => Reading.ListOf<Models.NavItem>(element, Models.ReadNavItem), []),
            Stack = Reading.Or<IReadOnlyList<long>>(json, "stack", static element => Reading.ListOf<long>(element, Reading.Long), []),
        };
    }

    private static Modal ReadModal(JsonElement json)
    {
        Reading.ExpectObject(json);
        return new Modal
        {
            Present = Reading.Word<Models.Present>(json, "present", Models.ReadPresent, Models.Present.Sheet),
            Locked = Reading.Or<bool>(json, "locked", Reading.Flag, false),
            Stack = Reading.Or<IReadOnlyList<long>>(json, "stack", static element => Reading.ListOf<long>(element, Reading.Long), []),
        };
    }

    private static ScreenContent ReadContent(JsonElement json)
    {
        if (json.ValueKind != JsonValueKind.Object)
        {
            return new BlankContent();
        }

        switch (Reading.Tag(json, "t"))
        {
            case "native":
                return Reading.TryField(json, "screen", out JsonElement screen)
                    ? new NativeContent(Models.ReadScreen(screen))
                    : new BlankContent();
            case "web":
                return new WebContent(
                    Reading.Or<string>(json, "url", Reading.Text, string.Empty),
                    Reading.Opt<string>(json, "title", Reading.Text),
                    Reading.Or<IReadOnlyList<string>>(json, "why", static element => Reading.ListOf<string>(element, Reading.Text), []),
                    Reading.Or<long>(json, "serial", Reading.Long, 0));
            default:
                return new BlankContent();
        }
    }

    private static FileRef ReadFileRef(JsonElement json)
    {
        Reading.ExpectObject(json);
        return new FileRef(
            Reading.Required<string>(json, "token", Reading.Text),
            Reading.Required<string>(json, "name", Reading.Text),
            Reading.Or<string>(json, "type", Reading.Text, string.Empty),
            Reading.Or<long>(json, "size", Reading.Long, 0));
    }

    private static FieldView ReadField(JsonElement json)
    {
        Reading.ExpectObject(json);
        return new FieldView
        {
            Name = Reading.Or<string>(json, "name", Reading.Text, string.Empty),
            Value = Reading.Or<FieldValue>(json, "value", ReadFieldValue, new NoValue()),
            Errors = Reading.Or<IReadOnlyList<string>>(json, "errors", static element => Reading.ListOf<string>(element, Reading.Text), []),
            Options = Reading.Opt<IReadOnlyList<Models.ChoiceOption>>(json, "options", static element => Reading.ListOf<Models.ChoiceOption>(element, Models.ReadChoiceOption)),
            More = Reading.Or<bool>(json, "more", Reading.Flag, false),
            Searching = Reading.Or<bool>(json, "searching", Reading.Flag, false),
        };
    }

    private static RegionView ReadRegion(JsonElement json)
    {
        Reading.ExpectObject(json);
        return new RegionView(
            Reading.Or<bool>(json, "loading", Reading.Flag, false),
            Reading.OptWord<Failure>(json, "failure", ReadFailure));
    }

    private static ListState ReadList(JsonElement json)
    {
        Reading.ExpectObject(json);
        return new ListState
        {
            Loading = Reading.Or<bool>(json, "loading", Reading.Flag, false),
            Failed = Reading.Or<bool>(json, "failed", Reading.Flag, false),
            LoadingBefore = Reading.Or<bool>(json, "loading_before", Reading.Flag, false),
            FailedBefore = Reading.Or<bool>(json, "failed_before", Reading.Flag, false),
            Across = Reading.Or<bool>(json, "across", Reading.Flag, false),
        };
    }

    private static Focus ReadFocus(JsonElement json)
    {
        Reading.ExpectObject(json);
        return new Focus(
            Reading.Required<string>(json, "target", Reading.Text),
            Reading.Required<long>(json, "serial", Reading.Long));
    }

    private static ScreenQuestion ReadQuestion(JsonElement json)
    {
        Reading.ExpectObject(json);
        return new ScreenQuestion(
            Reading.Required<long>(json, "id", Reading.Long),
            Reading.Or<string>(json, "title", Reading.Text, string.Empty),
            Reading.Opt<string>(json, "text", Reading.Text),
            Reading.Or<string>(json, "confirm", Reading.Text, string.Empty),
            Reading.Opt<string>(json, "cancel", Reading.Text));
    }

    private static Message ReadMessage(JsonElement json)
    {
        Reading.ExpectObject(json);
        return new Message
        {
            Id = Reading.Required<long>(json, "id", Reading.Long),
            Tone = Reading.Word<Models.Tone>(json, "tone", Models.ReadTone, Models.Tone.Neutral),
            Text = Reading.Opt<string>(json, "text", Reading.Text),
            Title = Reading.Opt<string>(json, "title", Reading.Text),
            Notice = Reading.OptWord<Notice>(json, "notice", ReadNotice),
            Action = Reading.Opt<string>(json, "action", Reading.Text),
        };
    }

    /// <summary>Reads a notice's word.</summary>
    /// <param name="json">The value.</param>
    /// <returns>The notice, or <see cref="Notice.Unknown"/>.</returns>
    public static Notice ReadNotice(JsonElement json) =>
        json.ValueKind == JsonValueKind.String
            ? json.GetString() switch
            {
                "offline" => Notice.Offline,
                "timeout" => Notice.Timeout,
                "failed" => Notice.Failed,
                "unsaved" => Notice.Unsaved,
                "loading" => Notice.Loading,
                "copied" => Notice.Copied,
                "fields_need_attention" => Notice.FieldsNeedAttention,
                "loaded" => Notice.Loaded,
                "second_step" => Notice.SecondStep,
                "saved_not_refreshed" => Notice.SavedNotRefreshed,
                _ => Notice.Unknown,
            }
            : Notice.Unknown;

    /// <summary>The core's word for a notice: what <see cref="ReadNotice"/> read it from.</summary>
    /// <param name="notice">The notice.</param>
    /// <returns>The word, or nothing for a notice this build does not know.</returns>
    public static string NoticeWord(Notice notice) =>
        notice switch
        {
            Notice.Offline => "offline",
            Notice.Timeout => "timeout",
            Notice.Failed => "failed",
            Notice.Unsaved => "unsaved",
            Notice.Loading => "loading",
            Notice.Copied => "copied",
            Notice.FieldsNeedAttention => "fields_need_attention",
            Notice.Loaded => "loaded",
            Notice.SecondStep => "second_step",
            Notice.SavedNotRefreshed => "saved_not_refreshed",
            _ => string.Empty,
        };

    private static Stage ReadStage(JsonElement json) =>
        json.ValueKind == JsonValueKind.String
            ? json.GetString() switch
            {
                "starting" => Stage.Starting,
                "ready" => Stage.Ready,
                "failed" => Stage.Failed,
                "upgrade" => Stage.Upgrade,
                _ => Stage.Unknown,
            }
            : Stage.Unknown;

    private static Failure ReadFailure(JsonElement json) =>
        json.ValueKind == JsonValueKind.String
            ? json.GetString() switch
            {
                "offline" => Failure.Offline,
                "timeout" => Failure.Timeout,
                "server" => Failure.Server,
                "protocol" => Failure.Protocol,
                "config" => Failure.Config,
                _ => Failure.Unknown,
            }
            : Failure.Unknown;

    private static Width ReadWidth(JsonElement json) =>
        json.ValueKind == JsonValueKind.String
            ? json.GetString() switch
            {
                "compact" => Width.Compact,
                "medium" => Width.Medium,
                "expanded" => Width.Expanded,
                _ => Width.Unknown,
            }
            : Width.Unknown;
}
