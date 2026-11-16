using System.Text;
using System.Text.Json;
using Navree.Protocol;

namespace Navree.Runtime;

/// <summary>
/// Writes the view state's values as JSON: what a field holds, for the event that
/// tells the core it changed, and the whole state in one fixed form (keys in order),
/// so two states can be compared by comparing what is written.
/// </summary>
public static class ViewWriting
{
    /// <summary>Writes what a field holds.</summary>
    /// <param name="w">Where to write.</param>
    /// <param name="value">What it holds.</param>
    public static void WriteFieldValue(Utf8JsonWriter w, FieldValue value)
    {
        w.WriteStartObject();
        switch (value)
        {
            case TextValue text:
                w.WriteString("t", "text");
                w.WriteString("text", text.Text);
                break;
            case FlagValue flag:
                w.WriteString("t", "flag");
                w.WriteBoolean("on", flag.On);
                break;
            case ManyValue many:
                w.WriteString("t", "many");
                w.WriteStartArray("values");
                foreach (string item in many.Values)
                {
                    w.WriteStringValue(item);
                }

                w.WriteEndArray();
                break;
            case FilesValue files:
                w.WriteString("t", "files");
                w.WriteStartArray("files");
                foreach (FileRef file in files.Files)
                {
                    w.WriteStartObject();
                    w.WriteString("token", file.Token);
                    w.WriteString("name", file.Name);
                    w.WriteString("type", file.MediaType);
                    w.WriteNumber("size", file.Size);
                    w.WriteEndObject();
                }

                w.WriteEndArray();
                break;
            case RangeValue range:
                w.WriteString("t", "range");
                w.WriteString("from", range.From);
                w.WriteString("to", range.To);
                break;
            default:
                w.WriteString("t", "none");
                break;
        }

        w.WriteEndObject();
    }

    /// <summary>Writes the whole view state in one fixed form.</summary>
    /// <param name="state">The state.</param>
    /// <returns>JSON text.</returns>
    public static string ToJson(ViewState state)
    {
        using MemoryStream stream = new();
        using (Utf8JsonWriter w = new(stream))
        {
            w.WriteStartObject();
            if (state.App is not null)
            {
                w.WritePropertyName("app");
                WriteApp(w, state.App);
            }

            w.WritePropertyName("nav");
            WriteNav(w, state.Nav);
            w.WriteStartObject("screens");
            foreach (long id in state.Screens.Keys.Order())
            {
                w.WritePropertyName(id.ToString(System.Globalization.CultureInfo.InvariantCulture));
                WriteScreen(w, state.Screens[id]);
            }

            w.WriteEndObject();
            w.WriteStartArray("messages");
            foreach (Message message in state.Messages)
            {
                WriteMessage(w, message);
            }

            w.WriteEndArray();
            w.WriteEndObject();
        }

        return Encoding.UTF8.GetString(stream.ToArray());
    }

    private static void WriteApp(Utf8JsonWriter w, AppPart app)
    {
        w.WriteStartObject();
        w.WriteString("name", app.Name);
        if (app.Context is not null)
        {
            w.WriteString("context", app.Context);
        }

        w.WriteString("home", app.Home);
        if (app.Theme is not null)
        {
            w.WritePropertyName("theme");
            Models.WriteTheme(w, app.Theme);
        }

        if (app.User is not null)
        {
            w.WritePropertyName("user");
            Models.WriteUser(w, app.User);
        }

        w.WriteEndObject();
    }

