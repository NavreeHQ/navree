using Navree.Platform;
using Navree.Runtime;

namespace Navree.Shell;

/// <summary>What an app built on the client says: where its server is, and what it is called.</summary>
public sealed record ShellOptions
{
    /// <summary>The origin the app was built for, such as <c>https://shop.example</c>.</summary>
    public required string Origin { get; init; }

    /// <summary>The app's name, which is also what its data folder is called.</summary>
    public required string AppName { get; init; }

    /// <summary>The app's own id, such as <c>com.charkha.supply</c>, by which a sign-in is handed over and a push token registered.</summary>
    public string? AppId { get; init; }

    /// <summary>The other origins the app treats as its own; a host may start with <c>*.</c>.</summary>
    public IReadOnlyList<string> Also { get; init; } = [];

    /// <summary>Whether plain <c>http</c> to the machine itself is allowed: a development build only.</summary>
    public bool LoopbackHttp { get; init; }

    /// <summary>What answers the core's requests, when it is not the network: the demo's recorded answers.</summary>
    public IFetcher? Fetcher { get; init; }

    /// <summary>What answers a request before the network does, when it has an answer to give and none when it has not: the demo's recorded sign-out.</summary>
    public Func<FetchEffect, FetchResult?>? AnswerFirst { get; init; }

    /// <summary>What answers a request of a page in the web view for the app's own origin, when it is not the network: the demo's recorded pages.</summary>
    public Func<Uri, (byte[] Body, string ContentType)?>? RecordedPages { get; init; }

    /// <summary>Where what the app keeps between launches goes; the app's folder under the local application data when left out.</summary>
    public string? KeptFolder { get; init; }

    /// <summary>
    /// Where the web views keep what a web page leaves (cookies, storage, cache); the app's folder under the local application data when left
    /// out. Each session keeps it in a folder of its own inside this one, which is never used for another session; signing out takes it away.
    /// </summary>
    public string? WebFolder { get; init; }

    /// <summary>What the app says about its place in Windows: its own scheme, its instance key, what its jump list holds.</summary>
    public PlaceOptions Place { get; init; } = new();

    /// <summary>
    /// The most bytes of all the files of one file field together. The protocol carries no largest size, so this is the client's own; a server's own
    /// limit is the server's to refuse by. 100 MiB when left out.
    /// </summary>
    public long MostFileBytes { get; init; } = Navree.Design.Inputs.FileRules.MostBytes;

    /// <summary>What opens when a person chooses files for a field: the system's file picker when left out. A run that must not open a window of the system's own gives its own.</summary>
    public IFilePicker? FilePicker { get; init; }

    /// <summary>The names of the kinds this build does not draw, which the core then shows in the web view: the kinds the renderer has no drawing for, unless the app says otherwise.</summary>
    public IReadOnlyList<string> Undrawable { get; init; } = Navree.Design.Undrawable.Kinds;
}
