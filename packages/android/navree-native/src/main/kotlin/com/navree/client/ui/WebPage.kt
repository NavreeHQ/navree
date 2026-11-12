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
import com.navree.client.BuildConfig
import com.navree.client.webViewToken

/**
 * A page of the app's origin that could not be drawn natively, in the
 * system's web view inside the app's own chrome. It shares the native
 * requests' cookies (the system store is the one store), carries the
 * protocol's token at the end of its user agent so the server leaves out its
 * web chrome, and has no bridge to the app: no script interface, no file or
 * content access, no pop-ups, no mixed content.
 *
 * Every link the person taps is handed to the core, which decides where it
 * leads (a native screen, the system browser, nothing); the web view's own
 * navigation is cancelled. Form posts and the page's own requests go through.
 */
@SuppressLint("SetJavaScriptEnabled")
@Composable
internal fun WebPage(
    url: String,
    serial: Long,
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
                WebView.setWebContentsDebuggingEnabled(BuildConfig.DEBUG)
                WebView(context).apply {
                    settings.javaScriptEnabled = true
                    settings.domStorageEnabled = true
                    settings.allowFileAccess = false
                    settings.allowContentAccess = false
                    settings.setSupportMultipleWindows(false)
                    settings.javaScriptCanOpenWindowsAutomatically = false
                    settings.mixedContentMode = WebSettings.MIXED_CONTENT_NEVER_ALLOW
                    settings.setGeolocationEnabled(false)
                    settings.safeBrowsingEnabled = true
                    settings.userAgentString = settings.userAgentString + " " + webViewToken()
                    CookieManager.getInstance().setAcceptCookie(true)
                    webViewClient = object : WebViewClient() {
                        override fun shouldOverrideUrlLoading(view: WebView, request: WebResourceRequest): Boolean {
                            if (!request.isForMainFrame) return false
                            // A redirect is part of loading what was asked for.
                            if (request.isRedirect) return false
                            // Any other navigation of the page is the core's to decide.
                            link(request.url.toString())
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
                val wanted = "$serial $url"
                if (web.tag != wanted) {
                    web.tag = wanted
                    web.loadUrl(url)
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
