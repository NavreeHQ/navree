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
    private readonly HashSet<long> runningShellRequests = [];
    private readonly List<TaskCompletionSource<FetchResult>> pendingShellChecks = [];
    private readonly Dictionary<long, CancellationTokenSource> timers = [];
    private readonly Dictionary<long, CancellationTokenSource> streams = [];
    private readonly SessionKeeper session;
    private readonly SessionGate asked = new();
    private readonly PersonChange people;
    private readonly WebProfiles webProfiles;
    private readonly List<string> startNotes = [];
    private bool started;
    private Task? signingOut;

    // The mark of the state the windows were last given, on the window's thread (SessionGate.OfSomeoneLeaving).
    private long shownMark;

    // The core's word that a session is ending and its word that the session is forgotten, paired: which of them a sign-out begins at.
    private readonly EndingWords words;
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
        // person who left is taken in. No web view runs yet, so the web view's folders can be deleted; one that cannot be is never used
        // again, because the session of this run keeps its web data in a folder of its own (WebProfiles).
        session = new SessionKeeper(kept, cookies);
        webProfiles = new WebProfiles(options.WebFolder ?? WebData.DefaultFolder(options.AppName), say: what => dispatcher.TryEnqueue(() => Note(what)));
        bool webNamed = true;
        FinishedSignOutAtStart = session.Open(() =>
        {
            Files.Clear();
            return webNamed = webProfiles.Choose(lastSessionEnded: true);
        });
        if (!FinishedSignOutAtStart)
        {
            webNamed = webProfiles.Choose(lastSessionEnded: false);
        }

        // Whom what is kept was kept for, as the last run left it: when this run's core names somebody else, what is kept is not theirs.
        // A start that reads no mark and finds something kept all the same (a file of the person's, or anything in the web view's folder)
        // cannot say whose it is: the first person named is then taken for another, and it goes before they see anything.
        string? keptFor = session.KeptPerson();
        bool keptForNobodyKnown = keptFor is null && (session.KeepsOfAPerson() || webProfiles.HoldsData);
        words = new EndingWords(asked);
        people = new PersonChange(
            new PersonWatch(keptFor, keptForNobodyKnown),
            asked,
            session,
            new PersonChangeParts(
                MoveWebOn: webProfiles.MoveOn,
                DropPictures: PictureCache.Clear,

                // Off the window's thread, as at a sign-out: the copies of chosen photographs are files to delete.
                ClearFiles: () => Task.Run(Files.ClearAsync),
                TakeFromSystem: () => Place?.PersonChangedAsync() ?? Task.CompletedTask,
                EmptyWeb: EndWebAsync,
                OnWindow: OnUi,
                More: () => moreWipeSteps,

                // The first person named since a sign-out or a clean start gets what waited for nobody; when the person before returns
                // after nobody was named, what waited for nobody is let go of so a newcomer's reminder never reaches the person before.
                GiveWaitingTo: mark => Place?.GiveWaitingTo(mark),
                DropWaitingForNobody: () => Place?.DropWaitingForNobody()),
            ReportProblem);

        // Only naming the session's folder has to be done before a web view starts. The older folders are deleted off the window's thread: a
        // profile of thousands of files, or one whose browser is still ending, would otherwise keep the window from showing.
        if (webNamed)
        {
            TimeSpan each = FinishedSignOutAtStart ? WebProfiles.AtStart : WebProfiles.LeftOver;
            _ = Task.Run(() => webProfiles.Tidy(each));
        }

        // The list of the files set aside was there and could not be trusted: nothing that was kept was read, and the start is clean.
        if (kept.SetAsideListWasLost)
        {
            Note("the list of kept files that were set aside could not be read: everything that was kept was set aside and nothing of it was read");
        }

        // A file of a session that ended which another program holds does not stop the start: it is set aside and never read.
        if (session.SetAsideAtStart > 0)
        {
            Note(session.SetAsideAtStart + " kept file(s) of a session that ended could not be deleted yet: they are set aside, never read, and deleted at a later start");
        }

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
    /// <returns>When the wipe has finished and the core has let go. A sign-out asked for while one is running is that one.</returns>
    public Task SignOutHereAsync() => signingOut is { IsCompleted: false } running ? running : signingOut = EndSessionAsync(coreEnded: false);

    /// <summary>Whether the last run left a sign-out unfinished and this start finished it before reading anything.</summary>
    public bool FinishedSignOutAtStart { get; }

    /// <summary>Whether a sign-out's steps are running: the web view asks nothing of the network meanwhile.</summary>
    internal bool Wiping => session.Wiping;

    /// <summary>
    /// Whether the state the windows show is that of a person who is signing out: the sign-out has begun and the core has not let go of
    /// them, or its word that it has is not shown yet. A web page such a screen asks for is not loaded: the web view's folder is already
    /// that of whoever comes next, and nothing of the person who is leaving is put there.
    /// </summary>
    internal bool ShowsSomeoneLeaving => asked.OfSomeoneLeaving(shownMark);

    /// <summary>
    /// The folder the web views of the session now keep their data in; null when no web page may be shown in this run, because what the web
    /// view kept for the person before (one who signed out, or one whose place another person took with no sign-out between) could not be
    /// taken away.
    /// </summary>
    internal string? WebFolder => webProfiles.Folder;

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
        // What the start had to say before anybody could listen is said now.
        started = true;
        foreach (string note in startNotes)
        {
            Problem?.Invoke(note);
        }

        startNotes.Clear();
        string? saved = StartRules.SnapshotToRestore(kept);
        if (SnapshotRulesAllow(saved))
        {
            _ = RestoreAsync(saved!);
            return;
        }

        // No snapshot is given to the core, so none is being put back.
        StartRules.Restored(kept);
        client.Send(Events.Start);
    }

    // Gives the core the snapshot. The note that it is being put back (written before this, StartRules) is taken away only when the core has
    // answered and the window's thread has carried the answer out: a start that ends anywhere before that begins clean the next time.
    private async Task RestoreAsync(string saved)
    {
        try
        {
            _ = await client.SendAsync(Events.Restore(saved));
        }
        catch (Exception)
        {
            // The core did not get to the end of it: the note stays.
            return;
        }

        // Behind the update the core answered with, which was handed to the window's thread before the answer came back here.
        if (!await OnDispatcherAsync())
        {
            return;
        }

        _ = dispatcher.TryEnqueue(() => StartRules.Restored(kept));
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

    // A problem line of the host's own, on the UI thread: kept until the app is started when nobody listens yet.
    private void Note(string what)
    {
        if (started)
        {
            Problem?.Invoke(what);
        }
        else
        {
            startNotes.Add(what);
        }
    }

    // On the core's thread: the state is handed to the UI thread, and then each effect, in order.
    // A window that cannot show the state, or an effect that fails, does not cost the rest of the update. Each effect is marked here, as it is
    // taken off the core, with the session it was asked in: what was asked for a person who has signed out by the time the UI thread gets
    // round to it is not started and not kept (SessionGate).
    private void OnUpdate(Update update)
    {
        long[] marks = asked.Asked(update.Effects, out long stateMark);
        int at = 0;
        dispatcher.TryEnqueue(() =>
            Delivery.Deliver(
                update,
                state =>
                {
                    // Whose state the windows are about to show: a web page its screens ask for is loaded only when it is not the state
                    // of a person who is signing out (ShowsSomeoneLeaving).
                    shownMark = stateMark;

                    // Who it names is heard before the windows are given it, and before any effect of its update: when it is another
                    // person than the one before, what the client keeps of its own for that one has begun to go by then (PersonChange).
                    Delivery.Show(
                        state,
                        heard => _ = people.Hear(heard, stateMark),
                        shown =>
                        {
                            WebPage.TakeDownUnder(shown);
                            StateChanged?.Invoke(shown);
                        },
                        what => Problem?.Invoke(what));
                },
                effect =>
                {
                    long mark = marks[at++];
                    Run(effect, update.State, mark);
                },
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

    private void Run(Effect effect, ViewState state, long mark)
    {
        // What the core asked for a person who has signed out since, or asks for one who is signing out, is not started and not kept.
        if (!asked.MayRun(effect, mark))
        {
            return;
        }

        switch (effect)
        {
            case FetchEffect fetch:
                _ = FetchAsync(fetch, mark);
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
                session.ReadCached(read.Key, value => client.Send(Events.Cache(read.Key, value)));
                break;
            case CacheWriteEffect write:
                session.WriteCached(write.Key, write.Value);
                break;
            case CacheClearEffect:
                session.ClearCache();
                break;
            case AnnounceEffect announce:
                string? said = announce.Text ?? (announce.Notice is Notice notice ? Words.Notice(ViewReading.NoticeWord(notice)) : null);
                if (said is not null)
                {
                    Announcement?.Invoke(said);
                }

                break;
            case CopyEffect copy:
                SystemClipboard.Put(copy.Text, copy.Sensitive);
                break;
            case TimerEffect timer:
                _ = TimerAsync(timer, mark);
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
                    _ = StreamAsync(open, mark);
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
            case SessionEndingEffect:
                // The core has asked the server for one last thing and lets go when that is answered. The sign-out begins here, without
                // waiting for the answer: its first part (the session's number, the web view's folder, the mark, the cookies out of memory,
                // the work under way stopped but for that one request) is done before the next effect of this update runs. When the session
                // the core speaks of has ended on the window's side already (the person signed out in the client first), no second sign-out
                // begins: the word is only counted, and the word that follows it finds nothing to begin (EndingWords).
                if (words.Ending(mark) is Task coreLetsGoLater)
                {
                    _ = EndSessionAsync(coreEnded: true, coreLetsGoLater);
                }

                break;
            case ForgetSessionEffect:
                // When the core said before that the session was ending, a sign-out began at that word or the word was only counted:
                // either way one is done or under way, it is told that the core has let go, and none begins here. Otherwise it begins
                // here, and its first part (the cookies out of memory, the work under way stopped) is done before the next effect of this
                // update runs.
                if (words.Forgotten())
                {
                    _ = EndSessionAsync(coreEnded: true);
                }

                break;
            default:
                Problem?.Invoke("an effect this build does not know: " + (effect is UnknownEffect unknown ? unknown.Tag : effect.GetType().Name));
                break;
        }
    }

    private async Task FetchAsync(FetchEffect fetch, long mark)
    {
        bool isShell = IsShellRequest(fetch);
        CancellationTokenSource work = CancellationTokenSource.CreateLinkedTokenSource(stopped.Token);
        running[fetch.Request] = work;
        if (isShell)
        {
            _ = runningShellRequests.Add(fetch.Request);
        }

        FetchResult? result = null;
        try
        {
            // The last request of a person who is signing out is made here, on the window's thread, so that it holds the cookies of this
            // moment: the next effect of the same update begins the sign-out, whose first act takes the cookies out of memory. Every other
            // request is made off the window's thread.
            Task<FetchResult> sending = asked.IsLast(fetch.Request)
                ? Delivery.FetchAsync(fetcher, fetch, work.Token)
                : Task.Run(() => Delivery.FetchAsync(fetcher, fetch, work.Token));
            result = await sending.ConfigureAwait(false);
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
        if (isShell)
        {
            _ = runningShellRequests.Remove(fetch.Request);
        }

        work.Dispose();

        // An answer to a request of a person who signed out meanwhile is told to nobody, and nothing of it is kept. The one answer the core
        // is told all the same is that of the leaving person's last request: the core waits for it before it lets go. Nothing of that
        // answer is kept either (its cookies are refused by the store, and the keeper writes nothing while the sign-out is under way).
        bool last = asked.LastEnded(fetch.Request);
        if (result is null || stopped.IsCancellationRequested || !(asked.Live(mark) || last))
        {
            if (isShell && pendingShellChecks.Count > 0)
            {
                CompleteShellChecks(new FetchResult(0, null, null, [], string.Empty, "cancelled"));
            }

            return;
        }

        if (result.Error is null
            && result.Status is >= 200 and < 300
            && IsMainOrigin(fetch.Url)
            && Navree.Presence.NotifyRules.IsNativeMediaType(result.ContentType))
        {
            LastHeardFromServer = DateTimeOffset.Now;
        }

        string answer = result.Error is not null
            ? Events.ResponseFailed(fetch.Request, result.Error)
            : Events.Response(fetch.Request, result.Status, result.ContentType, result.Location, result.Headers, result.Body);

        if (isShell && pendingShellChecks.Count > 0)
        {
            // A due reminder is waiting for this shell answer: wait for the core to handle it and for the UI thread to hear who is signed in
            // before waking the reminder check.
            try
            {
                _ = await client.SendAsync(answer).ConfigureAwait(false);
            }
            catch (ObjectDisposedException)
            {
            }

            if (!await OnDispatcherAsync())
            {
                return;
            }

            await YieldDispatcherAsync().ConfigureAwait(true);
            session.KeepCookies();
            CompleteShellChecks(result);
            return;
        }

        client.Send(answer);

        // What the cookie store holds is kept for the next launch, protected for the Windows user (see KeptStore for what that does and does not keep out).
        session.KeepCookies();
    }

    private async Task StreamAsync(StreamOpenEffect open, long mark)
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

        // The core closed it itself (it is no longer listed), the person it was opened for signed out, or the app is ending: nobody is told it ended.
        if (streams.Remove(open.Stream) && !stopped.IsCancellationRequested && asked.Live(mark))
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

    private async Task TimerAsync(TimerEffect timer, long mark)
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
        if (ran && asked.Live(mark))
        {
            timers.Remove(timer.Timer);
            client.Send(Events.Timer(timer.Timer));
        }
    }

    // The end of a session, on the UI thread: everything that writes for the person is stopped or refused at once, then what could rebuild the
    // session goes first, then the rest, each step with a limit (SessionKeeper). The core is told when the client ended it; when the core ended it,
    // it has let go already, or (when it only said the session is ending) lets go later, and the sign-out is told when it does.
    private async Task EndSessionAsync(bool coreEnded, Task? coreLetsGoLater = null)
    {
        // Already there when the core's effect or the window asks (then nothing waits); otherwise it goes there first.
        if (!await OnDispatcherAsync())
        {
            return;
        }

        // From here on only what the core asks for whoever comes next is started or kept. When the client ends the session, the core is the
        // leaving person's until it is told: the point is marked on the core's own thread, just before it is told.
        asked.SessionIsEnding();

        // The sign-out takes everything of the person away: whoever is named after it is the first, not another person with no sign-out between.
        people.SignedOut();

        // Whether the core has been told (or ended the session itself). The window's side has moved on to whoever comes next; until the core's
        // side has too, everything the core asks is taken for the leaving person's and refused. The two always come in a pair.
        int told = coreEnded ? 1 : 0;
        Task TellCore()
        {
            Volatile.Write(ref told, 1);
            return client.SendAsync(Events.SignedOut, () =>
            {
                asked.CoreIsToldTheSessionEnded();

                // The pictures the person's screens drew since the sign-out began go on the window's thread before the core's answer is
                // shown there: a screen of whoever comes next is never handed a picture that was drawn for the person who left.
                _ = dispatcher.TryEnqueue(PictureCache.Clear);
            });
        }

        WipeReport report;
        try
        {
            // The web view's folder moves on first, before any step and before any wait: a web page asked for from this line on is given the
            // folder of whoever comes next, or none, and never the folder of the person who is leaving, however long emptying that one takes.
            string webEnded = webProfiles.MoveOn();
            WipeStep[] web = [new WipeStep("what the web view keeps", () => EndWebAsync(webEnded), OnUi)];
            WipeStep[] ofTheWindows =
            [
                .. new[]
                {
                    // A photograph still being read when the person signed out is waited for: it is refused, and its copy is gone, when this is done.
                    new WipeStep("the files chosen for forms", Files.ClearAsync),

                    // What the system shows or holds for the person (notifications waiting and shown, the count, the jump list), before the windows' own.
                    new WipeStep("the notifications, the count and the jump list", () => Place?.EndSessionAsync() ?? Task.CompletedTask, OnUi),
                }.Concat(moreWipeSteps),
            ];

            // When the core said the session is ending and lets go later, everything runs at once and nothing waits for it; nothing runs a
            // second time when it does let go (the core closed the person's screens before it said so, so nothing is gained in between).
            report = coreLetsGoLater is null
                ? await session.EndAsync(StopWork, coreEnded ? null : TellCore, web, ofTheWindows).ConfigureAwait(false)
                : await session.EndBeforeCoreAsync(StopWork, coreLetsGoLater, web, ofTheWindows).ConfigureAwait(false);
        }
        catch (Exception problem)
        {
            report = new WipeReport([], [new KeyValuePair<string, string>("the start of the sign-out", problem.GetType().Name)]);
        }
        finally
        {
            // Whatever the lines above threw, a sign-out that began is one the core is told of: otherwise the app would ask for nothing again
            // until it is started again. The keeper tells the core itself whatever its own start throws, so this is for what nobody foresaw.
            if (Volatile.Read(ref told) == 0)
            {
                try
                {
                    await TellCore().WaitAsync(Wipe.StepLimit).ConfigureAwait(false);
                }
                catch (Exception)
                {
                    // The core did not answer. The report says the sign-out did not finish, and the person is told to start the app again.
                }
            }
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

    // What the web view kept for the person who is leaving is taken away: its profile cleared through a web view, or its folder deleted. The
    // folder itself moved on before this began (WebProfiles.MoveOn), so however this ends, and however long it takes, the folder is not used
    // again. When it could be neither cleared nor deleted the step fails (so the mark of an unfinished sign-out stays) and no web page is shown
    // for the rest of this run; that is settled whatever the emptying threw.
    private async Task EndWebAsync(string ended)
    {
        bool taken = false;
        bool finished = false;
        try
        {
            taken = await WebPage.ClearAllAsync(ended, webProfiles.Folder, what => ReportProblem("web wipe " + what));
        }
        finally
        {
            // Off the window's thread: deleting what is left of older folders takes as long as it takes, and the window is not kept waiting.
            finished = await Task.Run(() => webProfiles.Finish(ended, taken));
        }

        if (!finished)
        {
            throw new IOException("what the web view keeps could not be taken away");
        }
    }

    // On the UI thread, before the first step of a sign-out: every request, stream and timer under way for the person is cancelled and forgotten
    // (nothing of it is told to the core), and the snapshot timer stops until the sign-out is over.
    private void StopWork()
    {
        foreach (Dictionary<long, CancellationTokenSource> work in new[] { running, streams, timers })
        {
            foreach ((long id, CancellationTokenSource source) in work.ToArray())
            {
                // The last request of the person who is leaving is let through: the core waits for its answer before it lets go.
                if (ReferenceEquals(work, running) && asked.IsLast(id))
                {
                    continue;
                }

                try
                {
                    source.Cancel();
                }
                catch (Exception)
                {
                    // It had ended, or something that waited on it threw as it was told. It is cancelled either way, and the rest still are.
                }

                _ = work.Remove(id);
            }
        }

        // The pictures drawn for the person are let go of now, again just before the core is told when the client ends the session (the
        // person's screens are up until then), and in the windows' own step: they are held by their address alone, and an address may be
        // the same for whoever comes next (a person's own picture at one address for everyone).
        runningShellRequests.Clear();
        CompleteShellChecks(new FetchResult(0, null, null, [], string.Empty, "cancelled"));
        PictureCache.Clear();
        Place?.HoldSnapshots(true);
    }

    // Asks what the app answers by itself first, and the fetcher behind it for the rest.
    private sealed class FirstAnswer(Func<FetchEffect, FetchResult?> first, IFetcher behind) : IFetcher
    {
        public Task<FetchResult> FetchAsync(FetchEffect effect, CancellationToken cancel) =>
            first(effect) is FetchResult answer ? Task.FromResult(answer) : behind.FetchAsync(effect, cancel);
    }
}
