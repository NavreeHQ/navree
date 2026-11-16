using System.Security.Cryptography;
using Microsoft.UI.Dispatching;
using Navree.Design;
using Navree.Platform;
using Navree.Protocol;
using Navree.Runtime;

namespace Navree.Shell;

/// <summary>
/// The app's one core and everything that carries out what it asks, for every window: the view state is the app's,
/// not a window's. A window subscribes to <see cref="StateChanged"/>, shows what the state says and tells the host
/// what the person did; it keeps no state of its own about navigation, trees, forms or loading.
/// </summary>
public sealed partial class ShellHost : IDisposable
{
    private readonly DispatcherQueue dispatcher;
    private readonly CoreClient client;
    private readonly CookieStore cookies;
    private readonly KeptStore kept;
    private readonly IFetcher fetcher;
    private readonly HttpFetcher? network;
    private readonly Dictionary<long, CancellationTokenSource> running = [];
    private readonly Dictionary<long, CancellationTokenSource> timers = [];
    private readonly Dictionary<long, CancellationTokenSource> streams = [];
    private readonly SessionKeeper session;
    private readonly CancellationTokenSource stopped = new();
    private readonly EnvironmentWatcher environment;
    private readonly ForegroundTracker foreground = new();
    private readonly List<WipeStep> moreWipeSteps = [];

    /// <summary>Makes the host and its core. Nothing is sent to the core until <see cref="Start"/>.</summary>
    /// <param name="options">What the app says.</param>
    /// <param name="dispatcher">The UI thread's queue: state and effects that touch the window are handed to it.</param>
    public ShellHost(ShellOptions options, DispatcherQueue dispatcher)
    {
        ArgumentNullException.ThrowIfNull(options);
        ArgumentNullException.ThrowIfNull(dispatcher);
        this.dispatcher = dispatcher;
        Options = options;
        Origins = new Origins(options.Origin, options.Also, options.LoopbackHttp);
        cookies = new CookieStore(Origins);
        kept = new KeptStore(options.KeptFolder ?? KeptStore.DefaultFolder(options.AppName), options.AppName);
        Files = new PickedFiles(Path.Combine(options.KeptFolder ?? KeptStore.DefaultFolder(options.AppName), "files"));
        FilePicker = options.FilePicker ?? new SystemFilePicker();
        Files.WentMissing += tokens => _ = dispatcher.TryEnqueue(() => FilesWentMissing?.Invoke(tokens));
        if (options.Fetcher is null)
        {
            network = new HttpFetcher(Origins, cookies, files: Files.Open);
        }

        IFetcher answering = options.Fetcher ?? network!;
        fetcher = options.AnswerFirst is null ? answering : new FirstAnswer(options.AnswerFirst, answering);

        // A sign-out the last run did not finish is finished before anything kept is read: no cookie, no page and no web data of the
        // person who left is taken in. No web view runs yet, so the web view's folder can be deleted.
        session = new SessionKeeper(kept, cookies);
        string webFolder = options.WebFolder ?? WebData.DefaultFolder(options.AppName);
        FinishedSignOutAtStart = session.Open(() =>
        {
            Files.Clear();
            return WebData.DeleteAsync(webFolder, TimeSpan.FromSeconds(3)).GetAwaiter().GetResult();
        });

        client = new CoreClient(ClientEnvironment.Config(options), OnUpdate, OnProblem, () => dispatcher.TryEnqueue(NoteStop));

        // What the system says is told to the core when it changes, not only when the core is made.
        environment = new EnvironmentWatcher(
            dispatcher,
            ClientEnvironment.Settings(),
            change => client.Send(Events.Environment(change.Dark, change.Contrast, change.TextPercent, change.Locale, change.TimeZone)));
    }

    /// <summary>What the app says.</summary>
    public ShellOptions Options { get; }

    /// <summary>The files the person chose for forms, held under the tokens the core knows them by.</summary>
    public PickedFiles Files { get; }

    /// <summary>What opens when a person chooses files for a field. A run that must not open the system's own picker replaces it.</summary>
    public IFilePicker FilePicker { get; set; }

