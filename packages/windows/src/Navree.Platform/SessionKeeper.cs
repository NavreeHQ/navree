namespace Navree.Platform;

/// <summary>
/// A change of person that has begun (<see cref="SessionKeeper.BeginPersonChange"/>) and is to be finished
/// (<see cref="SessionKeeper.FinishPersonChangeAsync"/>).
/// </summary>
/// <param name="Number">Which change it is: only the newest writes its mark.</param>
/// <param name="Stamp">The session it was seen in; null when a sign-out was under way.</param>
/// <param name="KeptGone">Completes when what was kept for the person before has been deleted, in its turn in the queue.</param>
public readonly record struct PersonChangeBegun(long Number, long? Stamp, Task KeptGone);

/// <summary>
/// What is written on the device for the person signed in, and the end of their session.
/// </summary>
/// <remarks>
/// <para>
/// Every write on the person's behalf (the cookies, what the core asks to keep, the snapshot, a table's layout, the notifications that wait)
/// goes through one queue, in the order it was asked for, and is stamped with the session it was asked in (<see cref="Stamp"/>, the cookie
/// store's <see cref="CookieStore.Session"/>). A write whose session has ended by the time the queue reaches it is dropped. From the moment a
/// sign-out begins until the core says it has let go of the person, nothing new is written at all: what the core or a timer would write then
/// is still the person's who is leaving.
/// </para>
/// <para>
/// The end of a session survives the app ending in the middle of it. Before anything else a mark is left on disk; the cookies go from memory
/// at once, so a request made after that carries none and an answer to one made before it keeps none; then the cookie file goes, then what the
/// web view keeps, then (once the core has let go) everything else that is kept. The mark goes only when every step has finished and the core
/// has let go. A start that finds the mark finishes the wipe before it reads anything (<see cref="Open"/>).
/// </para>
/// <para>
/// A person can also change with no sign-out: a session runs out, or a server ends it and says nothing of it to the client, and somebody
/// else signs in. The cookies are theirs by then, and the core has its cache emptied. Everything else kept for the person before goes
/// then (<see cref="PersonChangedAsync"/>), and beside what is kept stands a mark of whom it is kept for (<see cref="PersonKey"/>), so a
/// start sees such a change when it happened across a restart, or was not finished.
/// </para>
/// </remarks>
public sealed class SessionKeeper : IDisposable
{
    /// <summary>The key the cookies are kept under.</summary>
    public const string CookiesKey = "cookies";

    /// <summary>The key the mark of the person is kept under, whom the rest is kept for (<see cref="PersonWatch.Named"/>).</summary>
    public const string PersonKey = "person";

    private readonly KeptStore kept;
    private readonly CookieStore cookies;
    private readonly SerialQueue disk = new();
    private readonly object gate = new();
    private string lastCookies = string.Empty;
    private bool ending;
    private int wiping;
    private int ends;
    private bool endFailed;
    private string lastPerson = string.Empty;
    private long personChanges;
    private bool personOwed;

    /// <summary>Makes the keeper. Nothing is read until <see cref="Open"/>.</summary>
    /// <param name="kept">What is kept between launches.</param>
    /// <param name="cookies">The cookies of the app's origins.</param>
    public SessionKeeper(KeptStore kept, CookieStore cookies)
    {
        ArgumentNullException.ThrowIfNull(kept);
        ArgumentNullException.ThrowIfNull(cookies);
        this.kept = kept;
        this.cookies = cookies;
    }

    /// <summary>Whether a sign-out has begun and the core has not yet let go of the person: nothing is written meanwhile.</summary>
    public bool Ending
    {
        get
        {
            lock (gate)
            {
                return ending;
            }
        }
    }

    /// <summary>
    /// Whether a sign-out is taking away what could rebuild the session: from its start until the cookie file and what the web view keeps are
    /// gone (the steps before the core is told). Nothing may reach the network for the web view meanwhile; after it, a page of whoever comes
    /// next loads into an emptied profile.
    /// </summary>
    public bool Wiping
    {
        get
        {
            lock (gate)
            {
                return wiping > 0;
            }
        }
    }

