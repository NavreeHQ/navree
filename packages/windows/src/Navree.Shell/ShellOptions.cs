using Navree.Platform;

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

    /// <summary>What answers a request of a page in the web view for the app's own origin, when it is not the network: the demo's recorded pages.</summary>
    public Func<Uri, (byte[] Body, string ContentType)?>? RecordedPages { get; init; }

    /// <summary>Where what the app keeps between launches goes; the app's folder under the local application data when left out.</summary>
    public string? KeptFolder { get; init; }

    /// <summary>What the app says about its place in Windows: its own scheme, its instance key, what its jump list holds.</summary>
    public PlaceOptions Place { get; init; } = new();

    /// <summary>The names of the kinds this build does not draw, which the core then shows in the web view.</summary>
    public IReadOnlyList<string> Undrawable { get; init; } = [];
}