    /// <summary>Raised on the UI thread with the tokens of files that could not be read when a request was written.</summary>
    public event Action<IReadOnlyList<string>>? FilesWentMissing;

    /// <summary>The app's origins.</summary>
    public Origins Origins { get; }

    /// <summary>The newest state the core's thread finished.</summary>
    public ViewState State => client.State;

    /// <summary>Raised on the UI thread each time the view state changed, before the effects of the same update are carried out.</summary>
    public event Action<ViewState>? StateChanged;

    /// <summary>Raised on the UI thread when the core asks for a short message to be shown.</summary>
    public event Action<Message>? MessageRequested;

    /// <summary>Raised on the UI thread when the core says something to assistive technology.</summary>
    public event Action<string>? Announcement;

    /// <summary>Raised when a problem of the client's own (not the server's) is worth the developers' knowing.</summary>
    public event Action<string>? Problem;

    /// <summary>Raised on the UI thread when the session ended and what was kept has been wiped.</summary>
    public event Action<WipeReport>? SessionEnded;

    /// <summary>
    /// Whether the client offers a way out of its own: somebody is signed in, the app has a sign-in, and the menu the server gave
    /// that person has none.
    /// </summary>
    public bool OffersOwnSignOut =>
        Options.Place.Scheme is not null && State.App?.User is Models.User user && !AccountRules.OffersSignOut(user.Menu);

    /// <summary>
    /// Signs the person out on this device: what could rebuild the session goes first (the cookies, the cookie file, what the web view
    /// keeps), then the core is told the client ended the session itself and starts again from the shell, then the rest is wiped
    /// (<see cref="SessionKeeper.EndAsync"/>).
    /// </summary>
    /// <returns>When the wipe has finished and the core has let go.</returns>
    public Task SignOutHereAsync() => EndSessionAsync(coreEnded: false);

    /// <summary>Whether the last run left a sign-out unfinished and this start finished it before reading anything.</summary>
    public bool FinishedSignOutAtStart { get; }

    /// <summary>Whether a sign-out's steps are running: the web view asks nothing of the network meanwhile.</summary>
    internal bool Wiping => session.Wiping;

    /// <summary>
    /// A window says whether it is shown (not minimised, not hidden). The core is told the app went to the background when the last
    /// window is no longer shown and that it came back when one is again; it is told nothing for a window opening beside another.
    /// Called on the UI thread.
    /// </summary>
    /// <param name="window">The window.</param>
    /// <param name="shown">Whether it is shown.</param>
    public void WindowShown(object window, bool shown)
    {
        if (foreground.Moved(window, shown) is bool front)
        {
            client.Send(front ? Events.Foreground : Events.Background);
            if (front)
            {
                // The person may have changed the system's settings, or its language, while the app was away.
                environment.Check();
            }
        }
    }

    /// <summary>
    /// Adds a step to the wipe at sign-out, after the host's own: something else the shell keeps of the person (the pictures of the pages
    /// it drew). Each step runs on the UI thread, where the window's state lives, has the wipe's limit, and a step that fails is reported with the rest.
    /// </summary>
    /// <param name="name">What it takes away, for the report.</param>
    /// <param name="run">Takes it away, on the UI thread.</param>
    public void AddWipeStep(string name, Func<Task> run) => moreWipeSteps.Add(new WipeStep(name, run, OnUi));

    /// <summary>Tells the core what happened.</summary>
    /// <param name="eventJson">An event, from <see cref="Events"/>.</param>
    public void Send(string eventJson) => client.Send(eventJson);

    /// <summary>Starts the app: the core asks for its shell, and is given the snapshot the last run kept, when there is one that reads.</summary>
    public void Start()
    {
        string? saved = kept.Read("snapshot");
        client.Send(SnapshotRulesAllow(saved) ? Events.Restore(saved!) : Events.Start);
    }

    private static bool SnapshotRulesAllow(string? saved) => Navree.Presence.SnapshotRules.ShouldKeep(saved);

    /// <summary>Waits until the core has handled everything sent so far, and returns the state.</summary>
    /// <returns>The whole view state.</returns>
    public Task<ViewState> SettledAsync() => client.ReadAllAsync();

