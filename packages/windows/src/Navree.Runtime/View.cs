using System.Collections.Immutable;
using System.Text.Json;
using Navree.Protocol;

namespace Navree.Runtime;

/*
 * The runtime core's view state, as the client reads it: everything it draws
 * from, as immutable values. Read as the protocol's models are: a field these
 * do not know is passed over, and a word outside a closed list is read as its
 * field's default.
 */

/// <summary>How far the app has got in starting.</summary>
public enum Stage
{
    /// <summary>A word this build does not know.</summary>
    Unknown,

    /// <summary>The shell has been asked for.</summary>
    Starting,

    /// <summary>The app is running.</summary>
    Ready,

    /// <summary>It could not start.</summary>
    Failed,

    /// <summary>The server needs a newer app.</summary>
    Upgrade,
}

/// <summary>Why something could not be loaded. The words for each are the client's.</summary>
public enum Failure
{
    /// <summary>A word this build does not know.</summary>
    Unknown,

    /// <summary>No network.</summary>
    Offline,

    /// <summary>It took too long.</summary>
    Timeout,

    /// <summary>The server answered with an error.</summary>
    Server,

    /// <summary>The answer was not the protocol's.</summary>
    Protocol,

    /// <summary>The app is set up wrongly.</summary>
    Config,
}

/// <summary>A message of the core's own, worded by the client.</summary>
public enum Notice
{
    /// <summary>A word this build does not know.</summary>
    Unknown,

    /// <summary>No network.</summary>
    Offline,

    /// <summary>It took too long.</summary>
    Timeout,

    /// <summary>It failed.</summary>
    Failed,

    /// <summary>There are unsaved changes.</summary>
    Unsaved,

    /// <summary>Loading.</summary>
    Loading,

    /// <summary>Copied.</summary>
    Copied,

    /// <summary>Some fields need attention.</summary>
    FieldsNeedAttention,

    /// <summary>Loaded.</summary>
    Loaded,

    /// <summary>A second step of signing in follows.</summary>
    SecondStep,

    /// <summary>The change was saved, and the screen could not be brought up to date.</summary>
    SavedNotRefreshed,
}

/// <summary>The window's width class, as the client reported it.</summary>
public enum Width
{
    /// <summary>A word this build does not know.</summary>
    Unknown,

    /// <summary>A narrow window.</summary>
    Compact,

    /// <summary>A middle one.</summary>
    Medium,

    /// <summary>A wide one.</summary>
    Expanded,
}

/// <summary>The app's own part of the chrome.</summary>
public sealed record AppPart
{
    /// <summary>The app's name.</summary>
    public string Name { get; init; } = string.Empty;

    /// <summary>What the app is about, when it says.</summary>
    public string? Context { get; init; }

    /// <summary>The address of the first screen.</summary>
    public string Home { get; init; } = "/";

    /// <summary>The app's colours, when it says.</summary>
    public Models.Theme? Theme { get; init; }

    /// <summary>Who is signed in, when anybody is.</summary>
    public Models.User? User { get; init; }
}

/// <summary>A top-level destination and the screens stacked in it, the first at the bottom.</summary>
public sealed record Destination
{
    /// <summary>What it is called.</summary>
    public string Label { get; init; } = string.Empty;

    /// <summary>Its icon.</summary>
    public Models.Icon? Icon { get; init; }

    /// <summary>Its address.</summary>
    public string Href { get; init; } = string.Empty;

    /// <summary>A count or word shown beside it.</summary>
    public string? Badge { get; init; }

    /// <summary>The group it stands in.</summary>
    public string? Group { get; init; }

    /// <summary>Items under it.</summary>
    public IReadOnlyList<Models.NavItem> Items { get; init; } = [];

    /// <summary>The screens stacked in it, by number.</summary>
    public IReadOnlyList<long> Stack { get; init; } = [];
}

/// <summary>Screens presented over everything else, with a stack of their own.</summary>
public sealed record Modal
{
    /// <summary>How they are presented.</summary>
    public Models.Present Present { get; init; } = Models.Present.Sheet;

