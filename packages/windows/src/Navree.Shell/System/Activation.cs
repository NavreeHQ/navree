using System.Diagnostics;
using Microsoft.Windows.AppLifecycle;
using Microsoft.Windows.AppNotifications;
using Navree.Presence;
using Windows.ApplicationModel.Activation;

namespace Navree.Shell;

/// <summary>
/// How the app is started and started again: one app running, a second start handing what it was asked to the first, a return from
/// signing in in the browser on the app's own scheme, a click on a notification. Everything that arrives is read into a
/// <see cref="LaunchRequest"/>, which is the one thing the rest of the app understands. It is called before any window is made.
/// </summary>
public static class Activation
{
    private static readonly object Gate = new();
    private static readonly List<(LaunchRequest Request, string How)> Waiting = [];
    private static string? scheme;
    private static Action<LaunchRequest, string>? sink;

    /// <summary>Whether Windows took the app's registration for notifications, which showing one needs.</summary>
    public static bool NotificationsRegistered { get; private set; }

    /// <summary>What this start was asked.</summary>
    public static LaunchRequest Initial { get; private set; } = LaunchRequest.Plain;

    /// <summary>
    /// Decides whether this start goes on: the first start of the app does, and a later one hands what it was asked to the first (which
    /// is brought to the front) and ends. Registers for notifications first, so that a click on one can start the app.
    /// </summary>
    /// <param name="key">The key of the app's one instance.</param>
    /// <param name="ownScheme">The app's own scheme, or null.</param>
    /// <param name="single">
    /// Whether a second start is handed to the first; false lets every start run, which is for walks that run several at once and not for an
    /// app: two running apps share what is kept and the web view's folder, and a sign-out in one does not reach the other.
    /// </param>
    /// <returns>Whether this start goes on.</returns>
    public static bool Claim(string key, string? ownScheme, bool single = true)
    {
        ArgumentException.ThrowIfNullOrEmpty(key);
        scheme = ownScheme;
        try
        {
            // The handler is set before the registration, as the system's page says; an app that Windows cannot register still runs, with no notifications.
            AppNotificationManager.Default.NotificationInvoked += OnNotification;
            AppNotificationManager.Default.Register();
            NotificationsRegistered = true;
        }
        catch (Exception)
        {
            NotificationsRegistered = false;
        }

        AppActivationArguments args = AppInstance.GetCurrent().GetActivatedEventArgs();
        LaunchRequest request = Read(args, own: true);
        Note("started by " + Describe(args));
        if (!single)
        {
            Initial = request;
            return true;
        }

        AppInstance instance = AppInstance.FindOrRegisterForKey(key);
        Note(instance.IsCurrent ? "this is the app's one instance" : "the app is already running (process " + instance.ProcessId + ")");
        if (instance.IsCurrent)
        {
            instance.Activated += (_, redirected) =>
            {
                Note("handed " + Describe(redirected));
                Deliver(Read(redirected, own: false), Describe(redirected));
            };
            Initial = request;
            return true;
        }

        Redirect(args, instance);
        return false;
    }

    /// <summary>Registers the app's own scheme for this person, for a build with no package (a package declares it in its manifest).</summary>
    /// <param name="ownScheme">The scheme.</param>
    /// <param name="displayName">What Windows calls the app for it.</param>
    /// <returns>Whether the scheme is the app's.</returns>
    public static bool RegisterScheme(string ownScheme, string displayName)
    {
        if (PackageIdentity.Has())
        {
            return true;
        }

        try
        {
            string exe = Environment.ProcessPath ?? string.Empty;
            if (exe.Length == 0)
            {
                return false;
            }

            ActivationRegistrationManager.RegisterForProtocolActivation(SignInRules.RegisteredName(ownScheme), exe + ",0", displayName, exe);
            return true;
        }
        catch (Exception)
        {
            return false;
        }
    }

    /// <summary>Starts receiving what arrives while the app runs; what arrived before is handed over now.</summary>
    /// <param name="receiver">Takes a request and a line saying how it came (never its address), on any thread.</param>
    public static void Receive(Action<LaunchRequest, string> receiver)
    {
        ArgumentNullException.ThrowIfNull(receiver);
        List<(LaunchRequest Request, string How)> early;
        lock (Gate)
        {
            sink = receiver;
            early = [.. Waiting];
            Waiting.Clear();
        }

        foreach ((LaunchRequest request, string how) in early)
        {
            receiver(request, how);
        }
    }

