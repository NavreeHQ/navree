using Navree.Protocol;
using Navree.Runtime;

namespace Navree.Platform;

/// <summary>
/// What the shell itself does at a change of person, each part the host's own: this is how the rule that orders them
/// (<see cref="PersonChange"/>) is held to without a window.
/// </summary>
/// <param name="MoveWebOn">Moves the web view's folder on, here and now, and gives the folder that ended.</param>
/// <param name="DropPictures">Lets go of the pictures held in memory, here and now.</param>
/// <param name="ClearFiles">Begins letting go of the files chosen for forms; completes when their copies are gone.</param>
/// <param name="TakeFromSystem">
/// Takes away what the system holds or shows for the person. What it does before its first wait is done at the call: when it returns nothing
/// waits to be notified and no count is shown. Completes when the system has taken the rest away.
/// </param>
/// <param name="EmptyWeb">Closes the web views that were started in a folder that ended, and clears or deletes what they kept.</param>
/// <param name="OnWindow">Runs a step on the window's thread and completes when it has run there.</param>
/// <param name="More">What else the shell keeps of a person, as steps: asked for when a change begins.</param>
/// <param name="GiveWaitingTo">
/// No part of a change: gives what waits and is nobody's (a reminder asked for while nobody had been named) to the person whose mark it
/// is given, here and now. It is called when the first person since a sign-out, or since a start that kept nothing of a person, is named.
/// </param>
public sealed record PersonChangeParts(
    Func<string> MoveWebOn,
    Action DropPictures,
    Func<Task> ClearFiles,
    Func<Task> TakeFromSystem,
    Func<string, Task> EmptyWeb,
    Func<Func<Task>, Task> OnWindow,
    Func<IEnumerable<WipeStep>> More,
    Action<string> GiveWaitingTo);

/// <summary>
/// Another person signed in than the one before, and nobody signed out between: a session ran out, or the server ended it and said nothing
/// of it to the client, and somebody else signed in. The core has let go of the person before, and no effect says so. This hears who each
/// state names before the windows are given it (<see cref="Hear"/>), and when it is somebody else it takes away what the client keeps of
/// its own for the person before, as a sign-out does, but for the cookies, which are already those of the person who is here now.
/// </summary>
/// <remarks>
/// <para>
/// What has to be so before the windows are given the state is done before <see cref="Hear"/> returns, in this order: the change is owed
/// (<see cref="SessionKeeper.BeginPersonChange"/>: from then on the mark kept stays that of the person before until every part finished,
/// and what was kept has its turn to be deleted ahead of anything kept from now on); the web view's folder moves on, so a web page of the
/// person who is here now is opened in a new one or not at all; the pictures are let go of; the files chosen for forms begin to go; and
/// nothing waits to be notified and no count is shown. A part that throws is written down as failed and the parts after it are still done.
/// </para>
/// <para>
/// The rest follows, each part with the wipe's limit: the web views of the folder that ended are closed and what they kept cleared or
/// deleted, the copies of chosen files and what the system still shows are waited for, and whatever else the shell keeps of a person goes.
/// When every part finished, the mark of the person who is here now is written. When one did not, that is said, the mark of the person
/// before stays, and the next start does the change over.
/// </para>
/// <para>
/// It is used from one thread, the window's.
/// </para>
/// </remarks>
public sealed class PersonChange
{
    private readonly PersonWatch watch;
    private readonly SessionGate gate;
    private readonly SessionKeeper keeper;
    private readonly PersonChangeParts parts;
    private readonly Action<string> problem;
    private bool saidNoKey;

    // A cover was up, and the core has not given the app's part anew since: the part the state had under the cover is the one it has still.
    private bool afterCover;
    private long givenUnderCover;

    /// <summary>Makes it for one host.</summary>
    /// <param name="watch">Who was named last.</param>
    /// <param name="gate">The gate the host marks what the core says with.</param>
    /// <param name="keeper">What is kept on the device for the person.</param>
    /// <param name="parts">What the shell itself does at a change.</param>
    /// <param name="problem">
    /// Told, in words that name no person, when a change did not finish, and once when the core names a person and gives no key for them.
    /// It may be called on any thread.
    /// </param>
    public PersonChange(PersonWatch watch, SessionGate gate, SessionKeeper keeper, PersonChangeParts parts, Action<string> problem)
    {
        ArgumentNullException.ThrowIfNull(watch);
        ArgumentNullException.ThrowIfNull(gate);
        ArgumentNullException.ThrowIfNull(keeper);
        ArgumentNullException.ThrowIfNull(parts);
        ArgumentNullException.ThrowIfNull(problem);
        this.watch = watch;
        this.gate = gate;
        this.keeper = keeper;
        this.parts = parts;
        this.problem = problem;
    }