    /// <summary>Whether the person can't take them away.</summary>
    public bool Locked { get; init; }

    /// <summary>The screens, by number.</summary>
    public IReadOnlyList<long> Stack { get; init; } = [];
}

/// <summary>Where the person is: everything about navigation, and no screen.</summary>
public sealed record NavPart
{
    /// <summary>How far the app has got in starting.</summary>
    public Stage Stage { get; init; } = Stage.Starting;

    /// <summary>Why it failed, when it did.</summary>
    public Failure? Failure { get; init; }

    /// <summary>What a newer app is needed for.</summary>
    public Models.Upgrade? Upgrade { get; init; }

    /// <summary>The top-level destinations.</summary>
    public IReadOnlyList<Destination> Destinations { get; init; } = [];

    /// <summary>The one chosen.</summary>
    public int Selected { get; init; }

    /// <summary>What is presented over them.</summary>
    public IReadOnlyList<Modal> Modals { get; init; } = [];

    /// <summary>The screen in front.</summary>
    public long? Front { get; init; }

    /// <summary>Whether back leads anywhere inside the app.</summary>
    public bool CanGoBack { get; init; }

    /// <summary>The window's width class.</summary>
    public Width Window { get; init; } = Width.Compact;

    /// <summary>Every screen some stack holds.</summary>
    /// <returns>The numbers.</returns>
    public IReadOnlySet<long> Held()
    {
        HashSet<long> held = [];
        foreach (Destination destination in Destinations)
        {
            held.UnionWith(destination.Stack);
        }

        foreach (Modal modal in Modals)
        {
            held.UnionWith(modal.Stack);
        }

        return held;
    }
}

/// <summary>What a screen shows.</summary>
public interface ScreenContent
{
}

/// <summary>Nothing yet.</summary>
public sealed record BlankContent : ScreenContent;

/// <summary>A tree of parts, ready to draw.</summary>
/// <param name="Screen">The screen.</param>
public sealed record NativeContent(Models.Screen Screen) : ScreenContent;

/// <summary>The page in the web view; load <paramref name="Url"/> again whenever <paramref name="Serial"/> moves on.</summary>
/// <param name="Url">A whole address on one of the app's origins.</param>
/// <param name="Title">The page's title, when it has one.</param>
/// <param name="Why">Why the page is shown here and not drawn.</param>
/// <param name="Serial">A number that moves on to say: load it again.</param>
public sealed record WebContent(string Url, string? Title, IReadOnlyList<string> Why, long Serial) : ScreenContent;

/// <summary>A file the person picked, kept by the client under its own token.</summary>
/// <param name="Token">What the client keeps it under.</param>
/// <param name="Name">The file's name.</param>
/// <param name="MediaType">Its type.</param>
/// <param name="Size">Its size in bytes.</param>
public sealed record FileRef(string Token, string Name, string MediaType, long Size);

/// <summary>What a field holds.</summary>
public interface FieldValue
{
}

/// <summary>Text.</summary>
/// <param name="Text">The text.</param>
public sealed record TextValue(string Text) : FieldValue;

/// <summary>A flag.</summary>
/// <param name="On">Whether it is on.</param>
public sealed record FlagValue(bool On) : FieldValue;

/// <summary>Several choices.</summary>
/// <param name="Values">The values chosen.</param>
public sealed record ManyValue(IReadOnlyList<string> Values) : FieldValue;

/// <summary>Files the person picked.</summary>
/// <param name="Files">The files.</param>
public sealed record FilesValue(IReadOnlyList<FileRef> Files) : FieldValue;

/// <summary>A range of two ends; an end not picked is empty.</summary>
/// <param name="From">The first end.</param>
/// <param name="To">The last end.</param>
public sealed record RangeValue(string From, string To) : FieldValue;

/// <summary>Nothing.</summary>
public sealed record NoValue : FieldValue;

