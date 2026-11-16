package com.navree.client.platform

import android.graphics.Bitmap
import android.webkit.ServiceWorkerClient
import android.webkit.ServiceWorkerController
import android.webkit.RenderProcessGoneDetail
import android.webkit.WebResourceRequest
import android.webkit.WebResourceResponse
import android.webkit.WebView
import android.webkit.WebViewClient
import androidx.webkit.WebViewCompat
import androidx.webkit.WebViewFeature
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonArray
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
 * The web view's rules: the page itself stays on the app's origins; what it
 * loads inside it (scripts, styles, pictures, frames) comes from the app's
 * origins or from a host the app names for the web view ([Origins.webHost]).
 * Plain functions of what the web view reports, so each can be tested
 * without one.
 */
object WebRules {
    /**
     * A navigation the web view reports before it starts it. The page's own
     * navigations on the app's origins are the core's to decide, as link taps
     * are; one that would leave the origins is stopped, and handed to the core
     * only when the person started it. A redirect on the origins is part of
     * loading what was asked for. A frame inside the page goes on only on the
     * app's origins or a web host.
     */
    fun navigation(ours: Boolean, webHost: Boolean, mainFrame: Boolean, redirect: Boolean, gesture: Boolean): WebNavigation = when {
        !mainFrame -> if (ours || webHost) WebNavigation.Load else WebNavigation.Stop
        ours -> if (redirect) WebNavigation.Load else WebNavigation.Core
        gesture -> WebNavigation.Core
        else -> WebNavigation.Stop
    }

    /**
     * Whether a navigation of the main frame that is not loaded leaves the
     * screen with nothing to show: a redirect away from the origins (of the
     * first load, or of a page posted), which the screen then shows as failed.
     */
    fun leftByRedirect(mainFrame: Boolean, redirect: Boolean, navigation: WebNavigation): Boolean =
        mainFrame && redirect && navigation != WebNavigation.Load

    /**
     * A request the web view is about to send, from any frame, by any method,
     * as far as its first address. The page itself only to the app's
     * origins; a page or a form posted elsewhere keeps the page in front.
     * What a page loads inside it: the app's origins and the web hosts. A
     * picture or a font written into the page itself (`data:`, `blob:`) is
     * not sent anywhere and is let through, unless it asks for a document,
     * which would be a page that is not the app's in a frame.
     */
    fun request(ours: Boolean, webHost: Boolean, mainFrame: Boolean, scheme: String?, accept: String?): WebRequest = when {
        ours -> WebRequest.Send
        mainFrame -> WebRequest.KeepPage
        webHost -> WebRequest.Send
        (scheme == "data" || scheme == "blob") && !asksForDocument(accept) -> WebRequest.Send
        else -> WebRequest.Refuse
    }

    /** Whether a service worker's own request is sent: only to the app's origins and the hosts it names for the web view. */
    fun workerRequest(ours: Boolean, webHost: Boolean): WebRequest = if (ours || webHost) WebRequest.Send else WebRequest.Refuse

    /**
     * What a web page is missing before it may be shown: the rules for a service
     * worker's requests, and the script at the start of every document. A page
     * is shown only when nothing is missing ([guardsInForce]); where the system's
     * web view is too old to take either, a web region is not shown at all and
     * the rest of the app works. Each name says what is not there, for the log.
     */
    fun missingGuards(workers: Boolean, documents: Boolean): List<String> = buildList {
        if (!workers) add("the rules for a service worker's requests are not in force")
        if (!documents) add("the script at the start of every document is not in place")
    }

    /** Whether a web page may be shown: both guards are in force. */
    fun guardsInForce(workers: Boolean, documents: Boolean): Boolean = missingGuards(workers, documents).isEmpty()

    /** Whether a page that has started loading in the main frame may be shown: only one of the app's origins. */
    fun shown(ours: Boolean, url: String?): Boolean = ours || url == null || url == "about:blank"

    /**
     * What to do after the web view's process ended: the first time the page
     * is loaded again in a new web view; after that the screen shows that it
     * failed, with "try again".
     */
    fun reloadAfterGone(times: Int): Boolean = times <= 1