    /// <summary>Loads the bytes of a picture by a whole address the core made.</summary>
    /// <param name="url">The address.</param>
    /// <returns>The bytes, or null.</returns>
    public Task<byte[]?> LoadPictureAsync(string url) =>
        network is null ? Task.FromResult<byte[]?>(null) : network.GetPictureAsync(url, stopped.Token);

    /// <inheritdoc />
    public void Dispose()
    {
        stopped.Cancel();
        environment.Dispose();
        client.Dispose();
        network?.Dispose();
        session.Dispose();
    }

    // Runs a step on the UI thread, where the window's state lives, and completes when it has run there (a wipe step's WipeStep.On).
    private Task OnUi(Func<Task> step)
    {
        TaskCompletionSource done = new(TaskCreationOptions.RunContinuationsAsynchronously);
        if (!dispatcher.TryEnqueue(async () =>
        {
            try
            {
                await step();
                done.SetResult();
            }
            catch (Exception problem)
            {
                done.SetException(problem);
            }
        }))
        {
            done.SetException(new InvalidOperationException("the window is gone"));
        }

        return done.Task;
    }

    /// <summary>What a page in the web view is answered with when the app has recorded answers instead of a network.</summary>
    internal Func<Uri, (byte[] Body, string ContentType)?>? RecordedPages => Options.RecordedPages;

    /// <summary>Reports a problem of the client's own from a part of the shell that is not the host (a dialog that could not be shown): it is raised as <see cref="Problem"/>.</summary>
    /// <param name="what">What went wrong, by its kind: never the server's words or a person's.</param>
    internal void ReportProblem(string what) => dispatcher.TryEnqueue(() => Problem?.Invoke(what));

    private void OnProblem(string what) => dispatcher.TryEnqueue(() => Problem?.Invoke(what));

    // On the core's thread: the state is handed to the UI thread, and then each effect, in order.
    // A window that cannot show the state, or an effect that fails, does not cost the rest of the update.
    private void OnUpdate(Update update)
    {
        dispatcher.TryEnqueue(() =>
            Delivery.Deliver(
                update,
                state => StateChanged?.Invoke(state),
                effect => Run(effect, update.State),
                what => Problem?.Invoke(what)));
    }

    // Goes on, after the await, on the dispatcher the host was given, whatever thread the caller finished on: the host's own state (the requests,
    // streams and timers under way) changes only there. What follows the await is itself the item handed to the dispatcher, so it runs there
    // and nowhere else. When the queue is gone (the end of the app) it does not go on at all.
    private ToDispatcher OnDispatcherAsync() => new(dispatcher);

    private readonly struct ToDispatcher(DispatcherQueue queue) : System.Runtime.CompilerServices.INotifyCompletion
    {
        public bool IsCompleted => queue.HasThreadAccess;

        public ToDispatcher GetAwaiter() => this;

        public void OnCompleted(Action continuation) => _ = queue.TryEnqueue(() => continuation());

        public bool GetResult() => queue.HasThreadAccess;
    }

