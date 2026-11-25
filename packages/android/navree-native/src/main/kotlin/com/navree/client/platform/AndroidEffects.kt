package com.navree.client.platform

import android.content.ActivityNotFoundException
import android.content.ClipData
import android.content.ClipDescription
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.os.PersistableBundle
import android.util.Log
import android.view.HapticFeedbackConstants
import android.webkit.WebStorage
import android.webkit.WebView
import android.webkit.WebViewDatabase
import androidx.activity.ComponentActivity
import androidx.browser.customtabs.CustomTabsClient
import androidx.browser.customtabs.CustomTabsIntent
import androidx.core.app.NotificationManagerCompat
import androidx.webkit.WebStorageCompat
import androidx.webkit.WebViewFeature
import com.navree.client.BuildConfig
import com.navree.client.R
import com.navree.client.core.Driver
import com.navree.client.core.Effect
import com.navree.client.core.EffectRunner
import com.navree.client.core.Events
import com.navree.client.core.Notice
import com.navree.protocol.Navree
import kotlinx.serialization.json.JsonObject
import okhttp3.HttpUrl
import java.io.File
import java.security.SecureRandom
import java.util.concurrent.CountDownLatch
import java.util.concurrent.Executors
import java.util.concurrent.RejectedExecutionException
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean
import kotlin.concurrent.thread

/**
 * Empties what the system's web views keep for the app: the resource cache on
 * disk, the back and forward list, the form data, the saved sign-ins for
 * sites, and what sites stored. Where the system's web view has it
 * (`WebViewFeature.DELETE_BROWSING_DATA`), what sites stored goes by
 * `WebStorageCompat.deleteBrowsingData`, which deletes "all data stored by
 * websites", "network cache, cookies, and any JavaScript-readable storage",
 * and says when it is done: [done] runs then, told that every kind went (the
 * reference gives that call no limit of time, so the caller sets its own). Where
 * it has not, the only call left is [WebStorage.deleteAllData], which its
 * reference says covers the Web SQL database and Web Storage and which says
 * nothing when it is done: [done] runs at once, told that not every kind went.
 * On the system web view of the tested emulator (API 35), which has not, it
 * took IndexedDB too and left Cache Storage and a page's service worker: those
 * are what [SiteWipe.documents] then takes away. Cookies are also the cookie
 * store's ([WebViewCookies.clear]). Call on the main thread; [done] runs on
 * it, once. Answers whether every kind of site data was asked to go.
 */
fun clearWebViews(context: Context, done: (everyKind: Boolean) -> Unit = {}): Boolean {
    clearWebViewKept(context)
    val storage = WebStorage.getInstance()
    if (WebViewFeature.isFeatureSupported(WebViewFeature.DELETE_BROWSING_DATA)) {
        WebStorageCompat.deleteBrowsingData(storage) { done(true) }
        return true
    }
    storage.deleteAllData()
    done(false)
    return false
}

/**
 * Empties what the system's web views keep for the app beside what sites
 * stored and the cookies: the resource cache on disk, the back and forward
 * list, the form data, and the saved sign-ins for sites. No cookie goes by
 * this, so it is also what is done when another person is signed in with no
 * sign-out between: the one store holds their session's cookies by then, and
 * what goes from it is decided apart ([WebViewCookies.keepOwn]). Call on the
 * main thread.
 */
fun clearWebViewKept(context: Context) {
    val database = WebViewDatabase.getInstance(context)
    @Suppress("DEPRECATION")
    database.clearFormData()
    database.clearHttpAuthUsernamePassword()
    // The cache is the app's, shared by every web view; clearing it needs one.
    WebView(context).apply {
        clearCache(true)
        clearFormData()
        clearHistory()
        destroy()
    }
}

/**
 * Carries out the core's effects with Android's own means. Effects that need
 * a window (the share sheet, a sign-in, a haptic, an announcement) use the
 * activity attached now; what comes of every effect is a later event, sent
 * to the driver on the main thread.
 */
