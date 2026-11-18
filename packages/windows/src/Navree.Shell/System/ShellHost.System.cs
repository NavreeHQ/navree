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
    private const string SnapshotKey = StartRules.SnapshotKey;
    private readonly SignInTracker signIn = new();
    private LoopbackReturn? loopback;
    private CancellationTokenSource? loopbackWait;
    private bool coreStopped;

    /// <summary>The app's place in Windows: set by the app once its main window exists; the device calls that need a window are answered through it.</summary>
    public SystemPlace? Place { get; set; }

    /// <summary>Raised on the UI thread once, when the core says it has stopped for good: the app can only be started again.</summary>
    public event Action? CoreStopped;

    /// <summary>Whether the core has said it stopped.</summary>
    public bool HasStopped => coreStopped;

    /// <summary>
    /// Tells the host an answer of the core, as the core's own thread reads one: when it says the core has stopped
    /// (<see cref="CoreAnswers.IsStop(string)"/>), the person is told the app must be started again and offered it.
    /// </summary>
    /// <param name="answer">The core's answer, as JSON.</param>
    public void ReportCoreAnswer(string answer)
    {
        if (CoreAnswers.IsStop(answer))
        {
            _ = dispatcher.TryEnqueue(NoteStop);
        }
    }

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
    /// The mark of the person what is kept is kept for: the one named last, also while nobody is signed in; null when nobody was named
    /// since the app was first started or since the last sign-out (<see cref="PersonChange.KeptFor"/>). Before any state is heard it is
    /// the mark this start read, and null when it read none.
    /// </summary>
    internal string? KeptFor => people.KeptFor;

    /// <summary>The mark of the person who is signed in now, by the state shown last; null while nobody is (<see cref="PersonChange.Here"/>).</summary>
    internal string? PersonHere => people.Here;

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
    // person's other sites. A session that must share nothing cannot be had in it, so it is not started. While signing in through the browser is
    // not offered (BrowserSignIn), none is started at all: no browser is opened and no port is listened on.
    private void BeginSignIn(SignInEffect effect)
    {
        SignInReturn? comesBack = SignInReturn.Parse(effect.Callback);
        string? why = BrowserSignIn.WhyNotStart(BrowserSignIn.Offered) is string notOffered
            ? notOffered
            : comesBack is null
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
        int ticket = signIn.Begin(comesBack, DateTimeOffset.UtcNow, limit, SignInRules.QueryValue(effect.Url, "state"));
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
    /// An address came to the app on its own scheme (or on a loopback port). While signing in through the browser is not offered
    /// (<see cref="BrowserSignIn"/>) it is dropped: never opened as a page, never taken as a sign-in's return, and the core is not told of it.
    /// Once it is offered, it goes to the core as it came: as the return of the sign-in that is open, or as a link from outside when none is,
    /// and the core takes it only with that sign-in's <c>state</c>, on the way back that sign-in named.
    /// </summary>
    /// <param name="address">The whole address.</param>
    public void HandleReturn(string address)
    {
        ArgumentNullException.ThrowIfNull(address);
        switch (BrowserSignIn.Route(BrowserSignIn.Offered, signIn.IsOpen, address))
        {
            case Arrival.SignInDone:
                // The sign-in's own return ends the sign-in here too: it is no longer open, so what arrives after it is a link from outside,
                // and its time running out tells the core nothing more. Any other address leaves it open; the core is told either way.
                if (signIn.Offer(address, out _) == SignInOffer.Taken)
                {
                    StopLoopback();
                }

                client.Send(Events.SignInDone(address));
                break;
            case Arrival.OpenLink:
                client.Send(Events.OpenLink(address));
                break;
            default:
                // Never the address itself in what is said: a return can carry a code.
                Problem?.Invoke("an address that arrived on the app's scheme was not used: signing in through the browser is not offered");
                break;
        }
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
    private void NoteStop()
    {
        if (coreStopped)
        {
            return;
        }

        coreStopped = true;

        // The start that follows begins clean: the snapshot kept so far may be what brought the stop, and is not given back to the core.
        session.Write(StartRules.StoppedKey, "1");
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

        // What was asked to be kept is on disk before the next process reads it: the word that the core stopped, above all.
        try
        {
            _ = session.DrainAsync().Wait(TimeSpan.FromSeconds(2));
        }
        catch (AggregateException)
        {
            // The queue has ended: nothing more is written.
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