    /// <summary>The change of person that began last, with what came of each part when it is over; null when none has begun.</summary>
    public Task<WipeReport>? Last { get; private set; }

    /// <summary>
    /// The mark of the person what is kept is kept for: the one named last, and still that one while a later state names nobody, since
    /// they may sign in again and find everything as it was. Null when nobody was named since the app was first started or since the last
    /// sign-out: what is kept then is nobody's.
    /// </summary>
    public string? KeptFor => watch.Named;

    /// <summary>
    /// The mark of the person who is signed in now, by the state heard last; null while that state names nobody, while it is that of a
    /// person who is signing out, while a cover is up over the app (<see cref="WayInIsUp"/>), after a cover until the core has given the
    /// app's part anew, and before any state was heard. What waits for a person's eyes (a reminder) is shown only while this is their
    /// mark.
    /// </summary>
    /// <remarks>
    /// <para>
    /// When a session's end is found by an answer (a page, a region, a list or a form answered that the session is over), the core puts
    /// the way in up as a cover the person cannot take away. On a server that names nobody by id, or when the shell could not be asked
    /// again just then, it goes on naming the person whose session it was. So the state names them while whoever is at the device looks
    /// at the sign-in. Nobody is signed in then, whatever the state says of the app.
    /// </para>
    /// <para>
    /// Nor when the cover goes. It goes with the answer to a sign-in, and that answer says nothing of who signed in: the app's part is
    /// still the one from before, naming the person whose session ended, until the shell asked for then has answered. Whoever signed
    /// in may be somebody else. So after a cover nobody is taken to be signed in until the core has given the app's part anew: then the
    /// person it names is heard as any other, and when it is another person everything that waited for the one before has gone before
    /// this is their mark. When that shell does not answer, this stays null until the shell is asked again and does.
    /// </para>
    /// </remarks>
    public string? Here { get; private set; }

    /// <summary>
    /// Whether a cover is up over the app: something is presented that the person cannot take away. The core presents so the sign-in a
    /// session's end leads to, and a page a redirect led to that has to be answered before anything else (most often a sign-in too).
    /// Nobody is taken to be signed in while one is up.
    /// </summary>
    /// <param name="state">The state.</param>
    /// <returns>Whether one is up.</returns>
    public static bool WayInIsUp(ViewState state)
    {
        ArgumentNullException.ThrowIfNull(state);
        return state.Nav.Modals.Any(modal => modal.Locked);
    }

    /// <summary>
    /// Hears who a state names as signed in, as the state is about to be shown and before any window is given it. The state of a person
    /// who is signing out says nothing of who comes next, and a state that names nobody changes nothing. The person named is written down
    /// as the one what is kept is kept for (the keeper writes only a mark it has not written, and tries again one that could not be
    /// written). Another person than the one before, with no sign-out between, begins a change of person: what must be so before the
    /// windows see the state is so when this returns.
    /// </summary>
    /// <param name="state">The state about to be shown.</param>
    /// <param name="stateMark">The state's mark, from <see cref="SessionGate.Asked(IReadOnlyList{Effect}, out long)"/>.</param>
    /// <returns>What the state said of who is signed in.</returns>
    public PersonHeard Hear(ViewState state, long stateMark)
    {
        ArgumentNullException.ThrowIfNull(state);
        bool covered = Covered(state);
        if (gate.OfSomeoneLeaving(stateMark) || state.App?.User is not Models.User user)
        {
            // Nobody is signed in by this state, though what is kept is still kept for the person named before.
            Here = null;
            return PersonHeard.Nobody;
        }

        if (string.IsNullOrEmpty(user.Key) && !saidNoKey)
        {
            // The core always gives a key for the person it names. One that gives none is not the core this client was built with.
            saidNoKey = true;
            Say("the core named the person who is signed in and gave no key for them: they are told by their name and email, which two accounts can share");
        }

        PersonHeard heard = watch.Hear(user.Key, user.Name, user.Email);

        // Whom the state names is heard all the same under a cover and after one, so a change of person is seen. But nobody is signed
        // in while a cover is up, nor after it until the app's part has been given anew: until then the person named is the one whose
        // session ended, and whoever signed in at the cover may be somebody else.
        Here = covered ? null : watch.Named;
        if (watch.Named is not string mark)
        {
            return heard;
        }

        if (heard == PersonHeard.Another)
        {
            // Nothing stands between the watch taking the person who is here now and the change being owed: the first thing a change
            // does is to say so to the keeper.
            Last = ChangeAsync(mark);
        }
        else
        {
            if (heard == PersonHeard.First)
            {
                // What waits and is nobody's was asked for since the last sign-out, or since a start that kept nothing of a person: by
                // this person, between their sign-in and the shell's answer that names them, or by a visitor before them. It is
                // theirs from here on, and shown to nobody else.
                try
                {
                    parts.GiveWaitingTo(mark);
                }
                catch (Exception failure)
                {
                    Say("what waited for nobody could not be given to the person who signed in: " + failure.GetType().Name);
                }
            }

            keeper.KeepPerson(mark);
        }

        return heard;
    }

