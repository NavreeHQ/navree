package com.navree.client

import android.app.Application
import android.content.Intent
import android.content.res.Configuration
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.unit.dp
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.ViewModelProvider
import com.navree.client.core.CoreJson
import com.navree.client.core.Driver
import com.navree.client.core.Engine
import com.navree.client.core.Events
import com.navree.client.core.Width
import com.navree.client.platform.AndroidEffects
import com.navree.client.platform.MOST_PICTURE_BYTES
import com.navree.client.platform.Origins
import com.navree.client.platform.readCapped
import com.navree.client.ui.NavreeApp
import com.navree.client.ui.rememberWindow
import com.navree.core.Core
import okhttp3.HttpUrl
import okhttp3.HttpUrl.Companion.toHttpUrlOrNull
import okhttp3.Request

/**
 * The runtime core and everything that carries out its effects, kept for as
 * long as the app's task lives: a rotation or a fold makes a new activity,
 * not a new core.
 */
class NavreeSession(
    application: Application,
    val config: NavreeConfig,
    origin: HttpUrl,
    /** The system ended the app and now brings its task back: the stored snapshot is offered to the core. */
    restoring: Boolean = false,
) : AndroidViewModel(application) {
    private val engine = CoreEngine()
    val driver = Driver(engine)

    /** The settings the core was last told of: dark, text size, locale. A change is told once. */
    var environment: Triple<Boolean, Int, String>? = null
    val effects = AndroidEffects(
        application,
        driver,
        origin,
        config.googleClientId,
        config.push,
        origins = Origins(config.origin, config.also, config.loopbackHttp, config.pictureHosts),
        loopbackHttp = config.loopbackHttp,
    )

    init {
        driver.effects = effects
        val device = Device.of(application)
        environment = Triple(device.dark, device.textPercent, device.locale)
        engine.core = Core(coreConfig(config, device, effects.caps, application.packageName).toString().encodeToByteArray())
        // The core checks the snapshot itself and starts plainly when it is not for this launch.
        val snapshot = if (restoring) effects.snapshots.read(SNAPSHOT)?.let { runCatching { CoreJson.parseToJsonElement(it) }.getOrNull() } else null
        if (snapshot == null) Thread { effects.snapshots.clear() }.start()
        driver.send(if (snapshot != null) Events.restore(snapshot) else Events.start)
        effects.askToken()
    }

    /** The app went to the background: what the core keeps for a launch the system may end is stored. */
    fun keep() {
        val snapshot = driver.snapshot() ?: return
        Thread { effects.snapshots.write(SNAPSHOT, snapshot) }.start()
    }

    /**
     * Fetches a picture's bytes, with the session's cookies: only from the
     * app's origins and the picture hosts its configuration names, asking for
     * a picture, and at most [MOST_PICTURE_BYTES]. Null for anything else.
     */
    suspend fun image(url: String): ByteArray? = kotlinx.coroutines.withContext(kotlinx.coroutines.Dispatchers.IO) {
        val target = effects.origins.picture(url) ?: return@withContext null
        try {
            val request = Request.Builder().url(target).header("Accept", "image/*").build()
            effects.http.client.newCall(request).execute().use { response ->
                when {
                    !response.isSuccessful -> null
                    response.body.contentLength() > MOST_PICTURE_BYTES -> null
                    else -> readCapped(response.body.source(), MOST_PICTURE_BYTES)
                }
            }
        } catch (_: Exception) {
            null
        }
    }

    override fun onCleared() {
        engine.core?.close()
        engine.core = null
    }

    private class CoreEngine : Engine {
        var core: Core? = null

        override fun send(event: ByteArray): ByteArray = core?.send(event) ?: ByteArray(0)

        override fun view(query: ByteArray): ByteArray = core?.view(query) ?: ByteArray(0)
    }

    class Factory(
        private val application: Application,
        private val config: NavreeConfig,
        private val origin: HttpUrl,
        private val restoring: Boolean,
    ) : ViewModelProvider.Factory {
        @Suppress("UNCHECKED_CAST")
        override fun <T : androidx.lifecycle.ViewModel> create(modelClass: Class<T>): T = NavreeSession(application, config, origin, restoring) as T
    }

    private companion object {
        const val SNAPSHOT = "snapshot"
    }
}

/**
 * The one activity of a Navree app. An app declares it in its manifest and
 * sets its origin, name and seed colour in resources; there is no code to
 * write. An app that needs more than resources (a push provider, say)
 * subclasses it and overrides [config].
 */
open class NavreeActivity : ComponentActivity() {
    private var session: NavreeSession? = null

    /** What this app is. Read once, when the activity is made. */
    protected open fun config(): NavreeConfig = NavreeConfig.fromResources(this)

    override fun onCreate(savedInstanceState: Bundle?) {
        // The app draws behind the system bars; each screen pads itself by the insets.
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)
        val config = config()
        val origin = config.origin.toHttpUrlOrNull()?.takeIf { Origins(config.origin, loopbackHttp = config.loopbackHttp).usable }
        if (origin == null) {
            setContent { Unusable(config.origin) }
            return
        }
        val session = ViewModelProvider(this, NavreeSession.Factory(application, config, origin, restoring = savedInstanceState != null))[NavreeSession::class.java]
        this.session = session
        session.effects.activity = this
        if (savedInstanceState == null) opened(intent)
        setContent { Host(session) }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        opened(intent)
    }

    override fun onResume() {
        super.onResume()
        session?.let {
            it.effects.activity = this
            it.effects.resumed()
        }
    }

    override fun onStart() {
        super.onStart()
        session?.driver?.send(Events.foreground)
    }

    override fun onStop() {
        session?.let {
            it.driver.send(Events.background)
            it.keep()
            it.effects.stopped()
        }
        super.onStop()
    }

    override fun onDestroy() {
        session?.effects?.let { if (it.activity === this) it.activity = null }
        super.onDestroy()
    }

    /** A link that opened the app, or a notification tapped: handed to the core, which checks it. */
    private fun opened(intent: Intent?) {
        val session = session ?: return
        intent?.getStringExtra(EXTRA_HREF)?.let {
            session.driver.send(Events.notification(it))
            return
        }
        intent?.data?.toString()?.let(session.effects::opened)
    }

    companion object {
        /** The path a notification opens, as the server's push channel puts it in a message's data. */
        const val EXTRA_HREF = "href"
    }
}

@Composable
private fun Host(session: NavreeSession) {
    val window = rememberWindow()
    val configuration = LocalConfiguration.current
    val driver = session.driver
    // The window's width class and the person's settings, as they change.
    LaunchedEffect(window.width) {
        driver.send(
            Events.window(
                when (window.width) {
                    com.navree.client.design.WidthClass.Compact -> Width.Compact
                    com.navree.client.design.WidthClass.Medium -> Width.Medium
                    else -> Width.Expanded
                },
            ),
        )
    }
    val dark = configuration.uiMode and Configuration.UI_MODE_NIGHT_MASK == Configuration.UI_MODE_NIGHT_YES
    val now = Triple(dark, (configuration.fontScale * 100).toInt(), configuration.locales[0].toLanguageTag())
    LaunchedEffect(now) {
        if (session.environment != now) {
            session.environment = now
            driver.send(Events.environment(dark = now.first, textPercent = now.second, locale = now.third))
        }
    }
    NavreeApp(driver, session.config, image = session::image)
}

@Composable
private fun Unusable(origin: String) {
    MaterialTheme {
        Box(Modifier.fillMaxSize().padding(32.dp), contentAlignment = Alignment.Center) {
            Text("This app's origin is not set, or is not HTTPS: $origin")
        }
    }
}