    /**
     * The script that runs at the start of every document in every frame,
     * before the document's own scripts: a document whose origin is not one
     * of [origins] (as `[scheme, host, port]`, a host may start with `*.`) is
     * stopped and emptied before anything of it is drawn or run.
     *
     * A document with no origin of its own (a sandboxed frame) is judged by
     * its address: on the app's origins or a web host's it is left alone,
     * anywhere else it is emptied. An `about:` document (`about:srcdoc`,
     * `about:blank`) holds only what the page around it wrote, and that page
     * passed this check itself, so it is left alone. A `data:` or `blob:`
     * document with no origin is emptied: nothing shows where it came from.
     *
     * At the start of a document its tree may not exist yet, so emptying it
     * takes three steps: `window.stop()`, which aborts the parser and drops
     * what it has not parsed yet; an observer of the document that takes out
     * every node as it is put in, which runs before a script the parser has
     * just put in is prepared, so such a script is no longer in the document
     * and never runs; and the root element taken out if it is there already.
     * A failure of the check itself empties the document too.
     *
     * The same script puts a check in front of the page's own `WebSocket`,
     * `WebSocketStream`, `Worker` and `SharedWorker`, which are not requests the
     * web view's client is asked about: one is made only for an address on the
     * origins (so never from a `blob:` or `data:` address). What the page hands
     * over is turned into a string once; that string, resolved against the
     * document's real base, is what is checked, and it is what the real
     * constructor gets, so an object whose conversion answers differently the
     * second time gets nowhere. Everything the check calls is taken when the
     * script starts, before any script of the page, and is called only through
     * the saved `Reflect.apply` and `Reflect.construct`: a global, a method or a
     * getter the page replaces afterwards is never used. It holds for the scripts
     * of the document and of its frames that are given a document, not the empty
     * first document of a frame the page makes and has not navigated, whose
     * window the script never ran in. It does not reach into a worker: a script
     * that already runs in a worker from one of the app's addresses opens what
     * it likes, and so does a service worker.
     */
    fun documentScript(origins: List<Triple<String, String, Int>>): String {
        val rules = buildJsonArray {
            for ((scheme, host, port) in origins) {
                add(
                    buildJsonArray {
                        add(JsonPrimitive(scheme))
                        add(JsonPrimitive(host))
                        add(JsonPrimitive(port))
                    },
                )
            }
        }
        return "(function(){" +
            "function stop(){try{window.stop()}catch(e){}" +
            "try{new MutationObserver(function(l){for(var i=0;i<l.length;i++){var a=l[i].addedNodes;for(var j=0;j<a.length;j++){var n=a[j];if(n.parentNode)n.parentNode.removeChild(n)}}})" +
            ".observe(document,{childList:true,subtree:true})}catch(e){}" +
            "var d=document.documentElement;if(d)d.remove()}" +
            "var r=$rules;" +
            // Everything the check calls is taken here, before any script of the page runs, and kept in this
            // closure; from then on it is called only through the saved Reflect.apply and Reflect.construct.
            // Nothing is looked up when a page's script calls a wrapper, so a global, a prototype's method or a
            // getter the page replaces afterwards is never what the check uses. The check and the wrappers are
            // one `try`: a failure of either empties the document.
            "try{var A=Reflect.apply,K=Reflect.construct,S=String,U=URL,E=DOMException,D=document,W=self,F=Array.from,Y=Array," +
            "G=function(o,p){return Object.getOwnPropertyDescriptor(o,p).get}," +
            "H=G(U.prototype,'href'),P=G(U.prototype,'protocol'),M=G(U.prototype,'hostname'),O=G(U.prototype,'port'),B=G(Node.prototype,'baseURI'),Z=S.prototype.endsWith;" +
            "if(!H||!P||!M||!O||!B||!Z)throw 0;" +
            "for(var i=0;i<r.length;i++){var n=r[i][1];r[i][3]=n.indexOf('*.')===0?n.slice(1):''}" +
            "var at=function(a){var u=K(U,[a]),s=A(P,u,[]);if(s==='ws:')s='http:';else if(s==='wss:')s='https:';if(s!=='http:'&&s!=='https:')return false;" +
            "var h=A(M,u,[]),p=A(O,u,[]);p=p===''?(s==='http:'?80:443):+p;" +
            "for(var i=0;i<r.length;i++){var x=r[i];if(x[0]+':'===s&&x[2]===p&&(x[3]===''?x[1]===h:h.length>x[3].length&&A(Z,h,[x[3]])))return true}" +
            "return false};" +
            // Neither a socket nor a worker is a request the web view's client is asked about, and a
            // document-start script does not run inside a worker: the page's own scripts may open a
            // socket or start a worker only where its loads may go (never from a `blob:` or `data:`
            // address). What the page hands over is turned into a string once, that string is resolved against
            // the document's real base and checked, and it is what the real constructor gets; the options are
            // copied into objects of the script's own.
            "var cp=function(q,s){if(q===null||typeof q!=='object'&&typeof q!=='function')return s===1?S(q):q;if(s===1)return A(F,Y,[q,S]);" +
            "if(s===2){var v=q.protocols;return{protocols:v===undefined?v:A(F,Y,[v,S]),signal:q.signal}}return{name:q.name,type:q.type,credentials:q.credentials}};" +
            "var wrap=function(k,s){var C=W[k];if(!C)return;var N=function(u,q){var x=S(u),a=A(H,K(U,[x,A(B,D,[])]),[]);" +
            "if(!at(a))throw K(E,['Not allowed','SecurityError']);return q===undefined?K(C,[x]):K(C,[x,cp(q,s)])};" +
            "N.prototype=C.prototype;C.prototype.constructor=N;var c=['CONNECTING','OPEN','CLOSING','CLOSED'];for(var j=0;j<c.length;j++)if(c[j] in C)N[c[j]]=C[c[j]];W[k]=N};" +
            "wrap('WebSocket',1);wrap('WebSocketStream',2);wrap('Worker');wrap('SharedWorker');" +
            "var o=W.origin;if(o!=='null'){if(!at(o))stop();return}" +
            "var l=location.href;if(l.indexOf('about:')===0)return;if(!at(l))stop()}catch(e){stop()}})();"
    }