/// <summary>What one field holds and what is wrong with it, and the options a search found for it.</summary>
public sealed record FieldView
{
    /// <summary>The field's name.</summary>
    public string Name { get; init; } = string.Empty;

    /// <summary>What it holds.</summary>
    public FieldValue Value { get; init; } = new NoValue();

    /// <summary>What is wrong with it.</summary>
    public IReadOnlyList<string> Errors { get; init; } = [];

    /// <summary>While present, the option list in place of the input's own.</summary>
    public IReadOnlyList<Models.ChoiceOption>? Options { get; init; }

    /// <summary>The searched list goes on.</summary>
    public bool More { get; init; }

    /// <summary>A search for options is on its way.</summary>
    public bool Searching { get; init; }
}

/// <summary>The state of one form.</summary>
public sealed record FormView
{
    /// <summary>Whether it is being sent.</summary>
    public bool Pending { get; init; }

    /// <summary>Its fields, by id.</summary>
    public IReadOnlyDictionary<string, FieldView> Fields { get; init; } = ImmutableDictionary<string, FieldView>.Empty;
}

/// <summary>The state of a part that fetches itself.</summary>
/// <param name="Loading">Whether it is being fetched.</param>
/// <param name="Failure">Why it failed, when it did.</param>
public sealed record RegionView(bool Loading, Failure? Failure);

/// <summary>The state of a list or table that loads more by cursor, at its end and before its first row.</summary>
public sealed record ListState
{
    /// <summary>Rows are being asked for at the end.</summary>
    public bool Loading { get; init; }

    /// <summary>Asking for rows at the end failed.</summary>
    public bool Failed { get; init; }

    /// <summary>Rows are being asked for before the first.</summary>
    public bool LoadingBefore { get; init; }

    /// <summary>Asking for rows before the first failed.</summary>
    public bool FailedBefore { get; init; }

    /// <summary>Every row of a table is chosen, also those not loaded.</summary>
    public bool Across { get; init; }
}

/// <summary>The confirmation the core asks the person for, in the server's words. Every way out is an answer.</summary>
/// <param name="Id">Its number.</param>
/// <param name="Title">What it asks.</param>
/// <param name="Text">More words.</param>
/// <param name="Confirm">The confirming button's label.</param>
/// <param name="Cancel">The cancelling button's label.</param>
public sealed record ScreenQuestion(long Id, string Title, string? Text, string Confirm, string? Cancel);

/// <summary>A request to move the focus; answered with a <c>focused</c> event.</summary>
/// <param name="Target">The id of the part.</param>
/// <param name="Serial">A number that moves on with each request.</param>
public sealed record Focus(string Target, long Serial);

/// <summary>One screen.</summary>
public sealed record ScreenView
{
    /// <summary>Its number.</summary>
    public required long Id { get; init; }

    /// <summary>Its address, as read.</summary>
    public string Url { get; init; } = string.Empty;

    /// <summary>What it shows.</summary>
    public ScreenContent Content { get; init; } = new BlankContent();

    /// <summary>Whether it is being loaded.</summary>
    public bool Loading { get; init; }

    /// <summary>Whether it is being refreshed.</summary>
    public bool Refreshing { get; init; }

    /// <summary>Whether what it shows came from what is kept.</summary>
    public bool Cached { get; init; }

    /// <summary>Why it failed, when it did.</summary>
    public Failure? Failure { get; init; }

    /// <summary>The answer's status.</summary>
    public int Status { get; init; }

    /// <summary>Whether it has the app's chrome around it.</summary>
    public bool Chrome { get; init; } = true;

    /// <summary>The sheets, dialogs and popovers that are open, the last opened last.</summary>
    public IReadOnlyList<string> Overlays { get; init; } = [];

    /// <summary>The states of parts that have no field for them, by id.</summary>
    public IReadOnlyDictionary<string, IReadOnlyList<Models.State>> States { get; init; } =
        ImmutableDictionary<string, IReadOnlyList<Models.State>>.Empty;

