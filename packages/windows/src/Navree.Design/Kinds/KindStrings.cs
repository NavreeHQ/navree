namespace Navree.Design;

/// <summary>
/// The words the drawn kinds say themselves, drawn or spoken: what a chart says of a place with no reading, what a button of the day
/// pager is called, what a screen reader calls a ring. They are English here, and a host that has the person's language puts its own
/// in through <see cref="Provider"/>; no kind keeps a word of its own in its code. The page's words (titles, labels, names) are never
/// here: they come from the server in the person's language already.
/// </summary>
public static class KindStrings
{
    private static readonly Dictionary<string, string> Defaults = new(StringComparer.Ordinal)
    {
        ["Kind_NoReading"] = "No reading",
        ["Kind_NotTested"] = "Not tested",
        ["Kind_MayBeChance"] = "May be chance",
        ["Kind_Pair"] = "{0} and {1}",
        ["Kind_NotMeasured"] = "Not measured",
        ["Kind_PastGoal"] = "past goal",
        ["Kind_PreviousDay"] = "Previous day",
        ["Kind_NextDay"] = "Next day",
        ["Kind_Today"] = "Today",
        ["Kind_PageOf"] = "Page {0} of {1}",
        ["Kind_About"] = "About",
        ["Kind_AboutThing"] = "About {0}",
        ["Kind_TrendUp"] = "Up",
        ["Kind_TrendDown"] = "Down",
        ["Kind_TrendSteady"] = "Steady",
        ["Kind_AxisAcross"] = "x",
        ["Kind_AxisDown"] = "y",
        ["Kind_NoteWords"] = "Internal note, not sent to the customer",
        ["Kind_Sending"] = "Sending",
        ["Kind_Sent"] = "Sent",
        ["Kind_Delivered"] = "Delivered",
        ["Kind_Read"] = "Read",
        ["Kind_NotSent"] = "Not sent",
        ["Kind_TypeChart"] = "chart",
        ["Kind_TypeRing"] = "ring",
        ["Kind_TypeRings"] = "rings",
        ["Kind_TypeGauge"] = "gauge",
        ["Kind_TypeRange"] = "range",
        ["Kind_TypeDayClock"] = "day clock",
        ["Kind_TypeMeter"] = "meter",
        ["Kind_TypeStrip"] = "strip",
        ["Kind_TypeCalendar"] = "calendar",
        ["Kind_TypeMatrix"] = "matrix",
        ["Kind_TypeScatter"] = "scatter chart",
    };

    /// <summary>Gives a host the means to put its own words in: it is asked first, and answers none for any it does not have.</summary>
    public static Func<string, string?>? Provider { get; set; }

    /// <summary>The identities of every word a kind says.</summary>
    public static IReadOnlyCollection<string> Ids => Defaults.Keys;

    /// <summary>The English words for an identity: what a person who has no other gets.</summary>
    /// <param name="id">The identity.</param>
    /// <returns>The words, or none for an identity that is not one.</returns>
    public static string? English(string id) => Defaults.GetValueOrDefault(id);

    /// <summary>The words for an identity, with its numbers and names put in the way the client writes them.</summary>
    /// <param name="id">The identity, such as <c>Kind_NoReading</c>.</param>
    /// <param name="arguments">What is put into the words.</param>
    /// <returns>The words; the identity itself where there are none (a fault of the client that shows plainly).</returns>
    public static string Get(string id, params object[] arguments)
    {
        string? text = Provider?.Invoke(id);
        if (text is null && !Defaults.TryGetValue(id, out text))
        {
            return id;
        }

        return arguments.Length == 0 ? text : string.Format(ClientCulture.Current, text, arguments);
    }

    /// <summary>A place of a chart that has no value.</summary>
    public static string NoReading => Get("Kind_NoReading");

    /// <summary>A pair of measures that was never tested.</summary>
    public static string NotTested => Get("Kind_NotTested");

    /// <summary>A relation that may be no more than chance.</summary>
    public static string MayBeChance => Get("Kind_MayBeChance");