    private static void Deliver(LaunchRequest request, string how)
    {
        Action<LaunchRequest, string>? receiver;
        lock (Gate)
        {
            receiver = sink;
            if (receiver is null)
            {
                Waiting.Add((request, how));
            }
        }

        receiver?.Invoke(request, how);
    }

    private static void OnNotification(AppNotificationManager sender, AppNotificationActivatedEventArgs args) =>
        Deliver(FromNotification(args), "a click on a notification");

    private static LaunchRequest FromNotification(AppNotificationActivatedEventArgs args)
    {
        string? href = args.Arguments.TryGetValue("href", out string? text) && OpeningRules.IsAppPath(text) ? text : null;
        return new LaunchRequest(href, null, false, false);
    }

    /// <summary>A line about how the app was started or handed something, kept beside the app's other temporary files for whoever looks into it; never an address.</summary>
    internal static void Note(string line)
    {
        try
        {
            File.AppendAllText(
                Path.Combine(Path.GetTempPath(), "navree-activation.log"),
                DateTime.Now.ToString("HH:mm:ss.fff", System.Globalization.CultureInfo.InvariantCulture) + " " + Environment.ProcessId + " " + line + Environment.NewLine);
        }
        catch (Exception problem) when (problem is IOException or UnauthorizedAccessException)
        {
            // A log that cannot be written is no reason to stop the app.
        }
    }

    // How an activation came, for the log: its kind and the kind of what it carries, never an address (which can hold a code).
    private static string Describe(AppActivationArguments args) =>
        "activation " + args.Kind + " carrying " + (args.Data?.GetType().Name ?? "nothing");

    // What the system says started the app, as one request: a flag or a path on the command line, an address on the app's scheme, a click on a notification.
    // The command line is the process's own: it is read only for the start that made this process, never for an activation handed to it by another.
    private static LaunchRequest Read(AppActivationArguments args, bool own)
    {
        try
        {
            switch (args.Kind)
            {
                case ExtendedActivationKind.Protocol when args.Data is IProtocolActivatedEventArgs protocol:
                    string address = protocol.Uri.OriginalString;
                    return scheme is not null && SignInRules.IsOnScheme(address, scheme)
                        ? new LaunchRequest(null, address, false, false)
                        : LaunchRequest.Plain;
                case ExtendedActivationKind.AppNotification when args.Data is AppNotificationActivatedEventArgs note:
                    return FromNotification(note);
                case ExtendedActivationKind.Launch when args.Data is ILaunchActivatedEventArgs launch && !string.IsNullOrEmpty(launch.Arguments):
                    return LaunchArguments.Read(LaunchArguments.Split(launch.Arguments), scheme);
                default:
                    return own ? LaunchArguments.Read([.. Environment.GetCommandLineArgs().Skip(1)], scheme) : LaunchRequest.Plain;
            }
        }
        catch (Exception)
        {
            // What the system said does not read: the app starts as it would with nothing said.
            return LaunchRequest.Plain;
        }
    }

    // The second start hands its activation to the first and waits for that to be done, on a wait that lets the thread answer calls sent to it.
    private static void Redirect(AppActivationArguments args, AppInstance instance)
    {
        Exception?[] failure = [null];
        PumpingWait.For(Task.Run(() =>
        {
            try
            {
                instance.RedirectActivationToAsync(args).AsTask().Wait();
            }
            catch (Exception problem)
            {
                failure[0] = problem.GetBaseException();
            }
        }));
        Note(failure[0] is Exception failed
            ? "handing " + args.Kind + " over failed: " + failed.GetType().Name + ": " + failed.Message
            : "handed " + args.Kind + " over");
        try
        {
            using Process running = Process.GetProcessById((int)instance.ProcessId);
            if (running.MainWindowHandle != 0)
            {
                WindowFrame.Raise(running.MainWindowHandle);
            }
        }
        catch (Exception problem) when (problem is ArgumentException or InvalidOperationException)
        {
            // The first instance is gone already, or has no window yet (it is hidden in the notification area): it brings itself to the front.
        }
    }
}