    private static void WriteNav(Utf8JsonWriter w, NavPart nav)
    {
        w.WriteStartObject();
        w.WriteNumber("stage", (int)nav.Stage);
        if (nav.Failure is Failure failure)
        {
            w.WriteNumber("failure", (int)failure);
        }

        if (nav.Upgrade is not null)
        {
            w.WritePropertyName("upgrade");
            Models.WriteUpgrade(w, nav.Upgrade);
        }

        w.WriteStartArray("destinations");
        foreach (Destination destination in nav.Destinations)
        {
            w.WriteStartObject();
            w.WriteString("label", destination.Label);
            if (destination.Icon is not null)
            {
                w.WritePropertyName("icon");
                Models.WriteIcon(w, destination.Icon);
            }

            w.WriteString("href", destination.Href);
            if (destination.Badge is not null)
            {
                w.WriteString("badge", destination.Badge);
            }

            if (destination.Group is not null)
            {
                w.WriteString("group", destination.Group);
            }

            w.WriteStartArray("items");
            foreach (Models.NavItem item in destination.Items)
            {
                Models.WriteNavItem(w, item);
            }

            w.WriteEndArray();
            WriteNumbers(w, "stack", destination.Stack);
            w.WriteEndObject();
        }

        w.WriteEndArray();
        w.WriteNumber("selected", nav.Selected);
        w.WriteStartArray("modals");
        foreach (Modal modal in nav.Modals)
        {
            w.WriteStartObject();
            w.WriteString("present", Models.NamePresent(modal.Present));
            w.WriteBoolean("locked", modal.Locked);
            WriteNumbers(w, "stack", modal.Stack);
            w.WriteEndObject();
        }

        w.WriteEndArray();
        if (nav.Front is long front)
        {
            w.WriteNumber("front", front);
        }

        w.WriteBoolean("can_go_back", nav.CanGoBack);
        w.WriteNumber("window", (int)nav.Window);
        w.WriteEndObject();
    }

    private static void WriteScreen(Utf8JsonWriter w, ScreenView screen)
    {
        w.WriteStartObject();
        w.WriteNumber("id", screen.Id);
        w.WriteString("url", screen.Url);
        w.WritePropertyName("content");
        switch (screen.Content)
        {
            case NativeContent native:
                w.WriteStartObject();
                w.WriteString("t", "native");
                w.WritePropertyName("screen");
                Models.WriteScreen(w, native.Screen);
                w.WriteEndObject();
                break;
            case WebContent web:
                w.WriteStartObject();
                w.WriteString("t", "web");
                w.WriteString("url", web.Url);
                if (web.Title is not null)
                {
                    w.WriteString("title", web.Title);
                }

                w.WriteStartArray("why");
                foreach (string why in web.Why)
                {
                    w.WriteStringValue(why);
                }

                w.WriteEndArray();
                w.WriteNumber("serial", web.Serial);
                w.WriteEndObject();
                break;
            default:
                w.WriteStartObject();
                w.WriteString("t", "blank");
                w.WriteEndObject();
                break;
        }

        w.WriteBoolean("loading", screen.Loading);
        w.WriteBoolean("refreshing", screen.Refreshing);
        w.WriteBoolean("cached", screen.Cached);
        if (screen.Failure is Failure failure)
        {
            w.WriteNumber("failure", (int)failure);
        }

        w.WriteNumber("status", screen.Status);
        w.WriteBoolean("chrome", screen.Chrome);
        w.WriteStartArray("overlays");
        foreach (string overlay in screen.Overlays)
        {
            w.WriteStringValue(overlay);
        }

        w.WriteEndArray();
        w.WriteStartObject("states");
        foreach (string id in screen.States.Keys.Order(StringComparer.Ordinal))
        {
            w.WriteStartArray(id);
            foreach (Models.State state in screen.States[id])
            {
                w.WriteStringValue(Models.NameState(state));
            }

            w.WriteEndArray();
        }

        w.WriteEndObject();
        w.WriteStartObject("forms");
        foreach (string id in screen.Forms.Keys.Order(StringComparer.Ordinal))
        {
            w.WritePropertyName(id);
            WriteForm(w, screen.Forms[id]);
        }

        w.WriteEndObject();
        w.WriteStartObject("regions");
        foreach (string id in screen.Regions.Keys.Order(StringComparer.Ordinal))
        {
            RegionView region = screen.Regions[id];
            w.WriteStartObject(id);
            w.WriteBoolean("loading", region.Loading);
            if (region.Failure is Failure failed)
            {
                w.WriteNumber("failure", (int)failed);
            }

            w.WriteEndObject();
        }

        w.WriteEndObject();
        w.WriteStartObject("lists");
        foreach (string id in screen.Lists.Keys.Order(StringComparer.Ordinal))
        {
            ListState list = screen.Lists[id];
            w.WriteStartObject(id);
            w.WriteBoolean("loading", list.Loading);
            w.WriteBoolean("failed", list.Failed);
            w.WriteBoolean("loading_before", list.LoadingBefore);
            w.WriteBoolean("failed_before", list.FailedBefore);
            w.WriteBoolean("across", list.Across);
            w.WriteEndObject();
        }

        w.WriteEndObject();
        if (screen.Focus is not null)
        {
            w.WriteStartObject("focus");
            w.WriteString("target", screen.Focus.Target);
            w.WriteNumber("serial", screen.Focus.Serial);
            w.WriteEndObject();
        }

        if (screen.Question is not null)
        {
            w.WriteStartObject("question");
            w.WriteNumber("id", screen.Question.Id);
            w.WriteString("title", screen.Question.Title);
            if (screen.Question.Text is not null)
            {
                w.WriteString("text", screen.Question.Text);
            }

            w.WriteString("confirm", screen.Question.Confirm);
            if (screen.Question.Cancel is not null)
            {
                w.WriteString("cancel", screen.Question.Cancel);
            }

            w.WriteEndObject();
        }

        if (screen.Title is not null)
        {
            w.WriteString("title", screen.Title);
        }

        if (screen.From is not null)
        {
            w.WriteString("from", screen.From);
        }

        w.WriteEndObject();
    }

