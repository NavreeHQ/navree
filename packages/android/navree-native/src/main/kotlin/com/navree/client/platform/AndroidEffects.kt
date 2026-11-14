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

/**
 * Empties what the system's web views keep for the app: what the page's
 * scripts stored ([WebStorage.deleteAllData]), the resource cache on disk,
 * the back and forward list, the form data and the saved sign-ins for sites.
 * Cookies are the cookie store's ([WebViewCookies.clear]). Call on the main
 * thread.
 */
fun clearWebViews(context: Context) {
    WebStorage.getInstance().deleteAllData()
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
    val cookies = WebViewCookies()

    /** Everything that touches what is kept on the device runs here, in order, one piece at a time. */
    private val storeThread = Executors.newSingleThreadExecutor { work -> Thread(work, "navree-store").apply { isDaemon = true } }
    val state = StoredState { storeThread.execute(it) }
    val http = CoreHttp(cookies, origins, loopbackHttp, state) { post(it) }
    private val streams = CoreStreams(http.client, origins, cookies, state) { post(it) }
    private val cache = SealedCache(app)
    private val timers = HashMap<Long, Runnable>()
    private val push = push
    val devices = DeviceCalls(origin, googleClientId, pushes = push != null, tokenGranted = ::askToken) { post(it) }

    /** What the core keeps for a launch the system ended: sealed, out of backups, gone at sign-out. */
    val snapshots = SealedCache(app, "navree-snapshot")
    private val random = SecureRandom()

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
     * The session ended on this device. In the store's turn, after whatever
     * was asked before: the sealed page cache and the snapshot go; then, on
     * the main thread, the web view's storage, its cache, history, form data
     * and saved sign-ins for sites; then every cookie of the one store the
     * web view shares. Nothing kept for the session after runs before the
     * cookie store says its cookies are gone (or five seconds have passed),
     * and no write of the session that ended runs at all. The push service's
     * token for this install is deleted and a new one asked for.
     */
    private fun forget() {
        state.end { done ->
            cache.clear()
            snapshots.clear()
            main.post {
                try {
                    clearWebViews(app)
                } catch (_: Exception) {
                    // No web view on this device: nothing of it to clear.
                }
                try {
                    cookies.clear {
                        state.always { cookies.flush() }
                        done()
                    }
                } catch (_: Exception) {
                    done()
                }
                main.postDelayed({ done() }, WIPE_WAIT_MS)
            }
        }
        push?.forget { askToken() }
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
    }
}