    /// <summary>
    /// Opens what the last run kept, at the start, before anything else is read. When a sign-out was left unfinished (its mark is there), it is
    /// finished first: the cookie file and everything kept go, nothing is taken from them, and <paramref name="finishRest"/> takes away what is
    /// kept elsewhere (the web view's folder, the copies of chosen files). A file that cannot be deleted (another program holds it) does not
    /// stop the start: it is set aside, never read again, and deleted when it can be (<see cref="SetAsideAtStart"/> says how many). The mark
    /// goes only when everything is gone or set aside; while it is there no start takes anything from what was kept. Otherwise the cookies
    /// kept are taken into the store.
    /// </summary>
    /// <param name="finishRest">Takes away the rest of the person's data; false when some of it could not be.</param>
    /// <returns>Whether a sign-out was left unfinished: this start began signed out, with nothing taken from what was kept.</returns>
    public bool Open(Func<bool>? finishRest = null)
    {
        if (kept.WipeMarked)
        {
            // Nothing is imported in a start that finds the mark, whatever can or cannot be deleted.
            cookies.Clear();
            bool gone = false;
            try
            {
                SetAsideAtStart = kept.ClearOrSetAside();
                gone = true;
            }
            catch (Exception problem) when (problem is IOException or UnauthorizedAccessException)
            {
                // What is still there could not even be named: the mark stays, and the next start begins the same way.
            }

            bool rest = finishRest?.Invoke() ?? true;
            if (gone && rest)
            {
                try
                {
                    kept.UnmarkWipe();
                }
                catch (Exception problem) when (problem is IOException or UnauthorizedAccessException)
                {
                    // The mark stays: the next start takes away again what is already gone.
                }
            }

            return true;
        }

        // What an earlier start had to set aside is deleted now, if whatever held it has let go. When the list that names it is there and
        // cannot be trusted, everything kept is set aside and deleted the same way: the start is clean, and nothing is taken from it below.
        SetAsideAtStart = kept.DeleteSetAside();
        if (kept.Read(CookiesKey) is string saved)
        {
            cookies.Import(saved);
            lock (gate)
            {
                lastCookies = cookies.Export();
            }
        }

        return false;
    }

    /// <summary>
    /// How many files of a session that ended are still on disk after <see cref="Open"/>, because another program holds them: they are set
    /// aside, never read, and deleted at a later start.
    /// </summary>
    public int SetAsideAtStart { get; private set; }

    /// <summary>The session a write is asked in now, to be given to <see cref="Write(long?, string, string?)"/>; null while a sign-out is under way.</summary>
    /// <returns>The stamp, or null.</returns>
    public long? Stamp()
    {
        lock (gate)
        {
            return ending ? null : cookies.Session;
        }
    }

    /// <summary>Keeps a value under a key, asked now, in order with the rest; a value of none takes the key away.</summary>
    /// <param name="key">The key.</param>
    /// <param name="value">The value.</param>
    public void Write(string key, string? value) => Write(Stamp(), key, value);

    /// <summary>
    /// Keeps a value under a key in order with the rest, when the session it was asked in is still the one now; a value of none takes the key
    /// away whenever it was asked.
    /// </summary>
    /// <param name="stamp">What <see cref="Stamp"/> said when the value was made.</param>
    /// <param name="key">The key.</param>
    /// <param name="value">The value.</param>
    public void Write(long? stamp, string key, string? value)
    {
        if (value is null)
        {
            disk.Run(() => kept.Write(key, null));
            return;
        }

        if (stamp is null)
        {
            return;
        }

        disk.Run(() =>
        {
            if (Current(stamp.Value))
            {
                kept.Write(key, value);
            }
        });
    }

