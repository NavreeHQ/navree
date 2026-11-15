using System.Security.Cryptography;
using Microsoft.UI.Dispatching;
using Navree.Design;
using Navree.Platform;
using Navree.Runtime;

namespace Navree.Shell;

/// <summary>
/// The app's one core and everything that carries out what it asks, for every window: the view state is the app's,
/// not a window's. A window subscribes to <see cref="StateChanged"/>, shows what the state says and tells the host
/// what the person did; it keeps no state of its own about navigation, trees, forms or loading.
/// </summary>
public sealed class ShellHost : IDisposable
{
    private readonly DispatcherQueue dispatcher;
    private readonly CoreClient client;
    private readonly CookieStore cookies;
    private readonly KeptStore kept;
    private readonly IFetcher fetcher;
    private readonly HttpFetcher? network;
    private readonly Dictionary<long, CancellationTokenSource> running = [];
    private readonly Dictionary<long, CancellationTokenSource> timers = [];
    private readonly SerialQueue disk = new();
    private readonly CancellationTokenSource stopped = new();
    private string lastCookies = string.Empty;

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
        if (options.Fetcher is null)
        {
            network = new HttpFetcher(Origins, cookies);
        }

        fetcher = options.Fetcher ?? network!;
        if (kept.Read("cookies") is string saved)
        {
            cookies.Import(saved);
            lastCookies = cookies.Export();
        }

        client = new CoreClient(ClientEnvironment.Config(options), OnUpdate, OnProblem);
    }

    /// <summary>What the app says.</summary>
    public ShellOptions Options { get; }

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

    /// <summary>Tells the core what happened.</summary>
    /// <param name="eventJson">An event, from <see cref="Events"/>.</param>
    public void Send(string eventJson) => client.Send(eventJson);

    /// <summary>Starts the app: the core asks for its shell.</summary>
    public void Start() => client.Send(Events.Start);

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
        client.Dispose();
        network?.Dispose();
        disk.Dispose();
    }

    // Runs a step that must be on the UI thread, and completes when it has been.
    private Func<Task> OnUiAsync(Func<Task> step) =>
        () =>
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
        };

    /// <summary>What a page in the web view is answered with when the app has recorded answers instead of a network.</summary>
    internal Func<Uri, (byte[] Body, string ContentType)?>? RecordedPages => Options.RecordedPages;

    private void OnProblem(string what) => dispatcher.TryEnqueue(() => Problem?.Invoke(what));

    // On the core's thread: the state is handed to the UI thread, and then each effect, in order.
    private void OnUpdate(Update update)
    {
        dispatcher.TryEnqueue(() =>
        {
            StateChanged?.Invoke(update.State);
            foreach (Effect effect in update.Effects)
            {
                try
                {
                    Run(effect, update.State);
                }
                catch (Exception problem)
                {
                    // One effect that fails, in any way, is reported by its kind alone and the rest go on.
                    Problem?.Invoke("an effect failed: " + effect.GetType().Name + ": " + problem.GetType().Name);
                }
            }
        });
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
            case SignInEffect:
                // Signing in through the system's browser is not carried out yet: the core is told the session did not finish.
                client.Send(Events.SignInDone(null));
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
                if (device.Posts)
                {
                    client.Send(Events.DeviceRefused(device.Call, "unsupported"));
                }

                break;
            case CacheReadEffect read:
                disk.Run(() => client.Send(Events.Cache(read.Key, kept.Read(read.Key))));
                break;
            case CacheWriteEffect write:
                disk.Run(() => kept.Write(write.Key, write.Value));
                break;
            case CacheClearEffect:
                disk.Run(kept.Clear);
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
            case StreamOpenEffect:
            case StreamCloseEffect:
                // Live streams are not carried out yet: a list that has one is kept up to date when it is asked again.
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
                _ = ForgetAsync();
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
        FetchResult result;
        try
        {
            result = await Task.Run(() => fetcher.FetchAsync(fetch, work.Token)).ConfigureAwait(true);
        }
        finally
        {
            running.Remove(fetch.Request);
            work.Dispose();
        }

        if (stopped.IsCancellationRequested)
        {
            return;
        }

        string answer = result.Error is not null
            ? Events.ResponseFailed(fetch.Request, result.Error)
            : Events.Response(fetch.Request, result.Status, result.ContentType, result.Location, result.Headers, result.Body);
        client.Send(answer);
        KeepCookies();
    }

    // What the cookie store holds is kept for the next launch, protected for this person on this device.
    private void KeepCookies()
    {
        string now = cookies.Export();
        if (now != lastCookies)
        {
            lastCookies = now;
            disk.Run(() => kept.Write("cookies", now));
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
        try
        {
            await Task.Delay(TimeSpan.FromMilliseconds(Math.Max(0, timer.Ms)), wait.Token).ConfigureAwait(true);
            timers.Remove(timer.Timer);
            client.Send(Events.Timer(timer.Timer));
        }
        catch (OperationCanceledException)
        {
            // The core no longer wants it.
        }
        finally
        {
            wait.Dispose();
        }
    }

    // The end of a session: what could rebuild it goes first, in order, each step with a limit.
    private async Task ForgetAsync()
    {
        WipeReport report = await Wipe.RunAsync(
            [
                new WipeStep("the cookies", () =>
                {
                    cookies.Clear();
                    lastCookies = string.Empty;
                    return Task.CompletedTask;
                }),
                new WipeStep("what is kept on the device", () =>
                {
                    kept.Clear();
                    return Task.CompletedTask;
                }),
                new WipeStep("what the web view keeps", OnUiAsync(WebPage.ClearAllAsync)),
            ]).ConfigureAwait(true);
        if (!report.Complete)
        {
            Problem?.Invoke("the wipe did not finish: " + string.Join(", ", report.Failed.Select(failed => failed.Key)));
        }

        SessionEnded?.Invoke(report);
    }
}
