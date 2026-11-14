package com.navree.client.platform

import android.graphics.Bitmap
import android.webkit.RenderProcessGoneDetail
import android.webkit.WebResourceRequest
import android.webkit.WebResourceResponse
import android.webkit.WebView
import android.webkit.WebViewClient
import java.io.ByteArrayInputStream

/** What becomes of a navigation the web view is about to make. */
enum class WebNavigation {
    /** It goes on in the web view. */
    Load,

    /** It is stopped and its address handed to the core, as a link the person tapped. */
    Core,

    /** It is stopped, and nothing else happens. */
    Stop,
}

/** What becomes of a request the web view is about to send. */
enum class WebRequest {
    /** It is sent. */
    Send,

    /** It is not sent, and the page in front stays as it is (an empty answer that shows nothing). */
    KeepPage,

    /** It is not sent; the page is told it failed. */
    Refuse,
}

/**
 * The web view's rules: the page stays on the app's origins, and so does
 * every frame inside it. Plain functions of what the web view reports, so
 * each can be tested without one.
 */
object WebRules {
    /**
     * A navigation the web view reports before it starts it. The page's own
     * navigations on the app's origins are the core's to decide, as link taps
     * are; one that would leave the origins is stopped, and handed to the core
     * only when the person started it. A redirect on the origins is part of
     * loading what was asked for. A frame inside the page goes on only on the
     * app's origins.
     */
    fun navigation(ours: Boolean, mainFrame: Boolean, redirect: Boolean, gesture: Boolean): WebNavigation = when {
        !mainFrame -> if (ours) WebNavigation.Load else WebNavigation.Stop
        ours -> if (redirect) WebNavigation.Load else WebNavigation.Core
        gesture -> WebNavigation.Core
        else -> WebNavigation.Stop
    }

    /**
     * A request the web view is about to send, from any frame, by any method:
     * only to the app's origins. A page or a form posted elsewhere keeps the
     * page in front; anything else elsewhere is refused. A picture or a font
     * written into the page itself (`data:`, `blob:`) is not sent anywhere and
     * is let through, unless it asks for a document, which would be a page
     * that is not the app's in a frame.
     */
    fun request(ours: Boolean, mainFrame: Boolean, scheme: String?, accept: String?): WebRequest = when {
        ours -> WebRequest.Send
        mainFrame -> WebRequest.KeepPage
        (scheme == "data" || scheme == "blob") && !asksForDocument(accept) -> WebRequest.Send
        else -> WebRequest.Refuse
    }

    /** Whether a page that has started loading in the main frame may be shown: only one of the app's origins. */
    fun shown(ours: Boolean, url: String?): Boolean = ours || url == null || url == "about:blank"

    /**
     * What to do after the web view's process ended: the first time the page
     * is loaded again in a new web view; after that the screen shows that it
     * failed, with "try again".
     */
    fun reloadAfterGone(times: Int): Boolean = times <= 1

    private fun asksForDocument(accept: String?): Boolean =
        accept != null && (accept.contains("text/html", ignoreCase = true) || accept.contains("application/xhtml", ignoreCase = true))
}

/**
 * The web view's client for a page of the app: every navigation and every
 * request of every frame is checked against the app's [origins] ([WebRules]).
 * What the web view reports that is not a request lands in the callbacks, on
 * the main thread.
 */
open class OriginWebClient(
    private val origins: Origins,
    /** A navigation the core decides, as a link tapped in the page. */
    private val onLink: (String) -> Unit,
    /** The page started (true) or finished (false) loading. */
    private val onLoading: (Boolean) -> Unit = {},
    /** A page that is not on the app's origins began to load in the main frame, and was stopped. */
    private val onLeft: () -> Unit = {},
    /** The web view's process ended; the web view can no longer be used. */
    private val onGone: (WebView) -> Unit = {},
) : WebViewClient() {
    override fun shouldOverrideUrlLoading(view: WebView, request: WebResourceRequest): Boolean {
        val url = request.url.toString()
        return when (WebRules.navigation(origins.ours(url), request.isForMainFrame, request.isRedirect, request.hasGesture())) {
            WebNavigation.Load -> false
            WebNavigation.Core -> {
                onLink(url)
                true
            }
            WebNavigation.Stop -> true
        }
    }

    // Called on a thread of the web view's own, for every request of every frame, posts included.
    override fun shouldInterceptRequest(view: WebView, request: WebResourceRequest): WebResourceResponse? {
        val url = request.url.toString()
        val accept = request.requestHeaders?.entries?.firstOrNull { it.key.equals("accept", ignoreCase = true) }?.value
        return when (WebRules.request(origins.ours(url), request.isForMainFrame, request.url.scheme?.lowercase(), accept)) {
            WebRequest.Send -> null
            WebRequest.KeepPage -> empty(204, "No Content")
            WebRequest.Refuse -> empty(403, "Forbidden")
        }
    }

    override fun onPageStarted(view: WebView, url: String?, favicon: Bitmap?) {
        // The last word for what the checks before cannot see, such as a redirect of a posted form.
        if (!WebRules.shown(url != null && origins.ours(url), url)) {
            view.stopLoading()
            onLeft()
            return
        }
        onLoading(true)
    }

    override fun onPageFinished(view: WebView, url: String?) {
        onLoading(false)
    }

    override fun onRenderProcessGone(view: WebView, detail: RenderProcessGoneDetail): Boolean {
        onGone(view)
        // Handled: the app goes on; the caller drops this web view.
        return true
    }

    private fun empty(status: Int, reason: String) =
        WebResourceResponse("text/plain", "utf-8", status, reason, emptyMap(), ByteArrayInputStream(ByteArray(0)))
}
