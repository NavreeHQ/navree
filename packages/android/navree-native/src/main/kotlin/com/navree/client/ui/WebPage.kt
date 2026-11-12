package com.navree.client.ui

import android.annotation.SuppressLint
import android.graphics.Bitmap
import android.webkit.CookieManager
import android.webkit.WebChromeClient
import android.webkit.WebResourceRequest
import android.webkit.WebSettings
import android.webkit.WebView
import android.webkit.WebViewClient
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.viewinterop.AndroidView
import com.navree.client.runtime.EMBED_HEADER
import com.navree.client.runtime.Origin
import com.navree.client.runtime.Target

/**
 * A page of the app in the system web view, inside native chrome, for a
 * screen the client cannot draw natively yet.
 *
 * - It loads only the app's origin. A link is not followed here: it is handed
 *   to native navigation, which opens it as a screen of its own or, when it
 *   leaves the origin, outside the app.
 * - It shares the session's cookies with the native requests, so the page is
 *   signed in.
 * - It has no bridge to the app: no script interface, no file or content
 *   access.
 */
@SuppressLint("SetJavaScriptEnabled")
@Composable
internal fun WebPage(
    url: String,
    origin: Origin,
    onLink: (String) -> Unit,
    onTitle: (String) -> Unit,
    modifier: Modifier = Modifier,
) {
    var loading by remember { mutableStateOf(true) }
    val link by rememberUpdatedState(onLink)
    val title by rememberUpdatedState(onTitle)
    var view by remember { mutableStateOf<WebView?>(null) }

    Box(modifier.fillMaxSize()) {
        AndroidView(
            modifier = Modifier.fillMaxSize(),
            factory = { context ->
                WebView(context).apply {
                    settings.javaScriptEnabled = true
                    settings.domStorageEnabled = true
                    settings.allowFileAccess = false
                    settings.allowContentAccess = false
                    settings.mixedContentMode = WebSettings.MIXED_CONTENT_NEVER_ALLOW
                    settings.setGeolocationEnabled(false)
                    CookieManager.getInstance().setAcceptCookie(true)
                    webViewClient = object : WebViewClient() {
                        override fun shouldOverrideUrlLoading(view: WebView, request: WebResourceRequest): Boolean {
                            if (!request.isForMainFrame) return false
                            val address = request.url.toString()
                            val inside = origin.resolve(address) is Target.Inside
                            // A redirect that stays on the origin is part of loading the page.
                            if (request.isRedirect && inside) return false
                            // A tap leads somewhere: native navigation decides where. A page that
                            // tries to leave by itself, with no tap, goes nowhere.
                            if (inside || request.hasGesture()) link(address)
                            return true
                        }

                        override fun onPageStarted(view: WebView, url: String?, favicon: Bitmap?) {
                            loading = true
                        }

                        override fun onPageFinished(view: WebView, url: String?) {
                            loading = false
                        }
                    }
                    webChromeClient = object : WebChromeClient() {
                        override fun onReceivedTitle(view: WebView, text: String?) {
                            if (!text.isNullOrBlank() && !text.startsWith("http")) title(text)
                        }
                    }
                    view = this
                }
            },
            update = { web ->
                val address = origin.absolute(url)
                if (web.tag != address) {
                    web.tag = address
                    // Asks for the page without its HTML chrome: the chrome is native.
                    web.loadUrl(address, mapOf(EMBED_HEADER to "1"))
                }
            },
        )
        if (loading) LinearProgressIndicator(Modifier.fillMaxWidth().align(Alignment.TopCenter))
    }

    DisposableEffect(Unit) {
        onDispose {
            view?.apply {
                stopLoading()
                destroy()
            }
        }
    }
}
