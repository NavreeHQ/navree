package com.navree.client

import android.app.UiModeManager
import android.content.Context
import android.content.res.Configuration
import android.os.Build
import com.navree.client.platform.PushProvider
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonArray
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import java.util.TimeZone

/**
 * What an app says about itself: where its server is, what it is called and
 * the colour its palette grows from. An app is this and nothing else; it sets
 * the values in its own resources (see [fromResources]).
 */
data class NavreeConfig(
    /** The one origin the app talks to, such as `https://store.example.in`. */
    val origin: String,
    /** The app's name, shown until the shell brings the server's. */
    val name: String,
    /**
     * The brand's seed colour, as ARGB, or null when the app sets none. The
     * shell's accent takes its place when it arrives.
     */
    val seed: Int?,
    /**
     * Whether the user's wallpaper colours are used. They are when the app
     * sets no accent of its own; an app with an accent opts in here.
     */
    val dynamicColour: Boolean = false,
    /** Whether `http` to this machine is allowed. For development builds only. */
    val loopbackHttp: Boolean = false,
    /** The server's OAuth client id for Google, when the server's call does not carry one. */
    val googleClientId: String? = null,
    /** Where push tokens come from; none means the app receives no notifications. */
    val push: PushProvider? = null,
    /** The addresses that start a sign-in with another site, where a star stands for one segment. */
    val signInPaths: List<String> = listOf("/auth/*/redirect", "/api/auth/social/*/redirect"),
    /** The scheme a sign-in session comes back on, when it is not a link to the origin. */
    val signInCallback: String? = null,
    /**
     * Whether a link from outside the app may open any address of the origin,
     * or only a destination and the addresses the shell's routes name.
     */
    val openAnyLink: Boolean = false,
) {
    companion object {
        /**
         * Reads the configuration from resources: `navree_origin`, `navree_name`,
         * `navree_seed`, `navree_dynamic_colour`, `navree_loopback_http` and
         * `navree_google_client_id`.
         */
        fun fromResources(context: Context): NavreeConfig {
            val resources = context.resources
            val label = context.applicationInfo.loadLabel(context.packageManager).toString()
            return NavreeConfig(
                origin = resources.getString(R.string.navree_origin),
                name = resources.getString(R.string.navree_name).ifEmpty { label },
                // A seed with no alpha is no seed: the library's own value, until an app sets one.
                seed = context.getColor(R.color.navree_seed).takeIf { it ushr 24 != 0 },
                dynamicColour = resources.getBoolean(R.bool.navree_dynamic_colour),
                loopbackHttp = resources.getBoolean(R.bool.navree_loopback_http),
                googleClientId = resources.getString(R.string.navree_google_client_id).ifEmpty { null },
            )
        }
    }
}

/** What the device says about itself on every request, as the protocol's descriptor has it. */
data class Device(
    val os: String,
    val tablet: Boolean,
    val app: String,
    val dark: Boolean,
    val contrast: String,
    val textPercent: Int,
    val locale: String,
    val tz: String,
) {
    companion object {
        fun of(context: Context): Device {
            val configuration = context.resources.configuration
            val night = configuration.uiMode and Configuration.UI_MODE_NIGHT_MASK == Configuration.UI_MODE_NIGHT_YES
            val contrast = if (Build.VERSION.SDK_INT >= 34) context.getSystemService(UiModeManager::class.java)?.contrast ?: 0f else 0f
            val version = runCatching { context.packageManager.getPackageInfo(context.packageName, 0).versionName }.getOrNull()
            return Device(
                os = Build.VERSION.RELEASE ?: Build.VERSION.SDK_INT.toString(),
                tablet = configuration.smallestScreenWidthDp >= 600,
                app = version ?: "0",
                dark = night,
                contrast = when {
                    contrast >= 0.75f -> "high"
                    contrast >= 0.25f -> "medium"
                    else -> "normal"
                },
                textPercent = (configuration.fontScale * 100).toInt(),
                locale = configuration.locales[0].toLanguageTag(),
                tz = TimeZone.getDefault().id,
            )
        }
    }
}

/** The version of this client library, sent as `lib` and in the web view's user agent. */
const val LIBRARY_VERSION = "0.1.0"

/**
 * The kinds and controls this client does not draw yet. A page that holds one
 * is shown whole in the web view, never with a hole.
 */
val UNDRAWABLE: List<String> = listOf("web", "input:colour", "input:file")

/** The core's configuration for this app on this device. */
fun coreConfig(config: NavreeConfig, device: Device, caps: List<String>, appId: String? = null): JsonObject = buildJsonObject {
    put("origin", config.origin.trimEnd('/'))
    // The package name: without it no sign-in is handed over and no push token registered.
    appId?.let { put("app_id", it) }
    if (config.loopbackHttp) put("loopback_http", true)
    put(
        "client",
        buildJsonObject {
            put("platform", "android")
            put("os", device.os)
            put("form", if (device.tablet) "tablet" else "phone")
            put("app", device.app)
            put("lib", LIBRARY_VERSION)
            put("vocab", 0)
            put("scheme", if (device.dark) "dark" else "light")
            put("contrast", device.contrast)
            put("text_percent", device.textPercent)
            put("locale", device.locale)
            put("tz", device.tz)
            put("caps", buildJsonArray { caps.forEach { add(JsonPrimitive(it)) } })
        },
    )
    // Android's back leads to the first destination before it leaves the app.
    put("back_to_first", true)
    if (config.openAnyLink) put("deep_links", "origin")
    put(
        "cache",
        buildJsonObject {
            put("pages", 24)
            put("persist", true)
        },
    )
    put("undrawable", buildJsonArray { UNDRAWABLE.forEach { add(JsonPrimitive(it)) } })
    put("views", buildJsonArray {})
    put(
        "sign_in",
        buildJsonObject {
            put("paths", buildJsonArray { config.signInPaths.forEach { add(JsonPrimitive(it)) } })
            config.signInCallback?.let { put("callback", it) }
            // Google's own sheet signs in on Android; any other provider goes the web's way.
            if ("platform_sign_in" in caps) put("native", buildJsonArray { add(JsonPrimitive("google")) })
        },
    )
}

/** The token a web view adds to its user agent, after a space: who is asking, and with which library. */
fun webViewToken(): String = "NavreeNative/1 (platform=android; lib=$LIBRARY_VERSION)"
