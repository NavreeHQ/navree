namespace Navree.Platform;

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
/// </remarks>
public sealed class SessionKeeper : IDisposable
{
    /// <summary>The key the cookies are kept under.</summary>
    public const string CookiesKey = "cookies";

    private readonly KeptStore kept;
    private readonly CookieStore cookies;
    private readonly SerialQueue disk = new();
    private readonly object gate = new();
    private string lastCookies = string.Empty;
    private bool ending;
    private int wiping;

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

        // What an earlier start had to set aside is deleted now, if whatever held it has let go.
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
    /// <paramref name="afterKept"/>. The mark goes when every one of them finished and the core let go.
    /// </summary>
    /// <param name="stopWork">Stops the requests, streams, timers and snapshots under way. If it throws, the sign-out stops there, nothing is
    /// written again in this run, and the mark makes the next start finish it.</param>
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
        }

        try
        {
            kept.MarkWipe();
        }
        catch (Exception problem) when (problem is IOException or UnauthorizedAccessException)
        {
            failed.Add(new KeyValuePair<string, string>("the mark that a sign-out is under way", problem.GetType().Name));
        }

        cookies.Clear();
        try
        {
            stopWork();
        }
        catch (Exception)
        {
            lock (gate)
            {
                wiping--;
            }

            throw;
        }

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
        if (report.Complete)
        {
            try
            {
                await disk.RunAsync(kept.UnmarkWipe).ConfigureAwait(false);
            }
            catch (Exception problem) when (problem is IOException or UnauthorizedAccessException)
            {
                // The mark stays: the next start takes away again what is already gone.
            }
        }

        return report;
    }

    /// <summary>Waits until everything given to the queue so far has run.</summary>
    /// <returns>When it has.</returns>
    public Task DrainAsync() => disk.RunAsync(static () => { });

    /// <inheritdoc />
    public void Dispose() => disk.Dispose();

    // Whether a write stamped with a session is still to be made: that session is the store's now, and no sign-out is under way.
    private bool Current(long stamp)
    {
        lock (gate)
        {
            return !ending && stamp == cookies.Session;
        }
    }
}
