using System.Collections.Immutable;
using System.Globalization;
using System.Text;
using System.Text.Json;
using System.Threading.Channels;
using Navree.Core;
using Navree.Protocol;

namespace Navree.Runtime;

/// <summary>What one event came to: the view state after it, and what the core asks for.</summary>
/// <param name="State">The view state after the event.</param>
/// <param name="Effects">What the core asks the client to do, in order.</param>
/// <param name="Answer">What the core answered, as it wrote it: <c>changed</c> and <c>effects</c>.</param>
public sealed record Update(ViewState State, IReadOnlyList<Effect> Effects, ReadOnlyMemory<byte> Answer);

/// <summary>
/// The loop of the runtime core: one event in, the parts that changed read back into the
/// view state, then the effects handed on, in order. The core lives on a thread of its own
/// with a stack large enough for the deepest tree the protocol allows, and every call to it,
/// every decoding of what it says and every finished view state is made there; the UI
/// thread is given immutable values and decodes nothing.
/// </summary>
/// <remarks>
/// The client keeps no state of its own about navigation, trees or forms: what it holds
/// is the core's view state, part by part, as it last said it. Events are handled one at
/// a time in the order they were sent, also those an effect answers at once.
/// <para>
/// A core that cannot be made ends the constructor with what went wrong; a library that cannot be loaded (not there, or for another
/// processor) is a <see cref="CoreUnavailableException"/> whose words name the file and where it was looked for. Work given after
/// <see cref="Dispose"/> began is never left waiting: a send is dropped, and work that is awaited completes with
/// <see cref="ObjectDisposedException"/>.
/// </para>
/// </remarks>
public sealed class CoreClient : IDisposable
{
    /// <summary>The stack the core's thread has: a tree as deep as the protocol allows needs more than a default thread has.</summary>
    public const int StackBytes = 4 * 1024 * 1024;

    private readonly Channel<Work> queue = Channel.CreateUnbounded<Work>(new UnboundedChannelOptions { SingleReader = true });
    private readonly Thread thread;
    private readonly TaskCompletionSource started = new(TaskCreationOptions.RunContinuationsAsynchronously);
    private NavreeCore? core;
    private ViewState state = ViewState.Empty;
    private volatile ViewState published = ViewState.Empty;

    /// <summary>Starts a core from a configuration, on a thread of its own.</summary>
    /// <param name="config">The configuration, as JSON.</param>
    /// <param name="onUpdate">
    /// Called on the core's thread after each event, with the new state and the effects, in order.
    /// It must not block: an effect that takes time is started, and what comes of it is sent as a later event.
    /// </param>
    /// <param name="onProblem">Called when the core or a part of its state does not read.</param>
    /// <param name="onStopped">
    /// Called once, on the core's thread, when an answer says the core has stopped for good after a failure inside it
    /// (<see cref="CoreAnswers.IsStop(JsonElement)"/>): the app must be started again.
    /// </param>
    public CoreClient(byte[] config, Action<Update> onUpdate, Action<string>? onProblem = null, Action? onStopped = null)
        : this(config, static bytes => new NavreeCore(bytes), onUpdate, onProblem, onStopped)
    {
    }

    // Makes the core with what is given: the library's own, or in a test one that fails as a missing library does.
    internal CoreClient(byte[] config, Func<byte[], NavreeCore> make, Action<Update> onUpdate, Action<string>? onProblem = null, Action? onStopped = null)
    {
        ArgumentNullException.ThrowIfNull(config);
        ArgumentNullException.ThrowIfNull(make);
        ArgumentNullException.ThrowIfNull(onUpdate);
        this.onUpdate = onUpdate;
        this.onProblem = onProblem ?? (static _ => { });
        this.onStopped = onStopped ?? (static () => { });
        thread = new Thread(() => Run(config, make), StackBytes) { IsBackground = true, Name = "Navree core" };
        thread.Start();
        started.Task.GetAwaiter().GetResult();
    }

    private readonly Action<Update> onUpdate;
    private readonly Action<string> onProblem;
    private readonly Action onStopped;
    private volatile bool hasStopped;

    /// <summary>The newest view state the core's thread finished. Safe to read from any thread.</summary>
    public ViewState State => published;

    /// <summary>Whether an answer of the core has said it stopped for good. Safe to read from any thread.</summary>
    public bool HasStopped => hasStopped;