    private void Run(Effect effect, ViewState state)
    {
        switch (effect)
        {
            case FetchEffect fetch:
                _ = FetchAsync(fetch);
                break;
            case CancelEffect cancel:
                if (running.Remove(cancel.Request, out CancellationTokenSource? work))
                {
                    work.Cancel();
                }

                break;
            case OpenUrlEffect open:
                _ = OpenAsync(open);
                break;
            case SignInEffect signInEffect:
                BeginSignIn(signInEffect);
                break;
            case ShowEffect show:
                if (state.Messages.FirstOrDefault(message => message.Id == show.Id) is Message message)
                {
                    MessageRequested?.Invoke(message);
                }

                break;
            case HapticEffect:
                break;
            case DeviceEffect device:
                RunDevice(device);
                break;
            case CacheReadEffect read:
                session.Read(read.Key, value => client.Send(Events.Cache(read.Key, value)));
                break;
            case CacheWriteEffect write:
                session.Write(write.Key, write.Value);
                break;
            case CacheClearEffect:
                session.ClearKept();
                break;
            case AnnounceEffect announce:
                string? said = announce.Text ?? (announce.Notice is Notice notice ? Words.Notice(NoticeName(notice)) : null);
                if (said is not null)
                {
                    Announcement?.Invoke(said);
                }

                break;
            case CopyEffect copy:
                SystemClipboard.Put(copy.Text, copy.Sensitive);
                break;
            case TimerEffect timer:
                _ = TimerAsync(timer);
                break;
            case CancelTimerEffect cancelTimer:
                if (timers.Remove(cancelTimer.Timer, out CancellationTokenSource? waiting))
                {
                    waiting.Cancel();
                }

                break;
            case StreamOpenEffect open:
                // With recorded answers there is no server to stream from: the stream stays opening, as one does that nobody answers.
                if (network is not null)
                {
                    _ = StreamAsync(open);
                }

                break;
            case StreamCloseEffect close:
                if (streams.Remove(close.Stream, out CancellationTokenSource? streaming))
                {
                    streaming.Cancel();
                }

                break;
            case ExitEffect:
                break;
            case ReportEffect report:
                Problem?.Invoke("report " + report.What + ": " + report.Detail);
                break;
            case RandomEffect random:
                client.Send(Events.Random(random.Request, Convert.ToHexStringLower(RandomNumberGenerator.GetBytes(Math.Clamp(random.Bytes, 1, 1024)))));
                break;
            case ForgetSessionEffect:
                // Its first part (the cookies out of memory, the work under way stopped) is done before the next effect of this update runs.
                _ = EndSessionAsync(coreEnded: true);
                break;
            default:
                Problem?.Invoke("an effect this build does not know: " + effect.GetType().Name);
                break;
        }
    }

    private static string NoticeName(Notice notice) =>
        notice switch
        {
            Notice.Offline => "offline",
            Notice.Timeout => "timeout",
            Notice.Failed => "failed",
            Notice.Unsaved => "unsaved",
            Notice.Loading => "loading",
            Notice.Copied => "copied",
            Notice.FieldsNeedAttention => "fields_need_attention",
            Notice.Loaded => "loaded",
            Notice.SecondStep => "second_step",
            _ => string.Empty,
        };

    private async Task FetchAsync(FetchEffect fetch)
    {
        CancellationTokenSource work = CancellationTokenSource.CreateLinkedTokenSource(stopped.Token);
        running[fetch.Request] = work;
        FetchResult? result = null;
        try
        {
            result = await Task.Run(() => Delivery.FetchAsync(fetcher, fetch, work.Token)).ConfigureAwait(false);
        }
        catch (OperationCanceledException)
        {
            // The core no longer wants the answer, or the app is ending.
        }

        if (!await OnDispatcherAsync())
        {
            return;
        }

        running.Remove(fetch.Request);
        work.Dispose();
        if (result is null || stopped.IsCancellationRequested)
        {
            return;
        }

        string answer = result.Error is not null
            ? Events.ResponseFailed(fetch.Request, result.Error)
            : Events.Response(fetch.Request, result.Status, result.ContentType, result.Location, result.Headers, result.Body);
        client.Send(answer);

        // What the cookie store holds is kept for the next launch, protected for the Windows user (see KeptStore for what that does and does not keep out).
        session.KeepCookies();
    }

    private async Task StreamAsync(StreamOpenEffect open)
    {
        CancellationTokenSource work = CancellationTokenSource.CreateLinkedTokenSource(stopped.Token);
        streams[open.Stream] = work;
        int status = 0;
        try
        {
            status = await Task.Run(() => network!.StreamAsync(
                open.Url,
                open.Headers,
                () =>
                {
                    client.Send(Events.StreamOpened(open.Stream));
                    return Task.CompletedTask;
                },
                name =>
                {
                    client.Send(Events.StreamEvent(open.Stream, name));
                    return Task.CompletedTask;
                },
                work.Token)).ConfigureAwait(false);
        }
        catch (OperationCanceledException)
        {
            // The core closed it, or the app is ending.
        }
        catch (Exception)
        {
            // A stream that broke ends as one that could not be kept open.
        }

        if (!await OnDispatcherAsync())
        {
            return;
        }

        work.Dispose();

        // The core closed it itself (it is no longer listed) or the app is ending: nobody is told it ended.
        if (streams.Remove(open.Stream) && !stopped.IsCancellationRequested)
        {
            client.Send(Events.StreamClosed(open.Stream, status));
        }
    }

