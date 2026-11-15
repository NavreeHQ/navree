using Navree.Protocol;

namespace Navree.Design;

/// <summary>How wide a window is, in the three classes the core and the placement rules speak.</summary>
public enum WidthClass
{
    /// <summary>A narrow window: the navigation is a button that opens a pane over the content.</summary>
    Compact,

    /// <summary>A middle one: the navigation is a column of icons.</summary>
    Medium,

    /// <summary>A wide one: the navigation is open at the side, and a list and its detail can stand side by side.</summary>
    Expanded,
}

/// <summary>What the width of the window decides, as plain functions of a number of effective pixels.</summary>
public static class WindowRules
{
    /// <summary>The widest a compact window is: the navigation pane's own breakpoint.</summary>
    public const double CompactMost = 640;

    /// <summary>The narrowest an expanded window is: the navigation pane's own breakpoint.</summary>
    public const double ExpandedLeast = 1008;

    /// <summary>The smallest a window may be made: wide enough for snap layouts to keep working.</summary>
    public const double SmallestWindow = 500;

    /// <summary>The width a list takes when it stands beside its detail.</summary>
    public const double ListPane = 380;

    /// <summary>The class of a window of this width in effective pixels.</summary>
    /// <param name="width">The width.</param>
    /// <returns>Its class.</returns>
    public static WidthClass ClassOf(double width) =>
        width <= CompactMost ? WidthClass.Compact : width < ExpandedLeast ? WidthClass.Medium : WidthClass.Expanded;

    /// <summary>The core's word for a class.</summary>
    /// <param name="width">The class.</param>
    /// <returns>compact, medium or expanded.</returns>
    public static string Word(WidthClass width) =>
        width switch
        {
            WidthClass.Compact => "compact",
            WidthClass.Medium => "medium",
            _ => "expanded",
        };

    /// <summary>Whether a list and its detail stand side by side: the window is wide and the screen in front is a detail of the one under it.</summary>
    /// <param name="width">The window's class.</param>
    /// <param name="stackDepth">How many screens the stack holds.</param>
    /// <returns>Whether there are two panes.</returns>
    public static bool TwoPanes(WidthClass width, int stackDepth) => width == WidthClass.Expanded && stackDepth >= 2;

    /// <summary>
    /// Whether a list and the screen opened from it stand side by side: the window is wide, there is a screen opened, and the first screen of the
    /// stack is a collection. A first screen that lays out panes of its own (a list with its thread and its customer) is drawn alone, as the
    /// other clients draw it, or its panes would be set beside a copy of themselves.
    /// </summary>
    /// <param name="width">The window's class.</param>
    /// <param name="stackDepth">How many screens the stack holds.</param>
    /// <param name="firstBody">The body of the first screen of the stack.</param>
    /// <returns>Whether there are two panes.</returns>
    public static bool TwoPanes(WidthClass width, int stackDepth, Models.Node? firstBody) => TwoPanes(width, stackDepth) && IsCollection(firstBody);

    /// <summary>Whether a body is, in the main, a list of things to open: a list, a table, a pane that says it is a list, or a region or stack that holds one and no form.</summary>
    /// <param name="body">The body.</param>
    /// <returns>Whether it is a collection.</returns>
    public static bool IsCollection(Models.Node? body) =>
        body switch
        {
            Models.List or Models.Table => true,
            Models.Pane pane => pane.Role == Models.PaneRole.List,
            Models.Region region => region.Children.Any(IsCollection),
            Models.Stack stack => stack.Children.Any(child => child is Models.List or Models.Table || (child is Models.Region inner && IsCollection(inner)))
                && !stack.Children.Any(child => child is Models.Form),
            _ => false,
        };

    /// <summary>
    /// Whether a long form or a detail is a pane at the side of the window and not a dialog: the window is wide enough to
    /// keep what is behind it in view, and what it holds is more than a question or a short form.
    /// </summary>
    /// <param name="width">The window's class.</param>
    /// <param name="large">Whether what it holds is a long form or a detail.</param>
    /// <returns>Whether it is a side pane.</returns>
    public static bool UsesSidePane(WidthClass width, bool large) => width == WidthClass.Expanded && large;

    /// <summary>
    /// Whether a stack the page lays out across is laid out down in a narrow window: it holds something that needs the width
    /// of the window itself (a field, a form, a table, a section, a list), which would run off its edge beside another.
    /// </summary>
    /// <param name="width">The window's class.</param>
    /// <param name="children">What the stack holds.</param>
    /// <returns>Whether it is laid out down.</returns>
    public static bool FoldsDown(WidthClass width, IEnumerable<Models.Node> children) =>
        width == WidthClass.Compact
        && children.Count() > 1
        && children.Any(child => child is Models.Field or Models.Form or Models.Table or Models.Section or Models.List or Models.Card);

    /// <summary>The space of a step of the protocol's spacing: four effective pixels, the grid of the Windows design guidance.</summary>
    /// <param name="steps">How many steps.</param>
    /// <returns>Effective pixels.</returns>
    public static double Space(int steps) => steps * 4.0;