    /// <summary>
    /// Keeps a value in the core's cache under a key, when the session it was asked in is still the one now; a value of none takes the key
    /// away whenever it was asked. It is in order with the rest.
    /// </summary>
    /// <param name="key">The key the core named.</param>
    /// <param name="value">The value.</param>
    public void WriteCached(string key, string? value)
    {
        if (value is null)
        {
            disk.Run(() => kept.WriteCached(key, null));
            return;
        }

        if (Stamp() is not long stamp)
        {
            return;
        }

        disk.Run(() =>
        {
            if (Current(stamp))
            {
                kept.WriteCached(key, value);
            }
        });
    }

    /// <summary>Reads what the core's cache holds under a key, in order with the writes, and hands it on from the queue's thread.</summary>
    /// <param name="key">The key the core named.</param>
    /// <param name="then">Given the value, or null.</param>
    public void ReadCached(string key, Action<string?> then)
    {
        ArgumentNullException.ThrowIfNull(then);
        disk.Run(() => then(kept.ReadCached(key)));
    }

    /// <summary>
    /// Empties the core's cache, in order with the writes (the core asks for it after every write the server may have run). What the client
    /// keeps for the person is not the cache and stays: the cookies, the snapshot, a table's widths, the notifications that wait.
    /// </summary>
    public void ClearCache() => disk.Run(kept.ClearCache);

    /// <summary>Keeps the cookies the store holds when they changed, for the next launch; nothing while a sign-out is under way.</summary>
    public void KeepCookies()
    {
        string now;
        long session;
        lock (gate)
        {
            if (ending)
            {
                return;
            }

            now = cookies.Export(out session);
            if (now == lastCookies)
            {
                return;
            }

            lastCookies = now;
        }

        // A store with no cookie leaves no file.
        string? value = now == "[]" ? null : now;
        disk.Run(() =>
        {
            if (Current(session))
            {
                kept.Write(CookiesKey, value);
            }
        });
    }

    /// <summary>
    /// Ends the session. What runs before the first await, on the caller's thread: the mark is left on disk and written through, nothing more
    /// is written, the cookies go from memory (a request made after this carries none, and the answer to one made before keeps none), and
    /// <paramref name="stopWork"/> stops what is running for the person. Then, in order, each with the limit of <see cref="Wipe"/>: the cookie
    /// file and <paramref name="beforeKept"/> (what the web view keeps); the core is told and its answer awaited (<paramref name="tellCore"/>;
    /// none when it is the core that ended the session), after which writes are made again for whoever comes next; everything else kept; and
    /// <paramref name="afterKept"/>. The mark goes when every one of them finished and the core let go, and no other sign-out is still
    /// running: when two overlap, it goes with the last of them, and only if both finished.
    /// </summary>
    /// <param name="stopWork">Stops the requests, streams, timers and snapshots under way. If it throws, that is written down in what is
    /// returned and the sign-out goes on to its end: its steps run and the core is told, so nothing of the person is left for whoever comes
    /// next and the app can be used again; the mark stays, and the next start finishes the sign-out once more.</param>
    /// <param name="tellCore">Tells the core the client ended the session and completes when the core has handled it; null when the core ended it.</param>
    /// <param name="beforeKept">Steps that run after the cookie file goes and before the rest of what is kept.</param>
    /// <param name="afterKept">Steps that run after.</param>
    /// <param name="limit">How long one step, and the core's answer, may take; <see cref="Wipe.StepLimit"/> when left out.</param>
    /// <returns>What came of each step.</returns>
    public async Task<WipeReport> EndAsync(Action stopWork, Func<Task>? tellCore, IEnumerable<WipeStep> beforeKept, IEnumerable<WipeStep> afterKept, TimeSpan? limit = null)
    {
        ArgumentNullException.ThrowIfNull(stopWork);
        ArgumentNullException.ThrowIfNull(beforeKept);
        ArgumentNullException.ThrowIfNull(afterKept);
        TimeSpan each = limit ?? Wipe.StepLimit;
        List<KeyValuePair<string, string>> failed = [];
        lock (gate)
        {
            ending = true;
            wiping++;
            lastCookies = string.Empty;
            ForgetPerson();
            if (ends++ == 0)
            {
                endFailed = false;
            }
        }

        // The start. A sign-out that has begun is carried to its end whatever a part of its start throws: the part is written down, and the
        // steps still run and the core is still told. Stopping here would leave everything of the person on disk for the rest of the run,
        // where the core of whoever comes next reads its pages from, and would leave the app unable to keep or ask for anything again.
        Begin("the mark that a sign-out is under way", kept.MarkWipe, failed);
        Begin("the cookies in memory", cookies.Clear, failed);
        Begin("the work under way", stopWork, failed);

        WipeReport first;
        try
        {
            // What could rebuild the session goes first: the cookie file, then what the web view keeps (while its pages are still there to clear it).
            first = await Wipe.RunAsync(
                [
                    new WipeStep("the cookies", () => disk.RunAsync(() => kept.Write(CookiesKey, null))),
                    .. beforeKept,
                ],
                each).ConfigureAwait(false);
        }
        finally
        {
            // What could rebuild the session is gone (or its step is reported): a page of whoever comes next may load.
            lock (gate)
            {
                wiping--;
            }
        }

        bool letGo = tellCore is null;
        if (tellCore is not null)
        {
            try
            {
                Task telling = tellCore();
                if (await Task.WhenAny(telling, Task.Delay(each)).ConfigureAwait(false) == telling)
                {
                    await telling.ConfigureAwait(false);
                    letGo = true;
                }
                else
                {
                    failed.Add(new KeyValuePair<string, string>("the core's answer", "it took longer than " + each.TotalSeconds + " seconds"));
                }
            }
            catch (Exception problem)
            {
                failed.Add(new KeyValuePair<string, string>("the core's answer", problem.GetType().Name));
            }
        }

        // Until the core has let go of the person, what it or a timer asks to keep is still theirs; a core that did not answer keeps it so.
        if (letGo)
        {
            lock (gate)
            {
                ending = false;
            }
        }

        WipeReport rest = await Wipe.RunAsync(
            [
                new WipeStep("what is kept on the device", () => disk.RunAsync(kept.Clear)),
                .. afterKept,
            ],
            each).ConfigureAwait(false);

        WipeReport report = new([.. first.Done, .. rest.Done], [.. failed, .. first.Failed, .. rest.Failed]);
        await FinishAsync(report).ConfigureAwait(false);
        return report;
    }

