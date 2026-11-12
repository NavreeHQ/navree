package com.navree.client.platform

import android.app.UiModeManager
import android.content.Context
import android.content.res.Configuration
import android.os.Build
import com.navree.client.R
import com.navree.client.runtime.ClientDescriptor
import com.navree.client.runtime.Origin
import java.util.TimeZone

/**
 * What an app says about itself: where its server is, what it is called and
 * the colour its palette grows from. An app is this and nothing else.
 */
data class NavreeConfig(
    /** The one origin the app talks to, such as `https://store.example.in`. */
    val origin: String,
    /** The app's name, shown until the shell brings the server's. */
    val name: String,
    /** The seed colour, as ARGB. The shell's accent takes its place when it arrives. */
    val seed: Int,
    /** Whether the user's wallpaper colours may be used instead, on Android 12 and later. */
    val dynamicColour: Boolean = false,
    /** Whether `http` to this machine is allowed. For development only. */
    val loopbackHttp: Boolean = false,
) {
    companion object {
        /**
         * Reads the configuration from resources: `navree_origin`,
         * `navree_name`, `navree_seed`, `navree_dynamic_colour` and
         * `navree_loopback_http`. An app sets them in its own `res/values`.
         */
        fun fromResources(context: Context): NavreeConfig {
            val resources = context.resources
            val label = context.applicationInfo.loadLabel(context.packageManager).toString()
            return NavreeConfig(
                origin = resources.getString(R.string.navree_origin),
                name = resources.getString(R.string.navree_name).ifEmpty { label },
                seed = context.getColor(R.color.navree_seed),
                dynamicColour = resources.getBoolean(R.bool.navree_dynamic_colour),
                loopbackHttp = resources.getBoolean(R.bool.navree_loopback_http),
            )
        }
    }
}

/** What this device and this app tell the server on every request. */
fun describe(context: Context): ClientDescriptor {
    val configuration = context.resources.configuration
    val night = configuration.uiMode and Configuration.UI_MODE_NIGHT_MASK == Configuration.UI_MODE_NIGHT_YES
    val contrast = if (Build.VERSION.SDK_INT >= 34) {
        context.getSystemService(UiModeManager::class.java)?.contrast ?: 0f
    } else {
        0f
    }
    val version = try {
        context.packageManager.getPackageInfo(context.packageName, 0).versionName
    } catch (_: Exception) {
        null
    }
    return ClientDescriptor(
        os = Build.VERSION.RELEASE ?: Build.VERSION.SDK_INT.toString(),
        form = if (configuration.smallestScreenWidthDp >= 600) "tablet" else "phone",
        app = version ?: "0",
        dark = night,
        highContrast = contrast >= 0.5f,
        textScale = configuration.fontScale,
        locale = configuration.locales[0].toLanguageTag(),
        timeZone = TimeZone.getDefault().id,
    )
}

/** The origin of a configuration, or null when it is not one the client may talk to. */
fun NavreeConfig.origin(): Origin? = Origin.parse(origin, loopbackHttp)