    /// <summary>The state of each form, by id.</summary>
    public IReadOnlyDictionary<string, FormView> Forms { get; init; } = ImmutableDictionary<string, FormView>.Empty;

    /// <summary>The state of each region, by id.</summary>
    public IReadOnlyDictionary<string, RegionView> Regions { get; init; } = ImmutableDictionary<string, RegionView>.Empty;

    /// <summary>The state of each list, by id.</summary>
    public IReadOnlyDictionary<string, ListState> Lists { get; init; } = ImmutableDictionary<string, ListState>.Empty;

    /// <summary>A request to move the focus.</summary>
    public Focus? Focus { get; init; }

    /// <summary>What a control asked before it acts.</summary>
    public ScreenQuestion? Question { get; init; }

    /// <summary>The title of the page the screen last showed.</summary>
    public string? Title { get; init; }

    /// <summary>The id of the part on the screen under it this screen was opened from.</summary>
    public string? From { get; init; }

    /// <summary>The screen of the protocol, when the content is one.</summary>
    public Models.Screen? Screen => (Content as NativeContent)?.Screen;

    /// <summary>Whether a node with this id has the state.</summary>
    /// <param name="id">The node's id.</param>
    /// <param name="state">The state.</param>
    /// <returns>Whether it has.</returns>
    public bool Has(string? id, Models.State state) =>
        id is not null && States.TryGetValue(id, out IReadOnlyList<Models.State>? held) && held.Contains(state);

    /// <summary>Whether an overlay of the tree is open.</summary>
    /// <param name="id">The overlay's id.</param>
    /// <returns>Whether it is.</returns>
    public bool IsOpen(string? id) => id is not null && Overlays.Contains(id);

    /// <summary>What one field holds, in a form.</summary>
    /// <param name="form">The form's id, when it has one.</param>
    /// <param name="field">The field's id.</param>
    /// <returns>The field's state.</returns>
    public FieldView? FieldOf(string? form, string? field) =>
        field is not null && Forms.TryGetValue(form ?? string.Empty, out FormView? view) && view.Fields.TryGetValue(field, out FieldView? held)
            ? held
            : null;
}

/// <summary>A short message waiting to be shown.</summary>
public sealed record Message
{
    /// <summary>Its number.</summary>
    public required long Id { get; init; }

    /// <summary>How it feels.</summary>
    public Models.Tone Tone { get; init; } = Models.Tone.Neutral;

    /// <summary>What it says.</summary>
    public string? Text { get; init; }

    /// <summary>A heading.</summary>
    public string? Title { get; init; }

    /// <summary>A message of the core's own, for the client to word.</summary>
    public Notice? Notice { get; init; }

    /// <summary>The label of its button.</summary>
    public string? Action { get; init; }
}

/// <summary>Everything the client draws from: the core's view state, part by part, as it last said it.</summary>
/// <param name="App">The app's own part.</param>
/// <param name="Nav">Where the person is.</param>
/// <param name="Screens">Every screen some stack holds, by number.</param>
/// <param name="Messages">The messages waiting to be shown.</param>
public sealed record ViewState(
    AppPart? App,
    NavPart Nav,
    ImmutableDictionary<long, ScreenView> Screens,
    ImmutableList<Message> Messages)
{
    /// <summary>The state before the core has said anything.</summary>
    public static ViewState Empty { get; } =
        new(null, new NavPart(), ImmutableDictionary<long, ScreenView>.Empty, ImmutableList<Message>.Empty);

    /// <summary>
    /// How many times the core has said that the app's part changed. It is no part of the core's state: the client counts. The core
    /// says so when it draws a shell, at a sign-out and when it forgets the session, so a state with another count than an earlier one
    /// holds an app's part the core gave since, and one with the same count holds the same part, also when every part was read again.
    /// </summary>
    public long AppGiven { get; init; }

    /// <summary>The screen in front, when there is one.</summary>
    public ScreenView? FrontScreen =>
        Nav.Front is long front && Screens.TryGetValue(front, out ScreenView? screen) ? screen : null;
}
