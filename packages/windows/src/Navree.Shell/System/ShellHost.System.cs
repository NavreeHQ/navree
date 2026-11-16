using System.Diagnostics;
using Navree.Platform;
using Navree.Presence;
using Navree.Protocol;
using Navree.Runtime;

namespace Navree.Shell;

// What the app does for the system outside its windows: the device calls that reach it, the sign-in in the browser, what is kept for
// the next launch, and what happens when the core says it has stopped.
public sealed partial class ShellHost
{
    private const string SnapshotKey = "snapshot";
    private readonly SignInTracker signIn = new();
    private LoopbackReturn? loopback;
    private CancellationTokenSource? loopbackWait;
    private bool coreStopped;
    private string? callbackText;
    private ReturnKind returnKind;

    /// <summary>The app's place in Windows: set by the app once its main window exists; the device calls that need a window are answered through it.</summary>
    public SystemPlace? Place { get; set; }

    /// <summary>Raised on the UI thread once, when the core says it has stopped for good: the app can only be started again.</summary>
    public event Action? CoreStopped;

    /// <summary>Whether the core has said it stopped.</summary>
    public bool HasStopped => coreStopped;

    /// <summary>Tells the host a problem the core's answer was found to say, as the core's own thread does; a stop is offered to the person.</summary>
    /// <param name="what">The problem, in the client's words.</param>
    public void ReportCoreProblem(string what) => OnProblem(what);

    /// <summary>What was kept under a key for the next launch, or null.</summary>
    /// <param name="key">The key.</param>
    /// <returns>The value.</returns>
    internal string? Recall(string key) => kept.Read(key);

    /// <summary>
    /// Keeps a value under a key for the next launch, in order with the rest, or takes the key away when there is none. Nothing is kept while a
    /// sign-out is under way, and nothing asked before one is kept after it (<see cref="SessionKeeper"/>).
    /// </summary>
    /// <param name="key">The key.</param>
    /// <param name="value">The value.</param>
    internal void Remember(string key, string? value) => session.Write(key, value);

    /// <summary>
    /// Keeps what the core says it needs to start again where the person was. A snapshot that says it holds nobody (<c>person_unknown</c>) is not
    /// kept, so a visitor's blank one never replaces a person's; nor is one asked for while a sign-out is under way, or one whose session ended
    /// while it was being made.
    /// </summary>
    /// <returns>Whether a snapshot was given to be kept.</returns>
    public async Task<bool> KeepSnapshotAsync()
    {
        if (session.Stamp() is not long stamp)
        {
            return false;
        }

        string? snapshot;
        try
        {
            snapshot = await client.SnapshotAsync().ConfigureAwait(true);
        }
        catch (Exception)
        {
            return false;
        }

        if (!SnapshotRules.ShouldKeep(snapshot))
        {
            return false;
        }

        session.Write(stamp, SnapshotKey, snapshot);
        return true;
    }

    // The core asks for a sign-in in the system's browser: the one the person is signed in to Windows with, which shares what it keeps with the
    // person's other sites. A session that must share nothing cannot be had in it, so it is not started.
    private void BeginSignIn(SignInEffect effect)
    {
        SignInReturn? comesBack = SignInReturn.Parse(effect.Callback);
        string? why = comesBack is null
            ? "the return is not one this build knows"
            : effect.Ephemeral
                ? "a session that shares nothing with the browser cannot be had in the system's browser"
                : !SignInRules.MayStart(effect.Url)
                    ? "the address is not one a browser may be sent to"
                    : comesBack.WhyNot();
        if (why is not null || comesBack is null)
        {
            Problem?.Invoke("a sign-in was not started: " + why);
            client.Send(Events.SignInDone(null));
            return;
        }

        StopLoopback();
        callbackText = effect.Callback;
        returnKind = comesBack.Kind;
        if (comesBack.Kind == ReturnKind.Loopback)
        {
            try
            {
                loopback = LoopbackReturn.Start(comesBack.Port);
                loopbackWait = new CancellationTokenSource();
                comesBack = comesBack with { Port = loopback.Port };
                _ = WaitForLoopbackAsync(loopback, loopbackWait.Token);
            }
            catch (Exception problem) when (problem is System.Net.Sockets.SocketException or InvalidOperationException)
            {
                Problem?.Invoke("a sign-in was not started: the machine would not listen for its return");
                client.Send(Events.SignInDone(null));
                return;
            }
        }

        TimeSpan limit = TimeSpan.FromMinutes(10);
        int ticket = signIn.Begin(comesBack, DateTimeOffset.UtcNow, limit);
        _ = OpenSignInAsync(effect.Url, ticket, limit);
    }

    private async Task OpenSignInAsync(string address, int ticket, TimeSpan limit)
    {
        bool opened = false;
        try
        {
            opened = await Windows.System.Launcher.LaunchUriAsync(new Uri(address));
        }
        catch (Exception problem) when (problem is UriFormatException or InvalidOperationException or System.Runtime.InteropServices.COMException)
        {
            opened = false;
        }

        if (!opened)
        {
            EndSignIn(ticket, null);
            return;
        }

        try
        {
            await Task.Delay(limit, stopped.Token).ConfigureAwait(true);
        }
        catch (OperationCanceledException)
        {
            return;
        }

        if (signIn.End(DateTimeOffset.UtcNow) is int ended && ended == ticket)
        {
            StopLoopback();
            client.Send(Events.SignInDone(null));
        }
    }