    /// <summary>Tells the core one thing that happened, and returns at once.</summary>
    /// <param name="eventJson">The event, as <see cref="Events"/> writes it.</param>
    public void Send(string eventJson)
    {
        ArgumentNullException.ThrowIfNull(eventJson);
        Queue(new SendWork(eventJson, null));
    }

    /// <summary>Tells the core one thing that happened, and completes when it has been handled.</summary>
    /// <param name="eventJson">The event, as <see cref="Events"/> writes it.</param>
    /// <returns>What it came to.</returns>
    public Task<Update> SendAsync(string eventJson) => SendAsync(eventJson, null);

    /// <summary>
    /// Tells the core one thing that happened, and completes when it has been handled. <paramref name="justBefore"/> runs on the core's own
    /// thread at the moment the core is about to be told: after every update of what was told before has been handed on, and before the
    /// update of this event is. So a caller can mark, in the core's own order, the point from which what the core says follows this event.
    /// </summary>
    /// <param name="eventJson">The event, as <see cref="Events"/> writes it.</param>
    /// <param name="justBefore">Runs on the core's thread just before the core is told; not run when the client has ended.</param>
    /// <returns>What it came to.</returns>
    public Task<Update> SendAsync(string eventJson, Action? justBefore)
    {
        ArgumentNullException.ThrowIfNull(eventJson);
        TaskCompletionSource<Update> done = new(TaskCreationOptions.RunContinuationsAsynchronously);
        Queue(new SendWork(eventJson, done, justBefore));
        return done.Task;
    }

    /// <summary>Reads every part again, as at the start and after the interface was made again from nothing.</summary>
    /// <returns>The whole view state.</returns>
    public Task<ViewState> ReadAllAsync()
    {
        TaskCompletionSource<ViewState> done = new(TaskCreationOptions.RunContinuationsAsynchronously);
        Queue(new ReadAllWork(done));
        return done.Task;
    }

    /// <summary>Asks the core for what it keeps for a launch after the system ended the app.</summary>
    /// <returns>JSON text to store as it is, or null when there is nothing to keep.</returns>
    public Task<string?> SnapshotAsync()
    {
        TaskCompletionSource<string?> done = new(TaskCreationOptions.RunContinuationsAsynchronously);
        Queue(new SnapshotWork(done));
        return done.Task;
    }

    /// <summary>
    /// Ends the core after the work given before this call has been handled. What is asked after it, or arrives while the core is ending,
    /// is never left waiting: <see cref="Send"/> is dropped, and <see cref="SendAsync(string)"/>, <see cref="ReadAllAsync"/> and
    /// <see cref="SnapshotAsync"/> complete with <see cref="ObjectDisposedException"/>.
    /// </summary>
    public void Dispose()
    {
        if (queue.Writer.TryWrite(new StopWork()))
        {
            queue.Writer.TryComplete();
            thread.Join();

            // What was given after the stop is answered as ended. No more can be given: the queue is complete.
            while (queue.Reader.TryRead(out Work? left))
            {
                left.Abandon();
            }
        }
    }

    // Gives work to the core's thread; work that cannot be given (the client has ended) is answered as ended at once.
    private void Queue(Work work)
    {
        if (!queue.Writer.TryWrite(work))
        {
            work.Abandon();
        }
    }

    private void Run(byte[] config, Func<byte[], NavreeCore> make)
    {
        NavreeCore engine;
        try
        {
            engine = make(config);
        }
        catch (Exception problem)
        {
            // Whatever ends the start ends it here, on this thread, and is handed to the thread that waits: a library that cannot be loaded
            // is said in words that name the file and where it was looked for.
            started.SetException(CoreLibrary.Explain(problem, CoreLibrary.Folder) is string words ? new CoreUnavailableException(words, problem) : problem);
            return;
        }

        core = engine;
        started.SetResult();
        try
        {
            while (queue.Reader.WaitToReadAsync().AsTask().GetAwaiter().GetResult())
            {
                while (queue.Reader.TryRead(out Work? work))
                {
                    if (work is StopWork)
                    {
                        return;
                    }

                    work.Do(this);
                }
            }
        }
        finally
        {
            engine.Dispose();
        }
    }

