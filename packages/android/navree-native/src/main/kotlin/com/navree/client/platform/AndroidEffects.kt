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
import kotlinx.serialization.json.JsonObject
import okhttp3.HttpUrl
import java.security.SecureRandom
import java.util.concurrent.Executors
import java.util.concurrent.RejectedExecutionException
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
    val cookies = WebViewCookies()

    init {
        // Read here what an earlier run of the app still owes of the end of a session; it is done below, once everything it needs is made.
        SiteWipe.arm(app, origins)
    }

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

    init {
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
            forget()
        } else if (SiteWipe.owes) {
            // Only what pages stored at the origins is left of it: no web page is shown before that is gone.
            main.post { SiteWipe.finish() }
        }
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

    override fun run(effect: Effect) {
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
            Effect.SessionEnding -> ending()
            Effect.ForgetSession -> forget()
            is Effect.Unknown -> if (BuildConfig.DEBUG) Log.w(TAG, "an effect this build does not know: ${effect.t}")
        }
    }

    /**
     * Asks the push service for this install's token and hands it to the
     * core, which registers it with the server. At every launch, and when the
     * person allows notifications.
     */
    fun askToken() {
        val push = push ?: return
        PushTokens.listener = { token -> post(Events.pushToken(token)) }
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
     * ([SiteWipe.documents]). Nothing kept for the session after runs before
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
     */
    private fun end() {
        // Owed from this moment and written down, whatever the store is still busy with: a page of the next session is not
        // shown over what this one's pages left, and a start of the app after a run that ended here does all of it.
        val debt = SiteWipe.owe()
        state.end { finished ->
            // The time limit is armed before anything it guards: whatever happens below, the queue goes on.
            endWithLimit(finished, arm = { release -> main.postDelayed({ release() }, WIPE_WAIT_MS) }) { done -> wipe(debt, done) }
        }
    }

    /** The core said the session is ending and has not yet said it is forgotten: the end of the session is begun ([ending]). */
    private var begun = false

    /**
     * The core says the session is ending (`session_ending`), in the update
     * in which it asks the server to forget this device's push token: the end
     * of the session begins now ([end]) and does not wait for that answer.
     * The request for the removal stands before this in the same update, so
     * the store has given it its cookies before the turn of the end of the
     * session comes.
     */
    private fun ending() {
        begun = true
        end()
    }

    /**
     * The core says the session is forgotten (`forget_session`). Where it
     * said before that the session was ending, the end of the session is
     * begun or done and nothing of it is done twice; where it did not (no
     * push token to remove), it begins here. The push service's token for
     * this install is deleted and a new one asked for.
     */
    private fun forget() {
        if (begun) begun = false else end()
        push?.forget { askToken() }
    }

    /**
     * The steps of [end], in order, for the end of a session numbered [debt]
     * ([SiteWipe.owe]): what it says to the mark it says for that end only, so
     * a later end of a session is not called done by this one. [done] when
     * the cookie store says its cookies are gone and the web view that
     * nothing of its pages is left.
     */
    private fun wipe(debt: Long, done: () -> Unit) {
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
            try {
                // Every notification of the app, also those the push service drew itself.
                NotificationManagerCompat.from(app).cancelAll()
            } catch (_: Throwable) {
                // Nothing shown.
            }
            val last = { left: Set<String>? ->
                thread(name = "navree-web-wipe", isDaemon = true) {
                    try {
                        SiteWipe.over(left, debt)
                    } finally {
                        done()
                    }
                }
                Unit
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
                            if (!flushed || !cookiesGone.get()) {
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
                        if (narrowed) main.post { SiteWipe.finish() }
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

    /** The activity is in front again. A sign-in session that brought nothing back was given up. */
    fun resumed() {
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
    }
}