    /// <summary>Two names said together.</summary>
    /// <param name="one">The first name.</param>
    /// <param name="other">The second name.</param>
    /// <returns>The words.</returns>
    public static string Pair(string one, string other) => Get("Kind_Pair", one, other);

    /// <summary>A ring that has no value.</summary>
    public static string NotMeasured => Get("Kind_NotMeasured");

    /// <summary>What a ring whose value is above its goal says besides its value.</summary>
    public static string PastGoal => Get("Kind_PastGoal");

    /// <summary>The button that goes to the day before.</summary>
    public static string PreviousDay => Get("Kind_PreviousDay");

    /// <summary>The button that goes to the day after.</summary>
    public static string NextDay => Get("Kind_NextDay");

    /// <summary>The button that goes to today.</summary>
    public static string Today => Get("Kind_Today");

    /// <summary>The mark of a page of a paged run.</summary>
    /// <param name="page">The page, from 1.</param>
    /// <param name="pages">How many pages.</param>
    /// <returns>The words.</returns>
    public static string PageOf(int page, int pages) => Get("Kind_PageOf", page, pages);

    /// <summary>The name of a hint's button when the hint says nothing about what it is about.</summary>
    public static string About => Get("Kind_About");

    /// <summary>The name of a hint's button.</summary>
    /// <param name="thing">What the hint is about.</param>
    /// <returns>The words.</returns>
    public static string AboutThing(string thing) => Get("Kind_AboutThing", thing);

    /// <summary>A trend that is going up.</summary>
    public static string TrendUp => Get("Kind_TrendUp");

    /// <summary>A trend that is going down.</summary>
    public static string TrendDown => Get("Kind_TrendDown");

    /// <summary>A trend that is not going either way.</summary>
    public static string TrendSteady => Get("Kind_TrendSteady");

    /// <summary>What a scatter calls its axis across when the page names none.</summary>
    public static string AxisAcross => Get("Kind_AxisAcross");

    /// <summary>What a scatter calls its axis down when the page names none.</summary>
    public static string AxisDown => Get("Kind_AxisDown");

    /// <summary>What an internal note says it is.</summary>
    public static string NoteWords => Get("Kind_NoteWords");

    /// <summary>A message on its way.</summary>
    public static string Sending => Get("Kind_Sending");

    /// <summary>A message that has gone.</summary>
    public static string Sent => Get("Kind_Sent");

    /// <summary>A message that has arrived.</summary>
    public static string Delivered => Get("Kind_Delivered");

    /// <summary>A message that has been read.</summary>
    public static string Read => Get("Kind_Read");

    /// <summary>A message that did not go.</summary>
    public static string NotSent => Get("Kind_NotSent");

    /// <summary>What a screen reader calls a chart.</summary>
    public static string TypeChart => Get("Kind_TypeChart");

    /// <summary>What a screen reader calls a ring.</summary>
    public static string TypeRing => Get("Kind_TypeRing");

    /// <summary>What a screen reader calls concentric rings.</summary>
    public static string TypeRings => Get("Kind_TypeRings");

    /// <summary>What a screen reader calls a gauge.</summary>
    public static string TypeGauge => Get("Kind_TypeGauge");

    /// <summary>What a screen reader calls a range.</summary>
    public static string TypeRange => Get("Kind_TypeRange");

    /// <summary>What a screen reader calls a day clock.</summary>
    public static string TypeDayClock => Get("Kind_TypeDayClock");

    /// <summary>What a screen reader calls a meter.</summary>
    public static string TypeMeter => Get("Kind_TypeMeter");

    /// <summary>What a screen reader calls a strip.</summary>
    public static string TypeStrip => Get("Kind_TypeStrip");

    /// <summary>What a screen reader calls a calendar of days.</summary>
    public static string TypeCalendar => Get("Kind_TypeCalendar");

    /// <summary>What a screen reader calls a matrix of relations.</summary>
    public static string TypeMatrix => Get("Kind_TypeMatrix");

    /// <summary>What a screen reader calls a scatter chart.</summary>
    public static string TypeScatter => Get("Kind_TypeScatter");
}