    /// <summary>
    /// Ends the session when the core has said it is ending and has not let go of the person yet: the core waits for one last answer of the
    /// server first, and an app that is ended while it waits must already have begun. What runs before the first await is what
    /// <see cref="EndAsync"/> runs there: the mark is left on disk, the cookies go from memory, and <paramref name="stopWork"/> stops what is
    /// running. Then every step runs at once, in the order of <see cref="EndAsync"/> and without waiting for the core: the cookie file,
    /// <paramref name="beforeKept"/>, everything else kept, <paramref name="afterKept"/>. Nothing is written for anybody until the core has
    /// let go (<paramref name="coreLetsGo"/>): until then what it or a timer asks to keep is still the leaving person's. Nothing runs a
    /// second time when it lets go: the core closed the person's screens before it said the session is ending, so nothing of theirs is
    /// gained in between. The mark goes when every step finished and the core let go in time.
    /// </summary>
    /// <param name="stopWork">As for <see cref="EndAsync"/>. It leaves the leaving person's last request running (<see cref="SessionGate.IsLast"/>).</param>
    /// <param name="coreLetsGo">Completes when the core has let go of the person.</param>
    /// <param name="beforeKept">Steps that run after the cookie file goes and before the rest of what is kept.</param>
    /// <param name="afterKept">Steps that run after.</param>
    /// <param name="limit">How long one step may take, and how long the core may take to let go once the steps have run; <see cref="Wipe.StepLimit"/> when left out.</param>
    /// <returns>What came of each step.</returns>
    public async Task<WipeReport> EndBeforeCoreAsync(
        Action stopWork,
        Task coreLetsGo,
        IEnumerable<WipeStep> beforeKept,
        IEnumerable<WipeStep> afterKept,
        TimeSpan? limit = null)
    {
        ArgumentNullException.ThrowIfNull(stopWork);
        ArgumentNullException.ThrowIfNull(coreLetsGo);
        ArgumentNullException.ThrowIfNull(beforeKept);
        ArgumentNullException.ThrowIfNull(afterKept);
        TimeSpan each = limit ?? Wipe.StepLimit;
        List<KeyValuePair<string, string>> failed = [];
        lock (gate)
        {
            ending = true;
            wiping++;
            lastCookies = string.Empty;
            ForgetPerson();
            if (ends++ == 0)
            {
                endFailed = false;
            }
        }

        // The start, as in EndAsync: a part that throws is written down and the sign-out goes on.
        Begin("the mark that a sign-out is under way", kept.MarkWipe, failed);
        Begin("the cookies in memory", cookies.Clear, failed);
        Begin("the work under way", stopWork, failed);

        WipeReport first;
        try
        {
            first = await Wipe.RunAsync(
                [
                    new WipeStep("the cookies", () => disk.RunAsync(() => kept.Write(CookiesKey, null))),
                    .. beforeKept,
                ],
                each).ConfigureAwait(false);
        }
        finally
        {
            lock (gate)
            {
                wiping--;
            }
        }

        // Everything else goes now too, and not when the core lets go: nothing is written while it has not, so nothing of the person comes back.
        WipeReport rest = await Wipe.RunAsync(
            [
                new WipeStep("what is kept on the device", () => disk.RunAsync(kept.Clear)),
                .. afterKept,
            ],
            each).ConfigureAwait(false);

        bool letGo = false;
        try
        {
            if (await Task.WhenAny(coreLetsGo, Task.Delay(each)).ConfigureAwait(false) == coreLetsGo)
            {
                await coreLetsGo.ConfigureAwait(false);
                letGo = true;
            }
            else
            {
                failed.Add(new KeyValuePair<string, string>("the core letting go", "it took longer than " + each.TotalSeconds + " seconds"));
            }
        }
        catch (Exception problem)
        {
            failed.Add(new KeyValuePair<string, string>("the core letting go", problem.GetType().Name));
        }

        // A core that did not let go keeps it so: what it asks to keep is still the leaving person's, and nothing is written.
        if (letGo)
        {
            lock (gate)
            {
                ending = false;
            }
        }

        WipeReport report = new([.. first.Done, .. rest.Done], [.. failed, .. first.Failed, .. rest.Failed]);
        await FinishAsync(report).ConfigureAwait(false);
        return report;
    }