    private async Task WaitForLoopbackAsync(LoopbackReturn listening, CancellationToken cancel)
    {
        string? address = await listening.WaitAsync(cancel).ConfigureAwait(true);
        if (address is not null && ReferenceEquals(loopback, listening))
        {
            HandleReturn(address);
        }
    }

    private void StopLoopback()
    {
        loopbackWait?.Cancel();
        loopbackWait?.Dispose();
        loopbackWait = null;
        loopback?.Dispose();
        loopback = null;
    }

    // The sign-in ended: with an address it came back on, or with none when it could not be opened or the time was up.
    private void EndSignIn(int ticket, string? address)
    {
        if (signIn.Serial != ticket)
        {
            return;
        }

        if (address is null && signIn.End(DateTimeOffset.UtcNow, force: true) is null)
        {
            return;
        }

        StopLoopback();
        client.Send(Events.SignInDone(address));
    }

    /// <summary>
    /// An address came back to the app on its own scheme (or on the loopback port). When a sign-in is open and it is that sign-in's return, the sign-in
    /// ends with it; any other address of the app's scheme is handed to the core as a link from outside, which uses it only if it is a return
    /// of a sign-in it has open, and says so when it did not.
    /// </summary>
    /// <param name="address">The whole address.</param>
    public void HandleReturn(string address)
    {
        ArgumentNullException.ThrowIfNull(address);

        // The platform-independent rule of the address (no white space, no control character, the callback's own scheme) is asked first.
        bool plausible = returnKind != ReturnKind.Scheme || Origins.SignInReturn(address, callbackText);
        if (plausible && signIn.Offer(address, out _) == SignInOffer.Taken)
        {
            StopLoopback();
            client.Send(Events.SignInDone(address));
            return;
        }

        client.Send(Events.OpenLink(address));
    }

    // A call from a tap or from the server's reply that is carried out with the system.
    private void RunDevice(DeviceEffect device)
    {
        Models.Call call = Models.ReadCall(device.Device);
        switch (call)
        {
            case Models.SetBadge badge:
                Place?.SetBadge(badge.Count);
                Answer(device);
                break;
            case Models.Notify notify:
                Place?.Notify(notify);
                Answer(device);
                break;
            case Models.CancelNotification cancel:
                Place?.CancelNotification(cancel.Id);
                Answer(device);
                break;
            case Models.OpenSettings settings:
                _ = OpenSettingsAsync(settings.Page);
                Answer(device);
                break;
            case Models.Share share when device.Posts:
                _ = ShareAsync(device, share);
                break;
            default:
                if (device.Posts)
                {
                    client.Send(Events.DeviceRefused(device.Call, "not_built"));
                }

                break;
        }
    }

    private void Answer(DeviceEffect device)
    {
        if (device.Posts)
        {
            client.Send(Events.DeviceDone(device.Call, []));
        }
    }

    private async Task ShareAsync(DeviceEffect device, Models.Share share)
    {
        if (Place is null)
        {
            client.Send(Events.DeviceRefused(device.Call, "unavailable"));
            return;
        }

        bool? completed = await Place.ShareAsync(share, Origins.Ours).ConfigureAwait(true);
        client.Send(completed is bool done
            ? Events.DeviceDone(device.Call, [new KeyValuePair<string, string>("completed", done ? "true" : "false")])
            : Events.DeviceRefused(device.Call, "unavailable"));
    }

    private static async Task OpenSettingsAsync(Models.SettingsPage page)
    {
        string address = page == Models.SettingsPage.Notifications ? "ms-settings:notifications" : "ms-settings:appsfeatures";
        try
        {
            _ = await Windows.System.Launcher.LaunchUriAsync(new Uri(address));
        }
        catch (Exception problem) when (problem is InvalidOperationException or System.Runtime.InteropServices.COMException)
        {
            // Windows' own settings would not open: nothing more to do.
        }
    }

    // Offered to the person once: the core has stopped and nothing it held can be used again.
    private void NoteStop(string what)
    {
        if (coreStopped || !CoreFault.IsStop(what))
        {
            return;
        }

        coreStopped = true;
        CoreStopped?.Invoke();
    }

    /// <summary>Starts the app again as a new process and ends this one: the way out when the core has stopped.</summary>
    public void Restart()
    {
        string? exe = Environment.ProcessPath;
        if (exe is null)
        {
            return;
        }

        try
        {
            Microsoft.Windows.AppLifecycle.AppInstance.GetCurrent().UnregisterKey();
        }
        catch (Exception)
        {
            // The key is released with the process in any case.
        }

        try
        {
            _ = Process.Start(new ProcessStartInfo(exe) { UseShellExecute = false });
        }
        catch (Exception problem) when (problem is InvalidOperationException or System.ComponentModel.Win32Exception)
        {
            Problem?.Invoke("the app could not be started again");
            return;
        }

        Microsoft.UI.Xaml.Application.Current.Exit();
    }
}