class AndroidEffects(
    context: Context,
    private val driver: Driver,
    origin: HttpUrl,
    googleClientId: String?,
    push: PushProvider?,
    /** The app's origins, asked again of every address before it is fetched, streamed or shown. */
    val origins: Origins = Origins(origin.toString()),
    loopbackHttp: Boolean = false,
) : EffectRunner {
    private val app = context.applicationContext
    private val main = Handler(Looper.getMainLooper())
    val cookies = WebViewCookies(kept = File(app.noBackupFilesDir, OwnCookies.FILE))

    /** Everything that touches what is kept on the device runs here, in order, one piece at a time. */
    private val storeThread = Executors.newSingleThreadExecutor { work -> Thread(work, "navree-store").apply { isDaemon = true } }
    val state = StoredState { work ->
        try {
            storeThread.execute(work)
        } catch (_: RejectedExecutionException) {
            // The session was closed: nothing more is kept.
        }
    }
    /** The files chosen for a form's file fields, which a request that sends them reads from. */
    val picked = PickedFiles.of(app).also { files -> state.always { files.purgeStale() } }
    val http = CoreHttp(cookies, origins, loopbackHttp, state, picked::body, picked::sent) { post(it) }
    private val streams = CoreStreams(http.client, origins, cookies, state) { post(it) }
    private val cache = SealedCache(app)
    private val timers = HashMap<Long, Runnable>()
    private val push = push
    val devices = DeviceCalls(origin, googleClientId, pushes = push != null, tokenGranted = ::askToken) { post(it) }

    /** What the core keeps for a launch the system ended: sealed, out of backups, gone at sign-out. */
    val snapshots = SealedCache(app, "navree-snapshot")
    private val random = SecureRandom()

    /** The push service's side of the end of a session: the token's deletion, owed until the service says it is gone. */
    private val tokens = PushEnd(push, OwedToken.of(app), onMain = { work -> main.post { work() } }, emptyShade = ::emptyShade, ask = ::askToken)

    /** A go at every origin is on its way to its turn in the store, or in it: a second asking adds none. */
    private val clearing = AtomicBoolean(false)

    /** The session is closed: the store's queue takes nothing more. */
    private val closed = AtomicBoolean(false)

    /** The end of a session whose deletion is on its way, while it is ([Turn]). */
    @Volatile
    private var ending: Turn? = null

    /**
     * Who the core last said is signed in, and on the disk the mark of whom
     * what is kept here is kept for: read now, so this run sees a change of
     * person that happened while the app was closed.
     */
    private val person = PersonWatch(File(app.noBackupFilesDir, PersonWatch.MARK), "${origin.scheme}://${origin.host}:${origin.port}")

    /**
     * The secret of this install, which the core makes its key for a person
     * with: read now, and made and written down when there is none
     * ([InstallSecret]). It goes to the core and nowhere else.
     */
    internal val installSecret: String = InstallSecret.of(File(app.noBackupFilesDir, InstallSecret.FILE))

    /**
     * Whether the mark this start read is of a plain key, made before the
     * install had a secret: the core is then asked to say what the plain key
     * of the person it names was, so that person is not taken for another.
     */
    internal val keepsPlainKey: Boolean = person.keepsPlainKey

    /**
     * The core's two words for the end of a session. `session_ending` comes
     * in the update in which the core asks the server to forget this device's
     * push token: the request for the removal stands before it in the same
     * update, so the store has given the removal its cookies before the turn
     * of the end of the session comes. The push service is asked to delete
     * the token then as well: until it has, the server may still send to the
     * token and the push service still deliver, and what arrives in that time
     * is not drawn (the messaging service asks [OwedToken] first).
     */
    private val ends = SessionEnd(begin = ::end, tokens = tokens, emptyShade = ::emptyShade)

    init {
        cookies.onOwn = ::cookieComing
        // Read here what an earlier run of the app still owes of the end of a session, and said here how a go that is asked for is
        // run: once everything that needs is made.
        SiteWipe.arm(app, origins, again = ::clearPages)
        if (SiteWipe.owesAll) {
            // A run of the app was told that the session ended and did not get as far as the cookies. What the app reads at
            // its start goes here and now; then all of the end of a session is done again, in the store's turn, before
            // anything of this run is kept.
            try {
                cache.clear()
                snapshots.clear()
                picked.clear()
            } catch (_: Throwable) {
                // The same three go again in the store's turn.
            }
            ends.forgotten()
        } else if (SiteWipe.owesPages && !SiteWipe.tried) {
            // What pages stored is owed: at every origin, where a run of the app ended in the middle of taking it away, or at the
            // origins a go left. It is gone at with the cookie store emptied, and that has its turn in the store before anything
            // this run asks for. Not where a go at the origins that were left has left them again: an origin that cannot be
            // cleared does not hold every start of the app up, and "Try again" on a page of it has another go.
            clearPages()
        }
        // The token of a session a run before this one was told had ended, where the push service had not said it is deleted:
        // asked for again now, whatever else is owed or not.
        tokens.delete()
    }

    /** The activity in front, while there is one. */
    var activity: ComponentActivity? = null

    /** A sign-in session is open in the browser: what comes back to the app ends it. */
    var signingIn = false
        private set

    /** The features this build carries out, by the protocol's names, for `client.caps`. */
    val caps: List<String> = buildList {
        addAll(listOf("clipboard", "open_url", "auth_session"))
        addAll(devices.features)
    }

    private fun post(event: JsonObject) {
        main.post { driver.send(event) }
    }

    /**
     * Hears who the core says is signed in. The driver says it each time the
     * app's part of the view state is read back, which is before any effect
     * of the same update is carried out and before anything is drawn from it;
     * and it is asked again before each effect, for a host that gives the
     * driver only this runner. While a session is ending, what the core says
     * is of the person who is leaving, or of nobody, and says nothing of who
     * comes next. A person is who the core's key says they are, and where
     * the core gives none, who their name and email say ([PersonWatch]). The
     * mark of the first person named, or of the same one, is written down as
     * whom what is kept here is kept for; another person than the one
     * before, with no sign-out between, is a change of person
     * ([personChanged]). Where the core says beside the person what their
     * plain key was (`app.user_was`), that is heard with them: the person a
     * mark of that key was kept for is the same one under the key they have
     * now, and only the mark is written anew.
     */
    fun hears(user: Navree.User?) {
        if (user == null || ends.under) return
        val word = person.hear(user.key, user.name, user.email, driver.app?.userWas)
        when {
            word.heard == Heard.Another -> personChanged(word)
            // Dropped when a session ends before its turn: the sign-out takes the mark with everything else.
            word.write -> state.write { person.keep(word) }
        }
    }

    /**
     * Another person is signed in than the one what is kept here was kept
     * for, and nobody signed out between: a session ran out, or the server
     * ended it and said nothing of it to the client, and somebody else signed
     * in, in this run or while the app was closed. The core has let go of
     * the person before and has its own cache emptied. What the client keeps
     * of its own for that person goes here, before the state that names the
     * new one is drawn:
     * - the app's notifications in the shade;
     * - the web view's cache, history, form data and saved sign-ins for
     *   sites, now ([clearWebViewKept]);
     * - what pages stored at the origins, owed from this moment and written
     *   down ([SiteWipe.owePages]), so no web page is shown until it is gone
     *   and a start of the app goes on with what a run did not finish;
     * - in the store's turn, ahead of anything kept from now on and of any
     *   fetch or stream of the client's own: the copies of the files chosen
     *   for forms, the snapshot of the launch, and then what pages stored
     *   and what the web view's side put among the cookies ([clearEvery]).
     *
     * The client keeps nothing else for a person: pictures are held only by
     * the screens that draw them, which the core has taken down, and the
     * client schedules no notification and writes no count or shortcut of
     * its own.
     *
     * The mark of the person before stays on the disk until the store's turn
     * has done its part, and only then is the new person's written in its
     * place ([PersonWatch.changed]). An app that ends before that leaves the
     * old mark, and the next start hears another person than the one it
     * remembers and does all of this over. A cookie store that does not say
     * in time that its cookies are gone leaves the old mark too, and the
     * whole of an end of a session owed with it ([SiteWipe.widen]).
     */
    private fun personChanged(word: PersonWatch.Word) {
        emptyShade()
        SiteWipe.owePages()
        try {
            clearWebViewKept(app)
        } catch (_: Throwable) {
            // No web view on this device: nothing of it to clear, and no page of it to show.
        }
        // Dropped when a session ends before its turn: the sign-out deletes all of it itself, and forgets the mark.
        state.write {
            picked.clear()
            snapshots.clear()
            // Without the cookie store's word in time the change is not finished: the mark of the person before stays.
            if (clearEvery()) person.changed(word)
        }
    }

    /**
     * A go at what pages stored is asked for: at a start of the app that
     * finds it owed, by "Try again" on a page that is held back, and when the
     * cookie store says late that its cookies are gone ([SiteWipe.again]).
     * Every go is run with no session in the cookie store ([clearEvery]), in
     * the store's turn, whether it is at every origin or at the origins an
     * earlier go left: an origin that was left still holds the worker of the
     * person before, and the go's document starts it. The cookies the
     * answers to the client's own requests set go back in afterwards, at a
     * start of the app too, which knows them from the disk ([OwnCookies]): so
     * the person signed in stays so. Call on the main thread.
     */
    private fun clearPages() {
        if (!SiteWipe.awaitsGo || !clearing.compareAndSet(false, true)) return
        val work = {
            try {
                // Asked again in its turn: a go that ended meanwhile, or the end of a session, has seen to it.
                if (SiteWipe.awaitsGo) clearEvery()
            } finally {
                clearing.set(false)
            }
        }
        // A session that is closed has no queue any more, and what is owed is the process's: the go is run all the same.
        if (closed.get()) thread(name = "navree-web-wipe", isDaemon = true) { work() } else state.always(work)
    }

    /** What a go begun from the store's thread came to, told across to it from the main thread. */
    private class Went {
        /** Taken by whichever comes first: the main thread, about to ask for the go, or the store's thread, which waits for that no longer. */
        val taken = AtomicBoolean(false)

        /** The number of what the go was for, where one began. */
        @Volatile
        var number: Long? = null

        /** What the go left; null where it could not be made. */
        @Volatile
        var left: Set<String>? = null

        /** The go said it ended. */
        @Volatile
        var ended = false

        /** The store's thread waits no longer: a go that begins after this ends at once. */
        @Volatile
        var gone = false

        /** Counted down when the main thread has asked for the go, whether one began or not. */
        val asked = CountDownLatch(1)

        /** Counted down when the go ended, or none began. */
        val over = CountDownLatch(1)
    }

    /**
     * Takes away what pages stored, at every origin or at the origins an
     * earlier go left, with no session in the cookie store while it is done.
     * On the store's thread and in its turn, so the queue waits and no fetch
     * or stream of the client's own reads the store meanwhile:
     * 1. From here on the store is said to stand empty ([SiteWipe.storeEmpty]):
     *    no web page is shown at any origin until its own cookies are back,
     *    since a page shown meanwhile would ask for everything as somebody
     *    who is not signed in. Every cookie goes out of the store, and the
     *    ones the answers to the client's own requests set are remembered as
     *    taken out ([WebViewCookies.takeOut]). The go's document on each
     *    origin starts the service workers of the person before; whatever
     *    such a worker sends, by a request the client is asked about or by
     *    one it is not (a socket), it sends with no session to carry.
     * 2. The go runs ([SiteWipe.go]), for no longer than its origins have for
     *    their tries and twenty seconds at the most ([SiteWipe.goWait]); one
     *    that is still at its origins then ends there, and what it had not
     *    cleared is left for a later go, which is run the same way.
     * 3. The store is emptied again, of whatever a worker the go started
     *    wrote there, and its own cookies go back in
     *    ([WebViewCookies.keepOwn]): the sign-in of the person who is here
     *    now among them, so they stay signed in.
     * 4. Only then is the deletion called over ([SiteWipe.over]) and are the
     *    pages of the origins it cleared shown.
     *
     * A cookie store that does not say in time, at step 1 or 3, that its
     * cookies are gone leaves the whole of an end of a session owed
     * ([SiteWipe.widen]): no go is begun without its word at step 1, no web
     * page is shown for the rest of this run, and the next start does all of
     * it. So does a go that has not said it ended a second after it was told
     * to, and a main thread that has not come to asking for one. Answers
     * whether the store said so both times.
     */
    private fun clearEvery(): Boolean {
        val onLooper = { work: () -> Unit ->
            main.post { work() }
            Unit
        }
        SiteWipe.storeEmpty(true)
        try {
            val out = cookies.takeOut(onLooper, COOKIES_WAIT_MS)
            val went = Went()
            // Whether the main thread has done with asking for the go, or never came to it.
            var asked = true
            if (out) {
                // By the number of origins the go is at: none is left only because one before it was slow.
                val wait = SiteWipe.goWait()
                main.post {
                    // The store's thread waited no longer for this to have its turn: no go begins behind it.
                    if (!went.taken.compareAndSet(false, true)) return@post
                    try {
                        val began = SiteWipe.go { left, of ->
                            went.number = of
                            went.left = left
                            went.ended = true
                            went.over.countDown()
                        }
                        if (began == null) went.over.countDown() else if (went.number == null) went.number = began
                        // Begun too late: the store has its cookies back, or is about to.
                        if (went.gone) SiteWipe.stopGo()
                    } catch (_: Throwable) {
                        went.over.countDown()
                    } finally {
                        went.asked.countDown()
                    }
                }
                if (!went.over.await(wait, TimeUnit.MILLISECONDS)) {
                    // The queue waits no longer: the go ends here, and says at once what it had not cleared.
                    main.post { SiteWipe.stopGo() }
                    went.over.await(STOP_WAIT_MS, TimeUnit.MILLISECONDS)
                }
                asked = went.taken.compareAndSet(false, true) || went.asked.await(STOP_WAIT_MS, TimeUnit.MILLISECONDS)
                went.gone = true
            }
            val back = cookies.keepOwn(onLooper, COOKIES_WAIT_MS)
            val emptied = out && back
            val number = went.number
            when {
                number != null && went.ended && emptied -> SiteWipe.over(went.left, number)
                // A go that never said it ended, or a store that never said it is empty: nothing is called gone.
                number != null -> SiteWipe.widen(number)
                // And so where the main thread is still asking for the go: what it begins ends at once, and nothing is known of it.
                !emptied || !asked -> SiteWipe.widen(null)
            }
            return emptied
        } finally {
            // Its own cookies are back, or the whole is owed and every page is held back for that.
            SiteWipe.storeEmpty(false)
        }
    }

    /**
     * The cookie store is emptied once more and given its own cookies back
     * ([WebViewCookies.keepOwn]), in a turn of the store's own, and [then] is
     * told whether the store said in time that it is empty. What follows a go
     * of the end of a session that was still at its origins when the queue
     * had waited its five seconds and gone on.
     */
    private fun closing(then: (emptied: Boolean) -> Unit) {
        val step = { then(cookies.keepOwn(onLooper = { work -> main.post { work() } }, waitMs = COOKIES_WAIT_MS)) }
        // A session that is closed has no queue any more, and what is owed is the process's: the step is done all the same.
        if (closed.get()) thread(name = "navree-web-wipe", isDaemon = true) { step() } else state.always(step)
    }

    /** Takes every notification of the app out of the shade, those the push service drew itself among them. */
    private fun emptyShade() {
        try {
            NotificationManagerCompat.from(app).cancelAll()
        } catch (_: Throwable) {
            // Nothing shown.
        }
    }

    override fun run(effect: Effect) {
        hears(driver.app?.user)
        when (effect) {
            is Effect.Fetch -> http.fetch(effect)
            is Effect.Cancel -> http.cancel(effect.request)
            is Effect.OpenUrl -> open(effect.url, effect.inside)
            is Effect.SignIn -> signIn(effect)
            // Messages are drawn from the view state's list.
            is Effect.Show -> Unit
            is Effect.Haptic -> haptic(effect.kind)
            is Effect.Device -> devices.run(activity, effect)
            // A page read for a session that has ended is answered as none kept.
            is Effect.CacheRead -> {
                val key = effect.key
                state.read { current -> post(Events.cache(key, if (current) cache.read(key) else null)) }
            }
            is Effect.CacheWrite -> {
                val (key, value) = effect.key to effect.value
                state.write { cache.write(key, value) }
            }
            Effect.CacheClear -> state.always { cache.clear() }
            is Effect.Announce -> announce(effect)
            is Effect.Copy -> copy(effect.text, effect.sensitive)
            is Effect.Timer -> {
                val timer = effect.timer
                val run = Runnable {
                    timers.remove(timer)
                    driver.send(Events.timer(timer))
                }
                timers[timer] = run
                main.postDelayed(run, effect.ms)
            }
            is Effect.CancelTimer -> timers.remove(effect.timer)?.let(main::removeCallbacks)
            is Effect.StreamOpen -> streams.open(effect)
            is Effect.StreamClose -> streams.close(effect.stream)
            Effect.Exit -> activity?.finish()
            is Effect.Report -> if (BuildConfig.DEBUG) Log.w(TAG, "${effect.what}: ${effect.detail}")
            // Answered at once, as the core asks; the bytes are a secret and are never logged.
            is Effect.Random -> {
                val bytes = ByteArray(maxOf(effect.bytes, 48))
                random.nextBytes(bytes)
                driver.send(Events.random(effect.request, bytes.joinToString("") { "%02x".format(it) }))
                bytes.fill(0)
            }
            Effect.SessionEnding -> ends.ending()
            Effect.ForgetSession -> ends.forgotten()
            is Effect.Unknown -> if (BuildConfig.DEBUG) Log.w(TAG, "an effect this build does not know: ${effect.t}")
        }
    }

    /**
     * Asks the push service for this install's token and hands it to the
     * core, which registers it with the server. At every launch, and when the
     * person allows notifications. While the token of a session that ended
     * is still to delete, none is asked for, since the service would give
     * that one: its deletion is asked for instead, and the new token when it
     * is gone ([PushEnd]).
     */
    fun askToken() {
        val push = push ?: return
        PushTokens.listener = { token -> post(Events.pushToken(token)) }
        if (tokens.owed) return tokens.delete()
        push.token { token -> token?.let { post(Events.pushToken(it)) } }
    }

    /**
     * The session is ending on this device. At once, before anything is
     * waited for, that is written down and forced to the disk
     * ([SiteWipe.owe]): from that moment the whole of what follows is owed,
     * and a run of the app that ends anywhere after it leaves the next start
     * to do it. Then, in the store's turn, after whatever
     * was asked before: the sealed page cache and the snapshot go; then, on
     * the main thread, the app's notifications in the shade and every cookie
     * of the one store the web view shares; then the web view's cache,
     * history, form data and saved sign-ins for sites, and what sites stored
     * ([clearWebViews]), whose word that it is done is waited for no longer
     * than [SITES_WAIT_MS]; then, on either kind of web view, the service
     * workers and the caches of each of the app's origins and web hosts,
     * through a document of the client's own that also says what is left
     * ([SiteWipe.documents]); then every cookie once more, since a service
     * worker that document started can write one
     * ([WebViewCookies.keepOwn]). Nothing kept for the session after runs before
     * all of that is done, or five seconds have passed (a limit armed before
     * anything else, so a failure of any kind cannot hold the queue), and no
     * write of the session that ended runs at all. What the web view holds is
     * owed its deletion from the first step until the web view has said it is
     * gone ([SiteWipe]): if the five seconds pass first, or the app ends, no
     * web page is shown until it has, and the next start finishes it; an
     * origin that does not say so is left and its pages stay held back, while
     * the pages of the others are shown again. Until the cookies are gone and
     * on the disk the mark owes the whole of this, and a start of the app that
     * finds it so does all of it again, the push token's deletion with it,
     * before anything of that run is kept (the `init` above). The cookie
     * store is waited for no longer than [COOKIES_WAIT_MS]: what follows the
     * cookies then goes on without its word, and the mark goes on owing the
     * whole until the store has said they are gone.
     *
     * The push service's token has a mark of its own, written here before the
     * other ([PushEnd.owe]): its deletion is owed from this moment until the
     * push service has said the token is gone, whatever becomes of the rest,
     * and a start of the app that finds it owed asks for it again.
     */
    private fun end() {
        // Everything of the person goes with the session, the mark of whom it was kept for among it: whoever the core names
        // next is the first of the next session.
        person.signedOut()
        // Owed from this moment and written down, whatever the store is still busy with: a page of the next session is not
        // shown over what this one's pages left, and a start of the app after a run that ended here does all of it.
        tokens.owe()
        val debt = SiteWipe.owe()
        val turn = Turn()
        ending = turn
        state.end { finished ->
            // The time limit is armed before anything it guards: whatever happens below, the queue goes on.
            val limit = { release: () -> Unit ->
                main.postDelayed({
                    turn.late = true
                    release()
                }, WIPE_WAIT_MS)
                Unit
            }
            endWithLimit(finished, arm = limit) { done -> wipe(debt, turn, done) }
        }
    }

    /**
     * One end of a session's deletion, as far as the cookie store goes: its
     * go at the origins runs only while the store holds no cookie an answer
     * has set since the store was emptied for it. While the store's queue
     * waits, none can; once the queue has gone on, the first answer to one of
     * the client's own requests that sets a cookie (the next person's
     * sign-in, or the app's server answering nobody) ends the go before the
     * cookie is in the store, or keeps the go from beginning.
     */
    private class Turn {
        /** An answer has a cookie for the store, the first since it was emptied for this end of a session. */
        val cookie = AtomicBoolean(false)

        /** Its go is at the origins. Written on the main thread. */
        @Volatile
        var going = false

        /** The queue waited its five seconds and has gone on. Written and read on the main thread. */
        var late = false
    }

    /**
     * An answer to one of the client's own requests is about to put a cookie
     * into the store ([WebViewCookies.onOwn]). Where the go of an end of a
     * session is at the origins, it ends here, before the cookie is there
     * ([Turn]): what it had not cleared is left for a later go. The go ends
     * on the main thread, and that is waited for, no longer than
     * [STOP_WAIT_MS]; an answer's cookies are saved in the store's turn and
     * never on the main thread.
     */
    private fun cookieComing() {
        val turn = ending ?: return
        turn.cookie.set(true)
        if (!turn.going) return
        if (Looper.myLooper() == Looper.getMainLooper()) return SiteWipe.stopGo()
        val stopped = CountDownLatch(1)
        main.post {
            try {
                if (turn.going) SiteWipe.stopGo()
            } finally {
                stopped.countDown()
            }
        }
        stopped.await(STOP_WAIT_MS, TimeUnit.MILLISECONDS)
    }

    /**
     * The steps of [end], in order, for the end of a session numbered [debt]
     * ([SiteWipe.owe]): what it says to the mark it says for that end only, so
     * a later end of a session is not called done by this one. The go at the
     * origins runs only while no answer has put a cookie into the store again
     * ([turn]). After it the cookie store is emptied once more, since a
     * service worker the go started can write a cookie: before [done], which
     * lets the store's queue go on, where the go ended inside the five
     * seconds the queue waits, so nothing asked meanwhile is sent with what a
     * worker wrote; in a turn of its own where the queue had gone on already
     * ([closing]). Only then is the deletion called over and are the pages of
     * the origins it cleared shown again.
     */
    private fun wipe(debt: Long, turn: Turn, done: () -> Unit) {
        try {
            cache.clear()
            snapshots.clear()
        } catch (_: Throwable) {
            // A file that could not be deleted is sealed under a key only this app holds.
        }
        try {
            // The copies of the files chosen for forms are the person's own documents, kept unsealed in the cache folder.
            picked.clear()
        } catch (_: Throwable) {
            // A copy that could not be deleted is in the cache folder, which no backup takes and the system may empty.
        }
        main.post {
            emptyShade()
            // The store is about to be emptied for this end of a session: no answer has put a cookie into it since.
            turn.cookie.set(false)
            // Told once, whichever way the go ends.
            val ended = AtomicBoolean(false)
            val last = { left: Set<String>? ->
                if (ended.compareAndSet(false, true)) {
                    turn.going = false
                    if (ending === turn) ending = null
                    // The store is emptied again, of whatever a worker the go started wrote there, and given back the cookies
                    // answers have set since (the next person's sign-in, where it came first). Only then is the deletion called
                    // over; a store that does not say in time that it is empty leaves the whole owed.
                    val close = { emptied: Boolean -> if (emptied) SiteWipe.over(left, debt) else SiteWipe.widen(debt) }
                    if (turn.late) {
                        // The queue has gone on: the store is emptied in a turn of its own, so no request reads it meanwhile.
                        done()
                        closing(close)
                    } else {
                        // Still the turn of the end of the session: the store is emptied before what was asked meanwhile goes on.
                        thread(name = "navree-web-wipe", isDaemon = true) {
                            val emptied = try {
                                cookies.keepOwn(onLooper = { work -> main.post { work() } }, waitMs = COOKIES_WAIT_MS)
                            } catch (_: Throwable) {
                                false
                            }
                            done()
                            close(emptied)
                        }
                    }
                }
            }
            // Whether the cookie store has said its cookies are gone: only then are they called gone in the mark.
            val cookiesGone = AtomicBoolean(false)
            // What follows the web view's own deletion goes on once: when the web view says what sites stored is gone, at once
            // where it cannot say, and after SITES_WAIT_MS where it has not said.
            val started = AtomicBoolean(false)
            val rest = {
                if (started.compareAndSet(false, true)) {
                    // The cookies are written to disk before the wipe is done, off the main thread: nothing after it, the end
                    // of the session's thread included, comes first. Once they are, the mark owes only what pages stored at
                    // the origins: a run of the app that ends before that does all of this again. Where the cookie store has
                    // not said its cookies are gone, the mark goes on owing the whole.
                    thread(name = "navree-cookie-flush", isDaemon = true) {
                        val flushed = try {
                            cookies.flush()
                            if (cookiesGone.get()) SiteWipe.narrow(debt)
                            true
                        } catch (_: Throwable) {
                            false
                        }
                        main.post {
                            // Whatever the web view's own call took, and whether or not it has said so: a document of the
                            // client's own on each origin unregisters its service workers, deletes its caches and says what
                            // is left, so nothing is called gone on that call's word alone. Not while the cookie store has
                            // not said its cookies are gone: no origin is shown a document while the session's cookies may
                            // stand, and the deletion is called over with the whole owed still.
                            // Nor once an answer has put a cookie into the store again, which it can once the queue has
                            // stopped waiting: a go at every origin runs only with no session in the store. What is owed
                            // stays so then, at every origin, for the next go that is asked for.
                            turn.going = true
                            if (!flushed || !cookiesGone.get() || turn.cookie.get()) {
                                last(null)
                            } else {
                                try {
                                    SiteWipe.documents(app, origins) { left -> last(left) }
                                } catch (_: Throwable) {
                                    last(null)
                                }
                            }
                        }
                    }
                }
            }
            // The cookies first: they do not wait for the web view's deletion of what sites stored, which has no limit of its own.
            // Nor does what follows wait for the cookie store longer than COOKIES_WAIT_MS: a store that never answers does not
            // keep the web view's cache and what sites stored from being deleted, nor the deletion from being called over, after
            // which something can be done about what is owed again. The store's word then comes late or never, and until it
            // comes the mark owes the whole.
            stepWithLimit(
                arm = { release -> main.postDelayed({ release() }, COOKIES_WAIT_MS) },
                step = { said -> cookies.clear { said() } },
                late = {
                    // The cookies are gone after all: written to the disk, the mark then owes only what pages stored, and
                    // that is a go's to take, begun here unless the deletion is still on its way and comes to it itself.
                    cookiesGone.set(true)
                    thread(name = "navree-cookie-flush", isDaemon = true) {
                        val narrowed = runCatching {
                            cookies.flush()
                            SiteWipe.narrow(debt)
                        }.isSuccess
                        if (narrowed) main.post { SiteWipe.again() }
                    }
                },
            ) { said ->
                cookiesGone.set(said)
                main.postDelayed({ rest() }, SITES_WAIT_MS)
                try {
                    clearWebViews(app) { rest() }
                } catch (_: Throwable) {
                    // No web view on this device: nothing of it to clear, and no page of it to show.
                    rest()
                }
            }
        }
    }

    /**
     * The session is gone for good. The store's thread is ended through its
     * own queue, in its turn: a wipe already asked for, and the cookie flush
     * that follows it, run first.
     */
    fun close() {
        streams.closeAll()
        closed.set(true)
        state.always { storeThread.shutdown() }
    }

    /** The app left the front: streams close (the core opens them again when it comes back). */
    fun stopped() {
        streams.closeAll()
    }

    /** Where the open sign-in session comes back: its callback, as the core gave it. */
    private var signInCallback: String? = null

    /**
     * An address the app was opened with. While a sign-in session is open, a
     * link that can be its return ([signInReturn]) ends it; any other link is
     * a link to open.
     */
    fun opened(url: String) {
        if (signingIn && signInReturn(url, signInCallback, origins)) {
            signingIn = false
            driver.send(Events.signInDone(url))
        } else {
            driver.send(Events.openLink(url))
        }
    }

    /**
     * The activity is in front again. A sign-in session that brought nothing
     * back was given up. A push token whose deletion the push service has not
     * yet said is done is asked for again.
     */
    fun resumed() {
        tokens.delete()
        activity?.let { devices.resumed(it) }
        if (signingIn) {
            main.postDelayed({
                if (signingIn) {
                    signingIn = false
                    driver.send(Events.signInDone(null))
                }
            }, 600)
        }
    }

    /**
     * Opens an address outside the app: only `http`, `https`, `mailto`,
     * `tel`, `sms` and `geo` are handed to the system ([handedToSystem]); the
     * system's browser inside the app takes only a web address ([webAddress]).
     */
    private fun open(url: String, inside: Boolean) {
        val context = activity ?: return
        if (!handedToSystem(url)) {
            if (BuildConfig.DEBUG) Log.w(TAG, "an address of a kind the system is not asked to open")
            return
        }
        val uri = Uri.parse(url)
        try {
            if (inside && webAddress(url)) {
                CustomTabsIntent.Builder().setShowTitle(true).build().launchUrl(context, uri)
            } else {
                context.startActivity(Intent(Intent.ACTION_VIEW, uri).addCategory(Intent.CATEGORY_BROWSABLE))
            }
        } catch (_: ActivityNotFoundException) {
            // Nothing on the device opens this kind of address.
        }
    }

    /**
     * The system's sign-in session: Custom Tabs, which shares the browser's
     * sign-ins unless the session must share nothing. A session that must
     * share nothing opens only in a browser that says it can keep it apart
     * (`CustomTabsClient.isEphemeralBrowsingSupported`); with none, no session
     * opens and the core is told the person did not sign in. It comes back to
     * the app as a link ([opened]) or not at all ([resumed]).
     */
    private fun signIn(effect: Effect.SignIn) {
        val context = activity ?: return post(Events.signInDone(null))
        // Only a web address opens in the sign-in session.
        if (!webAddress(effect.url)) return post(Events.signInDone(null))
        if (effect.ephemeral && !ephemeralSupported(context)) {
            if (BuildConfig.DEBUG) Log.w(TAG, "no browser here keeps a sign-in session apart from its own")
            return post(Events.signInDone(null))
        }
        // The address carries the challenge and the state: opened exactly as it came. A
        // session already open is replaced by this one.
        val builder = CustomTabsIntent.Builder().setShowTitle(true)
        if (effect.ephemeral) builder.setEphemeralBrowsingEnabled(true)
        try {
            signingIn = true
            signInCallback = effect.callback
            builder.build().launchUrl(context, Uri.parse(effect.url))
        } catch (_: ActivityNotFoundException) {
            signingIn = false
            post(Events.signInDone(null))
        }
    }

    /** Whether the browser that opens Custom Tabs here can keep a session apart from its own. */
    private fun ephemeralSupported(context: Context): Boolean = try {
        val browser = CustomTabsClient.getPackageName(context, null)
        browser != null && CustomTabsClient.isEphemeralBrowsingSupported(context, browser)
    } catch (_: Exception) {
        false
    }

    private fun haptic(kind: String) {
        val view = activity?.window?.decorView ?: return
        val constant = when (kind) {
            "success" -> if (Build.VERSION.SDK_INT >= 30) HapticFeedbackConstants.CONFIRM else HapticFeedbackConstants.CONTEXT_CLICK
            "warning", "error" -> if (Build.VERSION.SDK_INT >= 30) HapticFeedbackConstants.REJECT else HapticFeedbackConstants.LONG_PRESS
            "selection" -> HapticFeedbackConstants.CLOCK_TICK
            "heavy" -> HapticFeedbackConstants.LONG_PRESS
            "medium" -> HapticFeedbackConstants.CONTEXT_CLICK
            else -> HapticFeedbackConstants.KEYBOARD_TAP
        }
        view.performHapticFeedback(constant)
    }

    @Suppress("DEPRECATION")
    private fun announce(effect: Effect.Announce) {
        val view = activity?.window?.decorView ?: return
        val text = effect.text ?: when (effect.notice) {
            Notice.FieldsNeedAttention -> app.resources.getQuantityString(R.plurals.navree_fields_need_attention, effect.count ?: 1, effect.count ?: 1)
            Notice.Loaded -> app.resources.getQuantityString(R.plurals.navree_rows_loaded, effect.count ?: 0, effect.count ?: 0)
            // Android 13 and later say "copied" themselves.
            Notice.Copied -> if (Build.VERSION.SDK_INT >= 33) null else app.getString(R.string.navree_copied)
            Notice.SavedNotRefreshed -> app.getString(R.string.navree_notice_saved_not_refreshed)
            else -> null
        } ?: return
        view.announceForAccessibility(text)
    }

    private fun copy(text: String, sensitive: Boolean) {
        val clipboard = app.getSystemService(ClipboardManager::class.java) ?: return
        val clip = ClipData.newPlainText("", text)
        if (sensitive) {
            clip.description.extras = PersistableBundle().apply {
                if (Build.VERSION.SDK_INT >= 33) {
                    putBoolean(ClipDescription.EXTRA_IS_SENSITIVE, true)
                } else {
                    putBoolean("android.content.extra.IS_SENSITIVE", true)
                }
            }
        }
        clipboard.setPrimaryClip(clip)
    }

    private companion object {
        const val TAG = "Navree"

        /** The longest the store waits for the cookie store to say its cookies are gone. */
        const val WIPE_WAIT_MS = 5_000L

        /** The longest the web view is waited for to say that what sites stored is gone, before what follows goes on without its word. */
        const val SITES_WAIT_MS = 4_000L

        /** The longest the cookie store is waited for to say its cookies are gone, before what follows goes on without its word. */
        const val COOKIES_WAIT_MS = 4_000L

        /** The longest a go that was told to end is waited for to say so. */
        const val STOP_WAIT_MS = 1_000L
    }
}