    /// <summary>
    /// The mark of the person what is kept was kept for, as the last run left it; null when none is kept. Read at the start, after
    /// <see cref="Open"/>: a start that finished a sign-out finds none.
    /// </summary>
    /// <returns>The mark, or null.</returns>
    public string? KeptPerson()
    {
        string? mark = kept.Read(PersonKey);
        lock (gate)
        {
            lastPerson = mark ?? string.Empty;
        }

        return mark;
    }

    /// <summary>
    /// Keeps the mark of the person who is signed in, when it is not the one kept already: what is kept from now on is kept for them.
    /// Asked each time the core names them, so a mark that could not be written (a sign-out was still under way) is written the next
    /// time. Nothing is written while a sign-out is under way, nor while a change of person has not finished
    /// (<see cref="PersonChangedAsync"/>): the mark kept is then still that of the person before, and stays so.
    /// </summary>
    /// <param name="mark">The person's mark (<see cref="PersonWatch.Named"/>).</param>
    public void KeepPerson(string mark)
    {
        ArgumentException.ThrowIfNullOrEmpty(mark);
        lock (gate)
        {
            if (personOwed)
            {
                return;
            }
        }

        WritePerson(Stamp(), mark);
    }

    /// <summary>
    /// Keeps the mark of the person who is signed in, as <see cref="KeepPerson"/> does, and says, once the mark has had its turn in the
    /// queue, whether it is the mark kept on the device: read back from where it is kept, after every write asked for before. What is to
    /// be kept under a mark only when that mark is the one kept waits for this.
    /// </summary>
    /// <param name="mark">The person's mark (<see cref="PersonWatch.Named"/>).</param>
    /// <returns>Whether the mark kept is this one. False when it could not be written, was not to be (a sign-out is under way, or a
    /// change of person has not finished), or the queue has ended.</returns>
    public async Task<bool> KeepPersonAsync(string mark)
    {
        KeepPerson(mark);
        bool isKept = false;
        try
        {
            await disk.RunAsync(() => isKept = string.Equals(kept.Read(PersonKey), mark, StringComparison.Ordinal)).ConfigureAwait(false);
        }
        catch (ObjectDisposedException)
        {
            return false;
        }

        return isKept;
    }