    private static void WriteForm(Utf8JsonWriter w, FormView form)
    {
        w.WriteStartObject();
        w.WriteBoolean("pending", form.Pending);
        w.WriteStartObject("fields");
        foreach (string id in form.Fields.Keys.Order(StringComparer.Ordinal))
        {
            FieldView field = form.Fields[id];
            w.WriteStartObject(id);
            w.WriteString("name", field.Name);
            w.WritePropertyName("value");
            WriteFieldValue(w, field.Value);
            w.WriteStartArray("errors");
            foreach (string error in field.Errors)
            {
                w.WriteStringValue(error);
            }

            w.WriteEndArray();
            if (field.Options is not null)
            {
                w.WriteStartArray("options");
                foreach (Models.ChoiceOption option in field.Options)
                {
                    Models.WriteChoiceOption(w, option);
                }

                w.WriteEndArray();
            }

            w.WriteBoolean("more", field.More);
            w.WriteBoolean("searching", field.Searching);
            w.WriteEndObject();
        }

        w.WriteEndObject();
        w.WriteEndObject();
    }

    private static void WriteMessage(Utf8JsonWriter w, Message message)
    {
        w.WriteStartObject();
        w.WriteNumber("id", message.Id);
        w.WriteString("tone", Models.NameTone(message.Tone));
        if (message.Text is not null)
        {
            w.WriteString("text", message.Text);
        }

        if (message.Title is not null)
        {
            w.WriteString("title", message.Title);
        }

        if (message.Notice is Notice notice)
        {
            w.WriteNumber("notice", (int)notice);
        }

        if (message.Action is not null)
        {
            w.WriteString("action", message.Action);
        }

        w.WriteEndObject();
    }

    private static void WriteNumbers(Utf8JsonWriter w, string name, IReadOnlyList<long> numbers)
    {
        w.WriteStartArray(name);
        foreach (long number in numbers)
        {
            w.WriteNumberValue(number);
        }

        w.WriteEndArray();
    }
}