    private fun asksForDocument(accept: String?): Boolean =
        accept != null && (accept.contains("text/html", ignoreCase = true) || accept.contains("application/xhtml", ignoreCase = true))
}

/**
 * Adds [WebRules.documentScript] to a web view, to run at the start of every
 * document in every frame, the one check that also sees a document reached
 * by a redirect the web view's callbacks are never told of (a frame's, a
 * posted form's). Call before the first load. Where the system's web view
 * lacks the feature (`WebViewFeature.DOCUMENT_START_SCRIPT`), nothing is
 * added, and a web page is not shown without it ([WebRules.guardsInForce]).
 * That is said once here in the log, and every time by the page. Answers
 * whether the script was added.
 */
fun guardDocuments(view: WebView, origins: Origins): Boolean = guardDocuments(view, origins, ClientLog.told)

internal fun guardDocuments(view: WebView, origins: Origins, told: Told): Boolean {
    if (!WebViewFeature.isFeatureSupported(WebViewFeature.DOCUMENT_START_SCRIPT)) {
        told.once(
            "documents",
            "this system's web view cannot run a script at the start of a document: a web page is not shown",
        )
        return false
    }
    return try {
        WebViewCompat.addDocumentStartJavaScript(view, WebRules.documentScript(origins.documentOrigins()), setOf("*"))
        true
    } catch (e: Exception) {
        told.once("documents", "the script at the start of a document could not be added ($e): a web page is not shown")
        false
    }
}

/**
 * Puts the origin rules on the requests of service workers too: a page of the
 * app may register one, and its requests do not pass through the web view's
 * client. A worker's request to any origin but the app's (and its named web
 * hosts) is answered 403. The setting is the process's. Throws when the
 * system's web view cannot take it.
 */
internal fun setServiceWorkerRules(origins: Origins) {
    ServiceWorkerController.getInstance().setServiceWorkerClient(
        object : ServiceWorkerClient() {
            override fun shouldInterceptRequest(request: WebResourceRequest): WebResourceResponse? {
                val url = request.url.toString()
                return when (WebRules.workerRequest(origins.ours(url), origins.webHost(url))) {
                    WebRequest.Send -> null
                    else -> WebResourceResponse("text/plain", "utf-8", 403, "Forbidden", emptyMap(), ByteArrayInputStream(ByteArray(0)))
                }
            }
        },
    )
}