    private async Task OpenAsync(OpenUrlEffect open)
    {
        if (!Origins.HandedToSystem(open.Url) || !Uri.TryCreate(open.Url, UriKind.Absolute, out Uri? address))
        {
            Problem?.Invoke("an address that may not be opened outside was refused");
            return;
        }

        _ = await Windows.System.Launcher.LaunchUriAsync(address);
    }

    private async Task TimerAsync(TimerEffect timer)
    {
        CancellationTokenSource wait = CancellationTokenSource.CreateLinkedTokenSource(stopped.Token);
        timers[timer.Timer] = wait;
        bool ran = false;
        try
        {
            await Task.Delay(TimeSpan.FromMilliseconds(Math.Max(0, timer.Ms)), wait.Token).ConfigureAwait(false);
            ran = true;
        }
        catch (OperationCanceledException)
        {
            // The core no longer wants it.
        }

        if (!await OnDispatcherAsync())
        {
            return;
        }

        wait.Dispose();
        if (ran)
        {
            timers.Remove(timer.Timer);
            client.Send(Events.Timer(timer.Timer));
        }
    }

    // The end of a session, on the UI thread: everything that writes for the person is stopped or refused at once, then what could rebuild the
    // session goes first, then the rest, each step with a limit (SessionKeeper). The core is told when the client ended it; when the core ended it,
    // it has let go already.
    private async Task EndSessionAsync(bool coreEnded)
    {
        // Already there when the core's effect or the window asks (then nothing waits); otherwise it goes there first.
        if (!await OnDispatcherAsync())
        {
            return;
        }

        WipeReport report;
        try
        {
            report = await session.EndAsync(
                StopWork,
                coreEnded ? null : () => client.SendAsync(Events.SignedOut),
                [new WipeStep("what the web view keeps", () => WebPage.ClearAllAsync(Options.WebFolder ?? WebData.DefaultFolder(Options.AppName), what => ReportProblem("web wipe " + what)), OnUi)],
                [.. new[]
                {
                    new WipeStep("the files chosen for forms", () =>
                    {
                        Files.Clear();
                        return Task.CompletedTask;
                    }),

                    // What the system shows or holds for the person (notifications waiting and shown, the count, the jump list), before the windows' own.
                    new WipeStep("the notifications, the count and the jump list", () => Place?.EndSessionAsync() ?? Task.CompletedTask, OnUi),
                }.Concat(moreWipeSteps)]).ConfigureAwait(false);
        }
        catch (Exception problem)
        {
            report = new WipeReport([], [new KeyValuePair<string, string>("the start of the sign-out", problem.GetType().Name)]);
        }

        if (!await OnDispatcherAsync())
        {
            return;
        }

        Place?.HoldSnapshots(false);
        if (!report.Complete)
        {
            Problem?.Invoke("the wipe did not finish: " + string.Join(", ", report.Failed.Select(failed => failed.Key)));
        }

        SessionEnded?.Invoke(report);
    }

    // On the UI thread, before the first step of a sign-out: every request, stream and timer under way for the person is cancelled and forgotten
    // (nothing of it is told to the core), and the snapshot timer stops until the sign-out is over.
    private void StopWork()
    {
        foreach (Dictionary<long, CancellationTokenSource> work in new[] { running, streams, timers })
        {
            foreach (CancellationTokenSource source in work.Values)
            {
                try
                {
                    source.Cancel();
                }
                catch (ObjectDisposedException)
                {
                    // It had ended.
                }
            }

            work.Clear();
        }

        Place?.HoldSnapshots(true);
    }

    // Asks what the app answers by itself first, and the fetcher behind it for the rest.
    private sealed class FirstAnswer(Func<FetchEffect, FetchResult?> first, IFetcher behind) : IFetcher
    {
        public Task<FetchResult> FetchAsync(FetchEffect effect, CancellationToken cancel) =>
            first(effect) is FetchResult answer ? Task.FromResult(answer) : behind.FetchAsync(effect, cancel);
    }
}
