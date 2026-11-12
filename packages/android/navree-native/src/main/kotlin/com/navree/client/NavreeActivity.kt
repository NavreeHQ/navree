package com.navree.client

import android.content.ActivityNotFoundException
import android.content.ClipData
import android.content.ClipboardManager
import android.content.Intent
import android.net.Uri
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.ViewModelProvider
import androidx.lifecycle.lifecycleScope
import androidx.lifecycle.repeatOnLifecycle
import com.navree.client.platform.HttpsTransport
import com.navree.client.platform.NavreeConfig
import com.navree.client.platform.NavreeStore
import com.navree.client.platform.describe
import com.navree.client.platform.origin
import com.navree.client.runtime.Effect
import com.navree.client.runtime.Event
import com.navree.client.runtime.Load
import com.navree.client.ui.NavreeApp
import kotlinx.coroutines.launch

/**
 * The one activity of a Navree app. An app declares it in its manifest and
 * sets its origin, name and seed colour in resources; there is no code to
 * write. An app that needs more than resources subclasses it and overrides
 * [config].
 */
open class NavreeActivity : ComponentActivity() {
    private var store: NavreeStore? = null

    /** What this app is. Read once, when the activity is made. */
    protected open fun config(): NavreeConfig = NavreeConfig.fromResources(this)

    override fun onCreate(savedInstanceState: Bundle?) {
        // The app draws behind the system bars; each screen pads itself by the insets.
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)
        val config = config()
        val origin = config.origin()
        if (origin == null) {
            setContent {
                MaterialTheme {
                    Box(Modifier.fillMaxSize().padding(32.dp), contentAlignment = Alignment.Center) {
                        Text("This app's origin is not set, or is not HTTPS: ${config.origin}")
                    }
                }
            }
            return
        }
        val app = applicationContext
        val transport = HttpsTransport(origin, describe = { describe(app) })
        val store = ViewModelProvider(this, NavreeStore.Factory(origin, transport))[NavreeStore::class.java]
        this.store = store
        if (store.state.value.shellLoad == Load.Idle) store.dispatch(Event.Started)
        if (savedInstanceState == null) intent?.data?.let { store.dispatch(Event.Opened(it.toString())) }

        lifecycleScope.launch {
            repeatOnLifecycle(Lifecycle.State.STARTED) {
                store.effects.collect(::perform)
            }
        }
        setContent {
            NavreeApp(store, config, image = { url -> (store.transport as? HttpsTransport)?.bytes(url) })
        }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        intent.data?.let { store?.dispatch(Event.Opened(it.toString())) }
    }

    private fun perform(effect: Effect) {
        when (effect) {
            is Effect.OpenOutside -> try {
                startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(effect.url)))
            } catch (_: ActivityNotFoundException) {
                // Nothing on the device opens this kind of address.
            }
            is Effect.Copy -> {
                val clipboard = getSystemService(ClipboardManager::class.java)
                clipboard?.setPrimaryClip(ClipData.newPlainText("", effect.text))
            }
            Effect.Leave -> finish()
            is Effect.Fetch -> Unit
        }
    }
}