/** [setServiceWorkerRules], answering whether it was made. */
fun guardWorkers(origins: Origins): Boolean = try {
    setServiceWorkerRules(origins)
    true
} catch (_: Throwable) {
    false
}

/** What the client could not do, said once for each thing in the log: a web page made again does not say it again. */
internal class Told(private val log: (String) -> Unit) {
    private val said = HashSet<String>()

    fun once(what: String, why: String) {
        val first = synchronized(said) { said.add(what) }
        if (first) log(why)
    }

    /** Forgets that [what] was said, so that a failure of it is told again. */
    fun again(what: String) {
        synchronized(said) { said.remove(what) }
    }
}

internal object ClientLog {
    val told = Told { say(it) }

    /** A line in the log, every time it is asked for. */
    fun say(text: String) {
        android.util.Log.w("Navree", text)
    }
}

/**
 * The guard on service workers, as a state that can be tested: [make] sets the
 * rules, and the guard counts as made only when it did. Every call of [ensure]
 * until then tries again; a failure is told once, and so is a call before the
 * guard was armed. A web page is not shown while it is not in force.
 */
internal class WorkerRules(private val make: (Origins) -> Unit, private val told: Told) {
    private var origins: Origins? = null
    private var made = false

    /** Says which origins the guard is for; a guard made before is made again for them. */
    fun arm(origins: Origins) {
        this.origins = origins
        made = false
        told.again("workers")
    }

    /**
     * Makes the guard when the main thread has nothing to do: [register] is the
     * way to ask for that (a handler the main thread's queue calls when idle); the
     * handler answers false, so it is not called again.
     */
    fun whenIdle(register: (() -> Boolean) -> Unit) {
        register {
            ensure()
            false
        }
    }

    /** Whether the rules are in force: made now if they are not yet. False when nothing was armed or the system's web view cannot take them. */
    fun ensure(): Boolean {
        val wanted = origins
        if (wanted == null) {
            told.once("workers-unarmed", "the guard on service workers was asked for before it was told which origins it is for: it is not in force")
            return false
        }
        if (made) return true
        try {
            make(wanted)
            made = true
        } catch (e: Throwable) {
            told.once("workers", "the rules for a service worker's requests could not be set ($e): a web page is not shown; every web page made tries again")
        }
        return made
    }
}

/**
 * The guard on service workers. Setting the service worker controller's
 * client is a call into the web view's engine, and it is not made while the
 * app starts: it is made on the main thread when the main thread is idle after
 * the first screen is drawn, and in any case before a web view is made (a page
 * cannot register a worker before it is in a web view). That moves this call
 * only. The engine itself is started by the first native request, when the
 * cookie jar ([WebViewCookies]) asks the web view's cookie store. Where the
 * guard could not be made, each web page made tries again, and the first
 * failure is in the log.
 */
object WorkerGuard {
    private val rules = WorkerRules(::setServiceWorkerRules, ClientLog.told)

    /** Says which origins the guard is for. Call on the main thread. */
    fun arm(origins: Origins) = rules.arm(origins)

    /** Makes the guard the next time the main thread has nothing to do. Call on the main thread. */
    fun whenIdle() {
        rules.whenIdle { handler -> android.os.Looper.myQueue().addIdleHandler { handler() } }
    }

    /** Makes the guard now if it is not made yet, and says whether it is in force. Call on the main thread. */
    fun ensure(): Boolean = rules.ensure()
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
    /** The main frame was taken off the app's origins (a redirect, a page that began to load elsewhere) and stopped. */
    private val onLeft: () -> Unit = {},
    /** The web view's process ended; the web view can no longer be used. */
    private val onGone: (WebView) -> Unit = {},
) : WebViewClient() {
    override fun shouldOverrideUrlLoading(view: WebView, request: WebResourceRequest): Boolean {
        val url = request.url.toString()
        val navigation = WebRules.navigation(origins.ours(url), origins.webHost(url), request.isForMainFrame, request.isRedirect, request.hasGesture())
        if (WebRules.leftByRedirect(request.isForMainFrame, request.isRedirect, navigation)) onLeft()
        return when (navigation) {
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
        return when (WebRules.request(origins.ours(url), origins.webHost(url), request.isForMainFrame, request.url.scheme?.lowercase(), accept)) {
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