    /// <summary>
    /// Whether anything is kept for a person besides the cookies and the mark of whom it is kept for: a snapshot, the widths of a table's
    /// columns, notifications that wait. A start that reads no mark asks this: when something is kept and nothing says whose, the first
    /// person named is not taken for its owner (<see cref="PersonWatch"/>).
    /// </summary>
    /// <returns>Whether something is kept.</returns>
    public bool KeepsOfAPerson() => kept.HoldsOwn(CookiesKey, PersonKey);

    // Writes the mark of the person in its turn, when the session it was asked in is still the one now and it is not the mark last written.
    // A write that fails is not counted as made: the mark is asked to be kept each time the core names the person, so it is tried again
    // with the next state, until it is on disk.
    private void WritePerson(long? stamp, string mark)
    {
        long asked;
        lock (gate)
        {
            if (stamp is not long given || ending || given != cookies.Session || mark == lastPerson)
            {
                return;
            }

            asked = given;
            lastPerson = mark;
        }

        disk.Run(() =>
        {
            try
            {
                if (Current(asked))
                {
                    kept.Write(PersonKey, mark);
                }
            }
            catch (Exception)
            {
                lock (gate)
                {
                    if (lastPerson == mark)
                    {
                        lastPerson = string.Empty;
                    }
                }
            }
        });
    }

    /// <summary>
    /// Another person is signed in than the one what is kept was kept for, and nobody signed out between. Everything the client keeps for
    /// the person before goes: the snapshot, the widths they gave a table's columns, the notifications that wait, and whatever
    /// <paramref name="steps"/> take away (what the web view keeps, the files chosen for forms, what the system shows). The cookies stay:
    /// they are already those of the person who is here now. The core's cache is not touched here; the core has it emptied itself.
    /// </summary>
    /// <remarks>
    /// <para>
    /// What is kept is deleted in its turn in the queue, and that turn is taken before this returns: whatever is asked to be kept after
    /// the call is kept for the person who is here now, and stays.
    /// </para>
    /// <para>
    /// The mark of the person before is left where it is until every step has finished, and only then is the mark of the person who is
    /// here now written in its place. An app that is ended in the middle, or a step that fails, leaves the old mark: the next start hears
    /// another person named than the one it remembers, and does all of this over.
    /// </para>
    /// </remarks>
    /// <param name="mark">The mark of the person who is signed in now (<see cref="PersonWatch.Named"/>).</param>
    /// <param name="steps">What else takes away something of the person before, each with the limit of <see cref="Wipe"/>.</param>
    /// <param name="limit">How long one step may take; <see cref="Wipe.StepLimit"/> when left out.</param>
    /// <returns>What came of each step.</returns>
    public Task<WipeReport> PersonChangedAsync(string mark, IEnumerable<WipeStep> steps, TimeSpan? limit = null) =>
        FinishPersonChangeAsync(BeginPersonChange(), mark, steps, limit);