    /// <summary>Somebody signed out: the sign-out takes everything away itself, and whoever is named after it is the first.</summary>
    public void SignedOut()
    {
        watch.SignedOut();
        Here = null;
    }

    // Whether what the state says of who is signed in is not to be gone by: a cover is up, or one was and the app's part is still the
    // one the state had under it. The core says the app's part changed when it draws a shell, at a sign-out and when it forgets the
    // session, and the client counts each time (ViewState.AppGiven): a state with another count than the one under the cover holds a
    // part the core gave since, which says who is signed in now. The same count is the same part, also when every part was read
    // again meanwhile. Every state is held to this, the ones that name nobody too.
    private bool Covered(ViewState state)
    {
        if (WayInIsUp(state))
        {
            afterCover = true;
            givenUnderCover = state.AppGiven;
            return true;
        }

        if (afterCover && state.AppGiven == givenUnderCover)
        {
            return true;
        }

        afterCover = false;
        return false;
    }

    private async Task<WipeReport> ChangeAsync(string mark)
    {
        // Owed from this line on, before anything is taken away: whatever happens below, the mark kept is that of the person before
        // until every part has finished, so a start after a change that was cut short does it over.
        PersonChangeBegun begun = keeper.BeginPersonChange();

        // Here and now, before the windows are given the state that names the person who is here now.
        string? webEnded = null;
        Task moved = Now(() => webEnded = parts.MoveWebOn());
        Task pictures = Now(parts.DropPictures);
        Task files = Begun(parts.ClearFiles);
        Task system = Begun(parts.TakeFromSystem);

        WipeReport report;
        try
        {
            report = await keeper.FinishPersonChangeAsync(
                begun,
                mark,
                [
                    new WipeStep("the web view's folder", () => moved),
                    new WipeStep("the pictures", () => pictures),
                    new WipeStep("what the web view keeps", () => webEnded is string ended ? parts.EmptyWeb(ended) : Task.CompletedTask, parts.OnWindow),
                    new WipeStep("the files chosen for forms", () => files),
                    new WipeStep("the notifications, the count and the jump list", () => system),
                    .. parts.More(),
                ]).ConfigureAwait(false);
        }
        catch (Exception failure)
        {
            report = new WipeReport([], [new KeyValuePair<string, string>("the change of person", failure.GetType().Name)]);
        }

        // What did not finish is done over at the next start: the mark of the person before is still the one kept.
        if (!report.Complete)
        {
            Say("what was kept for the person who was signed in before was not all taken away: " + string.Join(", ", report.Failed.Select(failed => failed.Key)));
        }

        return report;
    }

    // Told or not, what is said of is as it is: whoever listens cannot stop it.
    private void Say(string what)
    {
        try
        {
            problem(what);
        }
        catch (Exception)
        {
            // Nothing to do for it here.
        }
    }

    // A part that is done at the call: what it throws is kept for the report, and the next part is still done.
    private static Task Now(Action part)
    {
        try
        {
            part();
            return Task.CompletedTask;
        }
        catch (Exception failure)
        {
            return Task.FromException(failure);
        }
    }

    // A part that begins at the call and finishes later: one that throws as it begins has failed.
    private static Task Begun(Func<Task> part)
    {
        try
        {
            return part();
        }
        catch (Exception failure)
        {
            return Task.FromException(failure);
        }
    }
}
