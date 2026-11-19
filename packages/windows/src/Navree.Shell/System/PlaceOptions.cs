using Navree.Presence;

namespace Navree.Shell;

/// <summary>What an app says about its place in Windows.</summary>
public sealed record PlaceOptions
{
    /// <summary>The app's own scheme, by which a sign-in in the browser comes back (<c>charkha</c>); none when the app has none.</summary>
    public string? Scheme { get; init; }

    /// <summary>The key by which a second start finds the app already running; the app's name when left out.</summary>
    public string? InstanceKey { get; init; }

    /// <summary>The name of the package's startup task, which the manifest declares.</summary>
    public string StartupTaskId { get; init; } = "NavreeStartup";

    /// <summary>Where what the person chose goes; the app's folder under the local application data when left out.</summary>
    public string? ChoicesFolder { get; init; }

    /// <summary>The most entries the jump list has.</summary>
    public int JumpListLimit { get; init; } = 4;

    /// <summary>
    /// How long the app may hear nothing from its server before a reminder that comes due for a signed-in person asks the shell first;
    /// fifteen minutes when left out.
    /// </summary>
    public TimeSpan ReminderSilence { get; init; } = NotifyRules.DefaultSilence;

    /// <summary>What confirms a person with the device's own means; none is proven, so the default confirms nobody.</summary>
    public IDeviceAuthenticator Authenticator { get; init; } = new NoAuthenticator();
}

/// <summary>What this run can do in its place in Windows, asked once and the same for the whole run.</summary>
public static class PlaceCapabilities
{
    private static IReadOnlyList<FeatureOffer>? current;

    /// <summary>The offers of this run: package identity, administrator rights, and whether Windows took the app's registration for notifications.</summary>
    /// <returns>The offers.</returns>
    public static IReadOnlyList<FeatureOffer> Current()
    {
        if (current is not null)
        {
            return current;
        }

        List<FeatureOffer> offers = [.. Capabilities.For(PackageIdentity.Has(), PackageIdentity.Elevated())];
        int at = offers.FindIndex(offer => offer.Feature == SystemFeature.Notifications);
        if (at >= 0 && offers[at].Offered && !Activation.NotificationsRegistered)
        {
            offers[at] = offers[at] with { Offered = false, Reason = "Windows did not take the app's registration for notifications" };
        }

        current = offers;
        return offers;
    }
}