    /// <summary>
    /// Begins a change of person, before anything else of it is done. From this call on the mark kept stays that of the person before, and
    /// no other is written, until <see cref="FinishPersonChangeAsync"/> finds that every step finished: whatever happens between, the next
    /// start does the change over. What is kept for the person before is given its turn in the queue here, ahead of anything that is asked
    /// to be kept after the call.
    /// </summary>
    /// <returns>The change, to be given to <see cref="FinishPersonChangeAsync"/>.</returns>
    public PersonChangeBegun BeginPersonChange()
    {
        // The session the change was seen in: when it has ended by the time the steps are done, a sign-out took everything and no mark is written.
        long? stamp = Stamp();
        long change;
        lock (gate)
        {
            change = ++personChanges;
            personOwed = true;
        }

        return new PersonChangeBegun(change, stamp, disk.RunAsync(() => kept.ClearOwn(CookiesKey, PersonKey)));
    }

    /// <summary>
    /// Finishes a change of person: waits for what was kept to be gone, runs <paramref name="steps"/>, each with the limit of
    /// <see cref="Wipe"/>, and when every one finished, and no newer change or sign-out began meanwhile, writes the mark of the person who
    /// is here now in place of the mark of the person before.
    /// </summary>
    /// <param name="begun">What <see cref="BeginPersonChange"/> gave.</param>
    /// <param name="mark">The mark of the person who is signed in now.</param>
    /// <param name="steps">What else takes away something of the person before.</param>
    /// <param name="limit">How long one step may take; <see cref="Wipe.StepLimit"/> when left out.</param>
    /// <returns>What came of each step.</returns>
    public async Task<WipeReport> FinishPersonChangeAsync(PersonChangeBegun begun, string mark, IEnumerable<WipeStep> steps, TimeSpan? limit = null)
    {
        ArgumentException.ThrowIfNullOrEmpty(mark);
        ArgumentNullException.ThrowIfNull(steps);
        WipeReport report = await Wipe.RunAsync(
            [
                new WipeStep("what was kept for the person before", () => begun.KeptGone),
                .. steps,
            ],
            limit ?? Wipe.StepLimit).ConfigureAwait(false);

        // When yet another person signed in while the steps ran, the mark is theirs to write, when their own steps have finished. A change
        // that did not finish leaves the mark of the person before for the rest of this run.
        lock (gate)
        {
            if (!report.Complete || begun.Number != personChanges)
            {
                return report;
            }

            personOwed = false;
        }

        WritePerson(begun.Stamp, mark);
        return report;
    }

    /// <summary>Waits until everything given to the queue so far has run.</summary>
    /// <returns>When it has.</returns>
    public Task DrainAsync() => disk.RunAsync(static () => { });

    /// <inheritdoc />
    public void Dispose() => disk.Dispose();

    // The end of one sign-out. Two can overlap (the person signs out while the server ends the session), and the mark is theirs together:
    // it goes when none is still running and every one that ran with this one finished.
    private async Task FinishAsync(WipeReport report)
    {
        bool unmark;
        lock (gate)
        {
            endFailed |= !report.Complete;
            unmark = --ends == 0 && !endFailed;
        }

        if (!unmark)
        {
            return;
        }

        try
        {
            await disk.RunAsync(kept.UnmarkWipe).ConfigureAwait(false);
        }
        catch (Exception problem) when (problem is IOException or UnauthorizedAccessException)
        {
            // The mark stays: the next start takes away again what is already gone.
        }
    }

    // With the gate held, as a sign-out begins: the sign-out takes the mark of the person with everything else, so none is written and none
    // is owed, and a change of person still under way has nothing left to write.
    private void ForgetPerson()
    {
        lastPerson = string.Empty;
        personOwed = false;
        personChanges++;
    }

    // One part of the start of a sign-out: what it throws is written down, and the next part runs.
    private static void Begin(string name, Action part, List<KeyValuePair<string, string>> failed)
    {
        try
        {
            part();
        }
        catch (Exception problem)
        {
            failed.Add(new KeyValuePair<string, string>(name, problem.GetType().Name));
        }
    }

    // Whether a write stamped with a session is still to be made: that session is the store's now, and no sign-out is under way.
    private bool Current(long stamp)
    {
        lock (gate)
        {
            return !ending && stamp == cookies.Session;
        }
    }
}