    private Update Handle(string eventJson)
    {
        byte[] answer = core!.Send(Encoding.UTF8.GetBytes(eventJson));
        List<Effect> effects = [];
        try
        {
            using JsonDocument document = Reading.Open(answer);
            JsonElement update = document.RootElement;
            if (update.ValueKind == JsonValueKind.Object && update.TryGetProperty("error", out JsonElement error))
            {
                onProblem("the core refused an event: " + (error.ValueKind == JsonValueKind.String ? error.GetString() : error.ToString()));
                NoteStop(update);
                return new Update(state, effects, answer);
            }

            if (update.TryGetProperty("changed", out JsonElement changed) && changed.ValueKind == JsonValueKind.Array)
            {
                foreach (JsonElement change in changed.EnumerateArray())
                {
                    Refresh(change);
                }
            }

            if (update.TryGetProperty("effects", out JsonElement list) && list.ValueKind == JsonValueKind.Array)
            {
                foreach (JsonElement effect in list.EnumerateArray())
                {
                    effects.Add(EffectReading.Read(effect));
                }
            }
        }
        catch (JsonException problem)
        {
            onProblem("the core answered what does not read: " + problem.Message);
        }

        published = state;
        return new Update(state, effects, answer);
    }

    private void CarryOut(Update update)
    {
        try
        {
            onUpdate(update);
        }
        catch (Exception problem)
        {
            onProblem("an update was not handled: " + problem.GetType().Name);
        }
    }

    private void Refresh(JsonElement change)
    {
        switch (Reading.Tag(change, "t"))
        {
            case "app":
                // Counted: this is the core saying who the app is for now, which reading every part again is not (ViewState.AppGiven).
                state = state with { App = Query<AppPart>(Part("app"), ViewReading.ReadApp), AppGiven = state.AppGiven + 1 };
                break;
            case "nav":
                if (Query<NavPart>(Part("nav"), ViewReading.ReadNav) is NavPart nav)
                {
                    IReadOnlySet<long> held = nav.Held();
                    state = state with { Nav = nav, Screens = state.Screens.RemoveRange(state.Screens.Keys.Where(id => !held.Contains(id)).ToList()) };
                }

                break;
            case "screen":
                if (Reading.OptV<long>(change, "id", Reading.Long) is long id)
                {
                    ReadScreen(id);
                }

                break;
            case "form":
                ReadForm(change);
                break;
            case "rows":
            case "redraw":
                // New rows, or rows named again: the whole screen read again is always right.
                if (Reading.OptV<long>(change, "screen", Reading.Long) is long screen)
                {
                    ReadScreen(screen);
                }

                break;
            case "messages":
                state = state with { Messages = Query<ImmutableList<Message>>(Part("messages"), ViewReading.ReadMessages) ?? ImmutableList<Message>.Empty };
                break;
            default:
                ReadAll();
                break;
        }
    }

    private void ReadForm(JsonElement change)
    {
        if (Reading.OptV<long>(change, "screen", Reading.Long) is not long screen
            || Reading.Opt<string>(change, "form", Reading.Text) is not string form)
        {
            return;
        }

        FormView? view = Query<FormView>(
            "{\"part\":\"form\",\"screen\":" + screen.ToString(CultureInfo.InvariantCulture) + ",\"form\":" + JsonSerializer.Serialize(form) + "}",
            ViewReading.ReadForm);
        if (!state.Screens.TryGetValue(screen, out ScreenView? current))
        {
            ReadScreen(screen);
            return;
        }

        ImmutableDictionary<string, FormView> forms = current.Forms.ToImmutableDictionary();
        forms = view is null ? forms.Remove(form) : forms.SetItem(form, view);
        state = state with { Screens = state.Screens.SetItem(screen, current with { Forms = forms }) };
    }

    private void ReadScreen(long id)
    {
        ScreenView? view = Query<ScreenView>(
            "{\"part\":\"screen\",\"id\":" + id.ToString(CultureInfo.InvariantCulture) + "}",
            ViewReading.ReadScreen);
        state = state with { Screens = view is null ? state.Screens.Remove(id) : state.Screens.SetItem(id, view) };
    }

    private void ReadAll()
    {
        byte[] bytes = core!.View();
        try
        {
            using JsonDocument document = Reading.Open(bytes);
            JsonElement all = document.RootElement;
            if (all.ValueKind != JsonValueKind.Object)
            {
                return;
            }

            // An answer that is an error is not a view state: the state stays as it was.
            if (all.TryGetProperty("error", out _))
            {
                onProblem("the core refused the whole view");
                NoteStop(all);
                return;
            }

            AppPart? app = Reading.TryField(all, "app", out JsonElement appPart) ? ViewReading.ReadApp(appPart) : null;
            NavPart nav = ViewReading.ReadNav(all);
            ImmutableDictionary<long, ScreenView>.Builder screens = ImmutableDictionary.CreateBuilder<long, ScreenView>();
            if (Reading.TryField(all, "screens", out JsonElement held) && held.ValueKind == JsonValueKind.Object)
            {
                foreach (JsonProperty screen in held.EnumerateObject())
                {
                    ScreenView view = ViewReading.ReadScreen(screen.Value);
                    screens[view.Id] = view;
                }
            }

            ImmutableList<Message> messages = Reading.TryField(all, "messages", out JsonElement list)
                ? ViewReading.ReadMessages(list)
                : ImmutableList<Message>.Empty;
            // Every part read again is the same app's part read again, not one the core gave anew: the count stays.
            state = new ViewState(app, nav, screens.ToImmutable(), messages) { AppGiven = state.AppGiven };
        }
        catch (JsonException problem)
        {
            onProblem("the view state did not decode: " + problem.Message);
        }
    }

