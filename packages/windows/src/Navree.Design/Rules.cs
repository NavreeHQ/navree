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

    /// <summary>The width the navigation pane takes beside the page when it is open at the side (a window of the expanded class).</summary>
    public const double NavigationPane = 320;

    /// <summary>The width the navigation pane takes beside the page when it shows only its icons (a window of the medium class).</summary>
    public const double NavigationIcons = 48;

    /// <summary>The width a pane at the side of the page takes (a long form or a detail beside what it was opened from).</summary>
    public const double SidePane = 440;

    /// <summary>The least width the page keeps beside a pane: a detail narrower than this is read better alone.</summary>
    public const double DetailLeast = 480;

    /// <summary>The width the page has in a window: what is left of the window beside the navigation pane.</summary>
    /// <param name="window">The window's width in effective pixels.</param>
    /// <returns>The page's room in effective pixels.</returns>
    public static double PageRoom(double window) =>
        Math.Max(0, window - ClassOf(window) switch
        {
            WidthClass.Compact => 0,
            WidthClass.Medium => NavigationIcons,
            _ => NavigationPane,
        });

    /// <summary>
    /// A size in effective pixels as the device pixels the system sizes windows in at a display scale. Every size the client gives a window goes
    /// through this: a size written in device pixels is a different window on each display (a thousand by seven hundred is 571 by 400 at 175 percent).
    /// </summary>
    /// <param name="effective">The size in effective pixels.</param>
    /// <param name="scale">How many device pixels make one effective pixel; a scale that is not a number above nothing counts as one.</param>
    /// <returns>The size in device pixels, rounded up so a window is never smaller than asked.</returns>
    public static int Raw(double effective, double scale) =>
        (int)Math.Ceiling(Math.Max(0, effective) * (scale > 0 && double.IsFinite(scale) ? scale : 1.0));

    /// <summary>A size in device pixels as effective pixels at a display scale.</summary>
    /// <param name="raw">The size in device pixels.</param>
    /// <param name="scale">How many device pixels make one effective pixel.</param>
    /// <returns>The size in effective pixels.</returns>
    public static double Effective(int raw, double scale) => raw / (scale > 0 && double.IsFinite(scale) ? scale : 1.0);

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

    /// <summary>
    /// Whether a list and the screen opened from it stand side by side in a window of this width: what is left beside the navigation pane holds the
    /// list and a detail that is still wide enough to read.
    /// </summary>
    /// <param name="window">The window's width in effective pixels.</param>
    /// <param name="stackDepth">How many screens the stack holds.</param>
    /// <param name="firstBody">The body of the first screen of the stack.</param>
    /// <returns>Whether there are two panes.</returns>
    public static bool TwoPanes(double window, int stackDepth, Models.Node? firstBody) =>
        TwoPanes(ClassOf(window), stackDepth, firstBody) && PageRoom(window) >= ListPane + DetailLeast;

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

    /// <summary>Whether a long form or a detail is a pane at the side in a window of this width: the page keeps room to be read beside the pane.</summary>
    /// <param name="window">The window's width in effective pixels.</param>
    /// <param name="large">Whether what it holds is a long form or a detail.</param>
    /// <returns>Whether it is a side pane.</returns>
    public static bool UsesSidePane(double window, bool large) => UsesSidePane(ClassOf(window), large) && PageRoom(window) >= SidePane + DetailLeast;

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
        && (children.Any(child => child is Models.Field or Models.Form or Models.Table or Models.Section or Models.List or Models.Card) || IsLanes(children));

    /// <summary>
    /// Whether what a stack holds across is a set of lanes (the columns of a board, a row of charts): three or more parts that each hold a list or a
    /// chart. Lanes are not squeezed to what is left of the window; a window too narrow for them scrolls across, or lays them down when it is compact.
    /// </summary>
    /// <param name="children">What the stack holds.</param>
    /// <returns>Whether they are lanes.</returns>
    public static bool IsLanes(IEnumerable<Models.Node> children)
    {
        List<Models.Node> parts = [.. children];
        return parts.Count >= 3 && parts.All(HoldsLaneContent);
    }

    // A list, a table or a chart is in the part, at any depth.
    private static bool HoldsLaneContent(Models.Node node) =>
        node is Models.List or Models.Table or Models.Chart || Tree.ChildNodes(node).Any(HoldsLaneContent);

    /// <summary>The least width of a lane, in effective pixels, before a stack of lanes scrolls across.</summary>
    public const double LeastLane = 220;

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
            "saved_not_refreshed" => "Saved. The screen could not be brought up to date.",
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
    // Names the web's icon set has more than one of for the same thing: each is drawn as the one it is another name for.
    private static readonly Dictionary<string, string> Aliases = new(StringComparer.OrdinalIgnoreCase)
    {
        ["bar-chart-3"] = "bar-chart", ["chart-bar"] = "bar-chart", ["chart-column"] = "bar-chart",
        ["line-chart"] = "chart", ["chart-line"] = "chart",
        ["check-check"] = "check", ["check-circle"] = "check", ["circle-check"] = "check", ["circle-plus"] = "plus",
        ["dumbbell"] = "fitness", ["receipt-text"] = "receipt", ["share-2"] = "share", ["square-kanban"] = "kanban",
        ["trash-2"] = "trash", ["bed"] = "sleep", ["search-x"] = "search",
    };

    private static readonly Dictionary<string, string> Names = new(StringComparer.OrdinalIgnoreCase)
    {
        ["home"] = "", ["house"] = "", ["dashboard"] = "", ["overview"] = "", ["today"] = "",
        ["orders"] = "", ["order"] = "", ["receipt"] = "", ["sales"] = "", ["invoice"] = "",
        ["inventory"] = "", ["stock"] = "", ["package"] = "", ["box"] = "", ["boxes"] = "", ["layout-dashboard"] = "", ["warehouse"] = "",
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
        // Names a page sends beyond the first rows, each with the symbol of the same meaning (looked at in the font's own sheet).
        ["footprints"] = "\uE726",
        ["person-standing"] = "\uE726",
        ["bike"] = "\uE726",
        ["walk"] = "\uE726",
        ["run"] = "\uE726",
        ["sunrise"] = "\uE706",
        ["sun"] = "\uE706",
        ["zap"] = "\uE945",
        ["flame"] = "\uECAD",
        ["droplet"] = "\uEB42",
        ["droplets"] = "\uEB42",
        ["flask-round"] = "\uEB42",
        ["thermometer"] = "\uE9CA",
        ["mountain"] = "\uE9D2",
        ["waves"] = "\uE9D2",
        ["waves-horizontal"] = "\uE9D2",
        ["wind"] = "\uE753",
        ["cloud"] = "\uE753",
        ["heart-pulse"] = "\uE95E",
        ["brain"] = "\uE9D9",
        ["timer"] = "\uE916",
        ["stopwatch"] = "\uE916",
        ["hourglass"] = "\uE823",
        ["clock-4"] = "\uE917",
        ["clock-3"] = "\uE917",
        ["radio"] = "\uEC44",
        ["volume-2"] = "\uE767",
        ["volume"] = "\uE767",
        ["sparkles"] = "\uE794",
        ["shield-check"] = "\uE705",
        ["shield"] = "\uE705",
        ["camera"] = "\uE722",
        ["image"] = "\uE91B",
        ["truck"] = "\uE7EC",
        ["car"] = "\uE7EC",
        ["map"] = "\uE800",
        ["map-pin"] = "\uE707",
        ["route"] = "\uE816",
        ["layout-grid"] = "\uE8A9",
        ["list"] = "\uE8FD",
        ["folder"] = "\uE8B7",
        ["file"] = "\uE7C3",
        ["form"] = "\uE7C3",
        ["file-text"] = "\uE8A5",
        ["store"] = "\uE719",
        ["gift"] = "\uE719",
        ["candy"] = "\uE719",
        ["building"] = "\uE731",
        ["building-2"] = "\uEC0B",
        ["credit-card"] = "\uE8C7",
        ["wallet"] = "\uE8C7",
        ["banknote"] = "\uE8C7",
        ["indian-rupee"] = "\uE8C7",
        ["key"] = "\uE8D7",
        ["eye"] = "\uE7B3",
        ["view"] = "\uE7B3",
        ["eye-off"] = "\uE7B3",
        ["history"] = "\uE81C",
        ["arrow-left"] = "\uE72B",
        ["arrow-right"] = "\uE72A",
        ["chevron-right"] = "\uE76C",
        ["chevron-down"] = "\uE70D",
        ["chevron-up"] = "\uE70E",
        ["trending-up"] = "\uEAFC",
        ["trending-down"] = "\uEAFC",
        ["help-circle"] = "\uE9CE",
        ["circle-help"] = "\uE9CE",
        ["alert-triangle"] = "\uE7BA",
        ["triangle-alert"] = "\uE7BA",
        ["alert-circle"] = "\uE783",
        ["circle-alert"] = "\uE783",
        ["undo"] = "\uE7A7",
        ["undo-2"] = "\uE7A7",
        ["reply"] = "\uE97A",
        ["send"] = "\uE724",
        ["paperclip"] = "\uE723",
        ["phone"] = "\uE717",
        ["smartphone"] = "\uE8EA",
        ["globe"] = "\uE774",
        ["log-out"] = "\uE7E8",
        ["message-square"] = "\uE8BD",
        ["message-circle"] = "\uE8BD",
        ["messages-square"] = "\uE8F2",
        ["star"] = "\uE734",
        ["package-check"] = "\uE7B8",
        ["package-plus"] = "\uE7B8",
        ["archive"] = "\uE7B8",
        ["ban"] = "\uE733",
        ["refresh-cw"] = "\uE72C",
        ["rotate-cw"] = "\uE72C",
        ["copy"] = "\uE8C8",
        ["printer"] = "\uE749",
        ["pen"] = "\uE70F",
        ["square-pen"] = "\uE70F",
        ["signature"] = "\uE70F",
        ["list-filter"] = "\uE71C",
        ["arrow-up-down"] = "\uE8CB",
        ["ellipsis"] = "\uE712",
        ["ellipsis-vertical"] = "\uE712",
        ["more-vertical"] = "\uE712",
        ["more-horizontal"] = "\uE712",
        ["settings-2"] = "\uE713",
        ["circle-user"] = "\uE77B",
        ["user-round"] = "\uE77B",
        ["users-round"] = "\uE716",
        ["tags"] = "\uE8EC",
        ["shirt"] = "\uE8EC",
        ["cup-soda"] = "\uEC32",
        ["coffee"] = "\uEC32",
        ["soup"] = "\uEC09",
        ["cooking-pot"] = "\uEC09",
        ["cookie"] = "\uEC09",
        ["milk"] = "\uEC09",
        ["salad"] = "\uEC09",
        ["utensils"] = "\uEC09",
        ["utensils-crossed"] = "\uEC09",
        ["flower"] = "\uEC0A",
        ["rose"] = "\uEC0A",
        ["wheat"] = "\uEC0A",
        ["leaf"] = "\uEC0A",
        ["bug"] = "\uEBE8",
        ["lamp"] = "\uE82F",
        ["layers"] = "\uE81E",
        ["palette"] = "\uE790",
        ["ribbon"] = "\uECA7",
        ["scale"] = "\uE9E9",
        ["type"] = "\uE8D2",
        ["columns"] = "\uE736",
        ["gauge"] = "\uEC48",
        ["pie-chart"] = "\uEB05",
        ["flag"] = "\uE7C1",
        ["shopping-cart"] = "\uE7BF",
        ["date"] = "\uE787",
        ["calendar-days"] = "\uE787",
        ["file-bar-chart"] = "\uE9F9",
        ["library"] = "\uE8F1",
        ["images"] = "\uE8B9",
        ["thumbs-up"] = "\uE8E1",
        ["list-checks"] = "\uE9A4",
        ["link"] = "\uE71B",
        ["external-link"] = "\uE8A7",
    };

    /// <summary>The glyph for a page's icon name, or the one for a page when the name maps nowhere.</summary>
    /// <param name="name">The icon's name.</param>
    /// <param name="label">The label beside it, whose words can name an icon when the icon has no name.</param>
    /// <returns>A character of Segoe Fluent Icons.</returns>
    public static string For(string? name, string? label = null)
    {
        if (name is not null && Find(name) is { } found)
        {
            return found;
        }

        foreach (string word in (label ?? string.Empty).Split([' ', '-', '_', '/'], StringSplitOptions.RemoveEmptyEntries))
        {
            if (Find(word) is { } byLabel)
            {
                return byLabel;
            }
        }

        // A name that is still unknown is a small neutral dot, never a document or any shape that says something it does not mean.
        return NeutralDot;
    }

    /// <summary>The glyph drawn for an icon whose name the client does not know.</summary>
    public const string NeutralDot = "\uECCC";

    /// <summary>Whether the client has a symbol for an icon name.</summary>
    /// <param name="name">The icon's name.</param>
    /// <returns>Whether the name is in the map.</returns>
    public static bool Has(string name) => Find(name) is not null;

    private static string? Find(string name)
    {
        string key = name.Trim();
        if (Names.TryGetValue(key, out string? found))
        {
            return found;
        }

        return Aliases.TryGetValue(key, out string? other) && Names.TryGetValue(other, out string? aliased) ? aliased : null;
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
