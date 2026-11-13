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
import androidx.activity.ComponentActivity
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
) : EffectRunner {
    private val app = context.applicationContext
    private val main = Handler(Looper.getMainLooper())
    val cookies = WebViewCookies()
    val http = CoreHttp(cookies) { post(it) }
    private val streams = CoreStreams(http.client) { post(it) }
    private val cache = SealedCache(app)
    private val timers = HashMap<Long, Runnable>()
    val devices = DeviceCalls(origin, googleClientId) { post(it) }
    val pushes: PushRegistration? = push?.let { PushRegistration(http.client, origin, app.packageName, it) }

    /** The activity in front, while there is one. */
    var activity: ComponentActivity? = null

    /** A sign-in session is open in the browser: what comes back to the app ends it. */
    var signingIn = false
        private set

    /** The features this build carries out, by the protocol's names, for `client.caps`. */
    val caps: List<String> = buildList {
        addAll(listOf("clipboard", "open_url", "auth_session"))
        addAll(devices.features)
        if (pushes != null) add("push")
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
            is Effect.CacheRead -> {
                val key = effect.key
                Thread {
                    val value = cache.read(key)
                    post(Events.cache(key, value))
                }.start()
            }
            is Effect.CacheWrite -> {
                val (key, value) = effect.key to effect.value
                Thread { cache.write(key, value) }.start()
            }
            Effect.CacheClear -> Thread { cache.clear() }.start()
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
            is Effect.Unknown -> if (BuildConfig.DEBUG) Log.w(TAG, "an effect this build does not know: ${effect.t}")
        }
    }

    /** The app left the front: streams close (the core opens them again when it comes back). */
    fun stopped() {
        streams.closeAll()
    }

    /**
     * An address the app was opened with. While a sign-in session is open, it
     * is the session coming back; otherwise it is a link to open.
     */
    fun opened(url: String) {
        if (signingIn) {
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

    private fun open(url: String, inside: Boolean) {
        val context = activity ?: return
        val uri = Uri.parse(url)
        try {
            if (inside && (uri.scheme == "https" || uri.scheme == "http")) {
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
     * sign-ins unless the session must share nothing. It comes back to the
     * app as a link ([opened]) or not at all ([resumed]).
     */
    private fun signIn(effect: Effect.SignIn) {
        val context = activity ?: return post(Events.signInDone(null))
        val builder = CustomTabsIntent.Builder().setShowTitle(true)
        if (effect.ephemeral) builder.setEphemeralBrowsingEnabled(true)
        try {
            signingIn = true
            builder.build().launchUrl(context, Uri.parse(effect.url))
        } catch (_: ActivityNotFoundException) {
            signingIn = false
            post(Events.signInDone(null))
        }
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
    }
}
