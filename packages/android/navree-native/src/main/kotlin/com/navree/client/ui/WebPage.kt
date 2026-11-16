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
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.viewinterop.AndroidView
import com.navree.client.BuildConfig
import com.navree.client.core.Failure
import androidx.compose.runtime.LaunchedEffect
import com.navree.client.platform.ClientLog
import com.navree.client.platform.OriginWebClient
import com.navree.client.platform.Origins
import com.navree.client.platform.SiteWipe
import com.navree.client.platform.WebRules
import com.navree.client.platform.WorkerGuard
import com.navree.client.platform.guardDocuments
import com.navree.client.webViewToken

/** The tag of a web view whose script at the start of every document could not be added: it loads nothing. */
private const val NOT_GUARDED = "not guarded"

/**
 * What a web page is shown with, as the answers [WebPage] decides by: whether
 * the rules for a service worker's requests are in force, whether the script
 * at the start of every document was added to a web view, and whether the web
 * view holds nothing of a session that ended. The app has only the real ones;
 * a test gives answers of its own, to see the page refused when one is missing.
 */
internal class WebGuards(
    val workers: () -> Boolean = { WorkerGuard.ensure() },
    val documents: (WebView, Origins) -> Boolean = { view, origins -> guardDocuments(view, origins) },
    val clean: () -> Boolean = { SiteWipe.clean() },
)

/** The guards [WebPage] asks: the real ones unless a test provides others. */
internal val LocalWebGuards = staticCompositionLocalOf { WebGuards() }

/**
 * A page of the app's origin that could not be drawn natively, in the
 * system's web view inside the app's own chrome. It shares the native
 * requests' cookies (the system store is the one store), carries the
 * protocol's token at the end of its user agent so the server leaves out its
 * web chrome, and has no bridge to the app: no script interface, no file or
 * content access, no pop-ups, no mixed content. The one object a document of
 * the app is given says only where the document is, and what is posted
 * through it is not read ([guardDocuments]).
 *
 * The page stays on the app's origins, and what it loads inside it is asked
 * for only from them or from the app's web hosts, by the first address of each
 * request ([OriginWebClient], and a check at the start of every document,
 * [guardDocuments]; where a redirect then leads is the page's own policy's to
 * hold): a link the person taps is
 * handed to the core, which decides where it leads (a native screen, the
 * system browser, nothing); any other way of leaving is stopped, and a page
 * taken off the origins ends as a failed screen. The page's title reaches the
 * app bar only from a page on the app's origins. When the web view's process
 * ends, the page is loaded again once in a new web view, and after that the
 * screen says it failed, with "try again".
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
    val guards = LocalWebGuards.current
    var loading by remember { mutableStateOf(true) }
    val link by rememberUpdatedState(onLink)
    val title by rememberUpdatedState(onTitle)
    // A new web view for each attempt: one whose process ended can no longer be used.
    var attempt by remember { mutableIntStateOf(0) }
    var gone by remember { mutableIntStateOf(0) }
    var failed by remember(url, serial) { mutableStateOf(!origins.ours(url)) }
    // A page is not shown without its guards. The rules for a service worker's requests are asked for again
    // every time a page is made (and by "try again"); the script at the start of every document is added to
    // the web view itself, so its answer comes from the web view made for this attempt.
    val workers = remember(url, serial, attempt) { guards.workers() }
    var documents by remember(url, serial, attempt) { mutableStateOf(true) }
    // Nor while the web view still holds what the pages of a session that ended stored: asked again each time, and asking sets it going.
    val clean = remember(url, serial, attempt) { guards.clean() }
    val missing = WebRules.missingGuards(workers, documents, clean)
    LaunchedEffect(url, serial, attempt, missing) {
        if (missing.isNotEmpty()) ClientLog.say("a web page was not shown: " + missing.joinToString("; "))
    }

    Box(modifier.fillMaxSize()) {
        if (failed || missing.isNotEmpty()) {
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
                            // Cookies of a frame from another site are refused (the system's default, said here).
                            CookieManager.getInstance().setAcceptThirdPartyCookies(this, false)
                            // Before the first load, so it runs in every document of every frame. Without it
                            // the page is not loaded: `update` below does not load a web view that has this tag.
                            if (!guards.documents(this, origins)) tag = NOT_GUARDED
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
                        if (web.tag === NOT_GUARDED) {
                            documents = false
                        } else if (web.tag != wanted && origins.ours(url)) {
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