    private static string Part(string name) => "{\"part\":\"" + name + "\"}";

    // Said once: the core has stopped for good.
    private void NoteStop(JsonElement answer)
    {
        if (hasStopped || !CoreAnswers.IsStop(answer))
        {
            return;
        }

        hasStopped = true;
        try
        {
            onStopped();
        }
        catch (Exception problem)
        {
            onProblem("the stop was not handled: " + problem.GetType().Name);
        }
    }

    // Reads one part: null when the core has none, or when it does not read.
    private T? Query<T>(string query, Func<JsonElement, T> read)
        where T : class
    {
        byte[] bytes = core!.View(Encoding.UTF8.GetBytes(query));
        try
        {
            using JsonDocument document = Reading.Open(bytes);
            JsonElement answer = document.RootElement;
            if (answer.ValueKind == JsonValueKind.Null)
            {
                return null;
            }

            // An error, with or without the mark of a stopped core beside it, is never read as a part.
            if (answer.ValueKind == JsonValueKind.Object
                && answer.TryGetProperty("error", out JsonElement error)
                && answer.EnumerateObject().All(property => property.NameEquals("error") || property.NameEquals("stopped")))
            {
                onProblem("the core refused a view: " + (error.ValueKind == JsonValueKind.String ? error.GetString() : error.ToString()));
                NoteStop(answer);
                return null;
            }

            return read(answer);
        }
        catch (JsonException problem)
        {
            onProblem("a part of the view state did not decode: " + problem.Message);
            return null;
        }
    }

    private abstract class Work
    {
        internal abstract void Do(CoreClient client);

        // The client ended before this was handled.
        internal abstract void Abandon();
    }

    private sealed class StopWork : Work
    {
        internal override void Do(CoreClient client)
        {
        }

        internal override void Abandon()
        {
        }
    }

    private sealed class SendWork(string eventJson, TaskCompletionSource<Update>? done, Action? justBefore = null) : Work
    {
        internal override void Abandon() => done?.TrySetException(new ObjectDisposedException(nameof(CoreClient)));

        internal override void Do(CoreClient client)
        {
            try
            {
                justBefore?.Invoke();
                Update update = client.Handle(eventJson);
                client.CarryOut(update);
                done?.SetResult(update);
            }
            catch (Exception problem)
            {
                client.onProblem("the core failed: " + problem.Message);
                done?.SetException(problem);
            }
        }
    }

    private sealed class ReadAllWork(TaskCompletionSource<ViewState> done) : Work
    {
        internal override void Abandon() => done.TrySetException(new ObjectDisposedException(nameof(CoreClient)));

        internal override void Do(CoreClient client)
        {
            try
            {
                client.ReadAll();
                client.published = client.state;
                done.SetResult(client.state);
            }
            catch (Exception problem)
            {
                done.SetException(problem);
            }
        }
    }

    private sealed class SnapshotWork(TaskCompletionSource<string?> done) : Work
    {
        internal override void Abandon() => done.TrySetException(new ObjectDisposedException(nameof(CoreClient)));

        internal override void Do(CoreClient client)
        {
            try
            {
                byte[] bytes = client.core!.View("{\"part\":\"snapshot\"}"u8);
                string text = Encoding.UTF8.GetString(bytes);
                using JsonDocument document = Reading.Open(bytes);
                if (document.RootElement.ValueKind == JsonValueKind.Object && document.RootElement.TryGetProperty("error", out _))
                {
                    // An error is not a snapshot: nothing is kept.
                    client.NoteStop(document.RootElement);
                    done.SetResult(null);
                    return;
                }

                done.SetResult(text == "null" ? null : text);
            }
            catch (Exception problem)
            {
                done.SetException(problem);
            }
        }
    }
}