    /// <summary>The widest a block of reading text is allowed to run, so a line stays short enough to read.</summary>
    public const double ReadingMeasure = 760;
}

/// <summary>The words the client says for what the core only names. They are the client's: the server's own words are never replaced.</summary>
public static class Words
{
    /// <summary>The words of a message of the core's own.</summary>
    /// <param name="notice">Which one, by the core's name.</param>
    /// <returns>What to say, or nothing for a name this build does not know.</returns>
    public static string? Notice(string notice) =>
        notice switch
        {
            "offline" => "You are offline. Check the connection and try again.",
            "timeout" => "That took too long. Try again.",
            "failed" => "That did not work. Try again.",
            "unsaved" => "You have changes that are not saved.",
            "loading" => "Loading",
            "copied" => "Copied",
            "fields_need_attention" => "Some fields need attention.",
            "loaded" => "Loaded",
            "second_step" => "One more step to finish signing in.",
            _ => null,
        };

    /// <summary>Why a page could not be shown.</summary>
    /// <param name="failure">The core's name for it.</param>
    /// <returns>A title and a line of help.</returns>
    public static (string Title, string Help) Failure(string failure) =>
        failure switch
        {
            "offline" => ("You are offline", "Check the connection and try again."),
            "timeout" => ("That took too long", "The server did not answer in time. Try again."),
            "server" => ("The server had a problem", "Try again in a moment."),
            "protocol" => ("This page could not be read", "The app may need an update."),
            "config" => ("The app is not set up", "Its address is not one it can use."),
            _ => ("Something went wrong", "Try again."),
        };

    /// <summary>What a screen reader is told when a list has no rows.</summary>
    public const string NothingHere = "Nothing here yet";
}

/// <summary>Which of Windows' own icons (Segoe Fluent Icons) stands for an icon a page names.</summary>
public static class SystemGlyphs
{
    private static readonly Dictionary<string, string> Names = new(StringComparer.OrdinalIgnoreCase)
    {
        ["home"] = "", ["house"] = "", ["dashboard"] = "", ["overview"] = "", ["today"] = "",
        ["orders"] = "", ["order"] = "", ["receipt"] = "", ["sales"] = "", ["invoice"] = "",
        ["inventory"] = "", ["stock"] = "", ["package"] = "", ["box"] = "", ["warehouse"] = "",
        ["products"] = "", ["shopping-bag"] = "", ["shopping_bag"] = "", ["cart"] = "", ["tag"] = "",
        ["customers"] = "", ["users"] = "", ["people"] = "", ["team"] = "", ["contacts"] = "", ["user"] = "",
        ["inbox"] = "", ["mail"] = "", ["messages"] = "", ["chat"] = "", ["message"] = "", ["support"] = "",
        ["board"] = "", ["kanban"] = "", ["tasks"] = "", ["checklist"] = "", ["projects"] = "",
        ["reports"] = "", ["analytics"] = "", ["chart"] = "", ["bar-chart"] = "",
        ["settings"] = "", ["cog"] = "", ["notifications"] = "", ["bell"] = "",
        ["search"] = "", ["plus"] = "", ["add"] = "", ["edit"] = "", ["pencil"] = "", ["trash"] = "", ["delete"] = "",
        ["share"] = "", ["download"] = "", ["upload"] = "", ["refresh"] = "", ["calendar"] = "", ["clock"] = "",
        ["heart"] = "", ["activity"] = "", ["moon"] = "", ["sleep"] = "", ["fitness"] = "",
        ["check"] = "", ["close"] = "", ["x"] = "", ["more"] = "", ["menu"] = "", ["filter"] = "",
        ["info"] = "", ["alert"] = "", ["warning"] = "", ["lock"] = "", ["key"] = "", ["globe"] = "",
    };

    /// <summary>The glyph for a page's icon name, or the one for a page when the name maps nowhere.</summary>
    /// <param name="name">The icon's name.</param>
    /// <param name="label">The label beside it, whose words can name an icon when the icon has no name.</param>
    /// <returns>A character of Segoe Fluent Icons.</returns>
    public static string For(string? name, string? label = null)
    {
        if (name is not null && Names.TryGetValue(name.Trim(), out string? found))
        {
            return found;
        }

        foreach (string word in (label ?? string.Empty).Split([' ', '-', '_', '/'], StringSplitOptions.RemoveEmptyEntries))
        {
            if (Names.TryGetValue(word, out string? byLabel))
            {
                return byLabel;
            }
        }

        return "";
    }

    /// <summary>The glyph of a standard action.</summary>
    /// <param name="standard">The action.</param>
    /// <returns>A character of Segoe Fluent Icons.</returns>
    public static string Of(Models.Standard standard) =>
        standard switch
        {
            Models.Standard.Share => "",
            Models.Standard.Delete => "",
            Models.Standard.Add => "",
            Models.Standard.Edit => "",
            Models.Standard.Search => "",
            Models.Standard.Done => "",
            Models.Standard.Cancel => "",
            _ => "",
        };
}
