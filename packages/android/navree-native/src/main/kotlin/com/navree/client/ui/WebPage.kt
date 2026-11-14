package com.navree.client.ui

import android.annotation.SuppressLint
import android.webkit.CookieManager
import android.webkit.WebChromeClient
import android.webkit.WebSettings
import android.webkit.WebView
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.viewinterop.AndroidView
import com.navree.client.BuildConfig
import com.navree.client.core.Failure
import com.navree.client.platform.OriginWebClient
import com.navree.client.platform.Origins
import com.navree.client.platform.WebRules
import com.navree.client.webViewToken

/**
 * A page of the app's origin that could not be drawn natively, in the
 * system's web view inside the app's own chrome. It shares the native
 * requests' cookies (the system store is the one store), carries the
 * protocol's token at the end of its user agent so the server leaves out its
 * web chrome, and has no bridge to the app: no script interface, no file or
 * content access, no pop-ups, no mixed content.
 *
 * The page, and every frame in it, stays on the app's origins
 * ([OriginWebClient]): a link the person taps is handed to the core, which
 * decides where it leads (a native screen, the system browser, nothing); any
 * other way of leaving is stopped. The page's title reaches the app bar only
 * from a page on the app's origins. When the web view's process ends, the page
 * is loaded again once in a new web view, and after that the screen says it
 * failed, with "try again".
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
    val origins = LocalUi.current.origins
    var loading by remember { mutableStateOf(true) }
    val link by rememberUpdatedState(onLink)
    val title by rememberUpdatedState(onTitle)
    // A new web view for each attempt: one whose process ended can no longer be used.
    var attempt by remember { mutableIntStateOf(0) }
    var gone by remember { mutableIntStateOf(0) }
    var failed by remember(url, serial) { mutableStateOf(!origins.ours(url)) }

    Box(modifier.fillMaxSize()) {
        if (failed) {
            Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                FailureState(Failure.Protocol) {
                    gone = 0
                    failed = !origins.ours(url)
                    attempt++
                }
            }
        } else {
            key(attempt) {
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
                            webViewClient = OriginWebClient(
                                origins,
                                onLink = { link(it) },
                                onLoading = { loading = it },
                                onLeft = {
                                    loading = false
                                    failed = true
                                },
                                onGone = {
                                    gone++
                                    if (WebRules.reloadAfterGone(gone)) attempt++ else failed = true
                                },
                            )
                            webChromeClient = object : WebChromeClient() {
                                override fun onReceivedTitle(view: WebView, text: String?) {
                                    val from = view.url
                                    if (from != null && origins.ours(from) && !text.isNullOrBlank() && !text.startsWith("http")) title(text)
                                }
                            }
                        }
                    },
                    update = { web ->
                        val wanted = "$serial $url"
                        if (web.tag != wanted && origins.ours(url)) {
                            web.tag = wanted
                            web.loadUrl(url)
                        }
                    },
                    onRelease = { web ->
                        web.stopLoading()
                        web.destroy()
                    },
                )
            }
            if (loading) LinearProgressIndicator(Modifier.fillMaxWidth().align(Alignment.TopCenter))
        }
    }
}
