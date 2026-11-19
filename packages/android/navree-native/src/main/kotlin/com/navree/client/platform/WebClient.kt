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
import java.io.IOException
import java.io.InputStream

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

/** What becomes of a request a service worker makes. */
enum class WorkerRequest {
    /** It is sent. */
    Send,

    /** It is not sent, and the worker is answered that it is refused (403). */
    Refuse,

    /**
     * It is not sent and not answered either: to the worker it is a request
     * that could not be made, as when there is no network. A worker that
     * keeps what it could not send, to send it again, keeps it.
     */
    Fail,
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

    /**
     * What becomes of a service worker's own request: it goes only to the
     * app's origins and the hosts it names for the web view, and there only
     * as what is owed of what a session's pages left allows ([owed]:
     * [SiteWipe.worker]). A worker which that deletion started is the worker
     * of the person who left; what it sends would go with the cookies of
     * whoever is signed in now.
     */
    fun workerRequest(ours: Boolean, webHost: Boolean, owed: WorkerRequest = WorkerRequest.Send): WorkerRequest = if (ours || webHost) owed else WorkerRequest.Refuse

    /**
     * Where a service worker's request says it is from. [said] is whether it
     * says at all, and [address] what it names: null where it says so and
     * names no address, which is an `Origin` of `null` (what the web engine
     * writes for a request whose referrer was withheld) and so no origin of
     * the app.
     */
    data class From(val said: Boolean, val address: String?)

    /**
     * Where a service worker's request says it is from ([From]): by its
     * `Referer`, which for a worker's own request is the worker's script or
     * the page it works for, else by its `Origin`. A request that carries
     * neither says nothing. It is the web view that writes these, not the
     * worker's script; the script can only have them left out.
     */
    fun workerFrom(headers: Map<String, String>?): From {
        fun header(name: String) = headers?.entries?.firstOrNull { it.key.equals(name, ignoreCase = true) }?.value?.trim()?.takeIf { it.isNotEmpty() }
        header("referer")?.let { return From(true, it) }
        val origin = header("origin") ?: return From(false, null)
        return From(true, origin.takeIf { it != "null" })
    }

    /**
     * What a web page is missing before it may be shown: the rules for a service
     * worker's requests, the script at the start of every document, and a web
     * view that holds nothing of a session that ended on the page's origin
     * ([clean]: [SiteWipe.shows]). A
     * page is shown only when nothing is missing ([guardsInForce]); where the
     * system's web view is too old to take either guard, a web region is not
     * shown at all and the rest of the app works. Each name says what is not
     * there, for the log.
     */
    fun missingGuards(workers: Boolean, documents: Boolean, clean: Boolean = true): List<String> = buildList {
        if (!workers) add("the rules for a service worker's requests are not in force")
        if (!documents) add("the script at the start of every document is not in place")
        if (!clean) add("what the pages of a session that ended stored in the web view is not gone yet")
    }

    /** Whether a web page may be shown: both guards are in force and nothing of an ended session is left to take away. */
    fun guardsInForce(workers: Boolean, documents: Boolean, clean: Boolean = true): Boolean = missingGuards(workers, documents, clean).isEmpty()

    /** Whether a request asks for a document: the page itself, or what a frame is filled with. */
    fun document(mainFrame: Boolean, accept: String?): Boolean = mainFrame || asksForDocument(accept)

    /** Whether a page that has started loading in the main frame may be shown: only one of the app's origins. */
    fun shown(ours: Boolean, url: String?): Boolean = ours || url == null || url == "about:blank"

    /**
     * What to do after the web view's process ended: the first time the page
     * is loaded again in a new web view; after that the screen shows that it
     * failed, with "try again".
     */
    fun reloadAfterGone(times: Int): Boolean = times <= 1

    /** The name of the object a document of the app says where it is through ([guardDocuments]). */
    const val SEEN = "navreeSeen"

    /**
     * The origins a document may have ([Origins.documentOrigins]) as the
     * rules a message listener of the web view takes: `scheme://host`, with
     * the port where it is not the scheme's own; a host may start with `*.`.
     */
    fun listenerRules(origins: List<Triple<String, String, Int>>): Set<String> =
        origins.mapTo(LinkedHashSet()) { (scheme, host, port) -> "$scheme://$host" + if (port == (if (scheme == "http") 80 else 443)) "" else ":$port" }

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
     * A document that is left alone says where it is: it posts an empty
     * message through the object named [SEEN], which the web view gives only
     * to the app's origins, and the web view itself names the origin the
     * message came from. That is how the client knows of an origin a frame or
     * a page reached through a redirect it was never asked about. A document
     * that fails to say it is emptied, as by any other failure of the script.
     *
     * The same script puts a check in front of the page's own `WebSocket`,
     * `WebSocketStream`, `Worker` and `SharedWorker`, which are not requests the
     * web view's client is asked about: one is made only for an address on the
     * [origins], the app's own and its web hosts' (so never from a `blob:` or
     * `data:` address). What the page hands over is read once, before the
     * check, into copies of the script's own: the address turned into a
     * string, a list of protocols into an array of strings, a worker's options
     * into strings. The address is then resolved against the document's base
     * as it is at that moment, that absolute address is what is checked, and
     * the real constructor gets that same absolute address and the copies. So
     * an object whose conversion answers differently the second time gets
     * nowhere, and neither does a base element the page's code puts into the
     * document while its options are read or after. Everything the check
     * calls is taken when the script starts, before any script of the page,
     * and is called only through
     * the saved `Reflect.apply` and `Reflect.construct`: a global, a method or a
     * getter the page replaces afterwards is never used. (In a frame whose
     * first, empty window is kept for a document of the same origin, what the
     * page replaced in that window beforehand is what is taken: no more than
     * the empty frame gave it already.) It holds for the scripts
     * of the document and of its frames that are given a document, not the empty
     * first document of a frame the page makes and has not navigated, whose
     * window the script never ran in. It does not reach into a worker: a script
     * that already runs in a worker from one of the app's addresses opens what
     * it likes, and so does a service worker.
     *
     * It also puts capture-phase listeners on `window` for `dragover`, `drop`
     * and `paste` that cancel any drop or clipboard paste carrying files while
     * leaving plain-text and HTML paste untouched, so files enter a document
     * only through the web view's checked file chooser.
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
            "G=function(o,p){var d=o&&Object.getOwnPropertyDescriptor(o,p);return d&&d.get},Q=function(c){return c&&c.prototype}," +
            "H=G(U.prototype,'href'),P=G(U.prototype,'protocol'),M=G(U.prototype,'hostname'),O=G(U.prototype,'port'),B=G(Node.prototype,'baseURI'),Z=S.prototype.endsWith," +
            "IP=Object.prototype.isPrototypeOf,IX=Y.prototype.indexOf," +
            "L=EventTarget.prototype.addEventListener,PD=Event.prototype.preventDefault,SP=Event.prototype.stopImmediatePropagation," +
            "DP=Q(W.DragEvent),DG=G(DP,'dataTransfer'),CP=Q(W.ClipboardEvent),CG=G(CP,'clipboardData')," +
            "DT=Q(W.DataTransfer),DF=G(DT,'files'),DI=G(DT,'items'),DTY=G(DT,'types')," +
            "FL=G(Q(W.FileList),'length'),IL=G(Q(W.DataTransferItemList),'length'),IK=G(Q(W.DataTransferItem),'kind');" +
            "if(!H||!P||!M||!O||!B||!Z||!L||!PD||!SP)throw 0;" +
            "for(var i=0;i<r.length;i++){var n=r[i][1];r[i][3]=n.indexOf('*.')===0?n.slice(1):''}" +
            "var at=function(a){var u=K(U,[a]),s=A(P,u,[]);if(s==='ws:')s='http:';else if(s==='wss:')s='https:';if(s!=='http:'&&s!=='https:')return false;" +
            "var h=A(M,u,[]),p=A(O,u,[]);p=p===''?(s==='http:'?80:443):+p;" +
            "for(var i=0;i<r.length;i++){var x=r[i];if(x[0]+':'===s&&x[2]===p&&(x[3]===''?x[1]===h:h.length>x[3].length&&A(Z,h,[x[3]])))return true}" +
            "return false};" +
            // Neither a socket nor a worker is a request the web view's client is asked about, and a
            // document-start script does not run inside a worker: the page's own scripts may open a
            // socket or start a worker only where its loads may go (never from a `blob:` or `data:`
            // address). Everything of the page's that can run while one is made is read once, first: the address
            // turned into a string, a list of protocols and the fields of the options copied into strings and
            // objects of the script's own. Then the address is resolved against the document's base as it is at
            // that moment, that absolute address is checked, and the real constructor gets it and the copies:
            // nothing the page's code does afterwards (a base element it puts in, a list's iterator it replaced)
            // changes where the socket or the worker goes.
            "var T=function(v){return v===undefined?v:S(v)};" +
            "var cp=function(q,s){if(q===null||typeof q!=='object'&&typeof q!=='function')return s===1?S(q):q;if(s===1)return A(F,Y,[q,S]);" +
            "if(s===2){var v=q.protocols;return{protocols:v===undefined?v:A(F,Y,[v,S]),signal:q.signal}}return{name:T(q.name),type:T(q.type),credentials:T(q.credentials)}};" +
            "var wrap=function(k,s){var C=W[k];if(!C)return;var N=function(u,q){var x=S(u),o=q===undefined?q:cp(q,s),a=A(H,K(U,[x,A(B,D,[])]),[]);" +
            "if(!at(a))throw K(E,['Not allowed','SecurityError']);return o===undefined?K(C,[a]):K(C,[a,o])};" +
            "N.prototype=C.prototype;C.prototype.constructor=N;var c=['CONNECTING','OPEN','CLOSING','CLOSED'];for(var j=0;j<c.length;j++)if(c[j] in C)N[c[j]]=C[c[j]];W[k]=N};" +
            "wrap('WebSocket',1);wrap('WebSocketStream',2);wrap('Worker');wrap('SharedWorker');" +
            "var hf=function(t){if(!t)return false;var f=DF&&A(DF,t,[]);if(f&&FL&&A(FL,f,[])>0)return true;" +
            "var m=DI&&A(DI,t,[]);if(m&&IL&&IK)for(var i=0,n=A(IL,m,[]);i<n;i++)if(m[i]&&A(IK,m[i],[])==='file')return true;" +
            "var y=DTY&&A(DTY,t,[]);return !!(y&&A(IX,y,['Files'])>=0)};" +
            "var bf=function(e){if(hf(DP&&A(IP,DP,[e])?A(DG,e,[]):CP&&A(IP,CP,[e])?A(CG,e,[]):0)){A(PD,e,[]);A(SP,e,[])}};" +
            "var ev=['dragover','drop','paste'];for(var i=0;i<3;i++)A(L,W,[ev[i],bf,true]);" +
            "var o=W.origin;if(o!=='null'){if(!at(o))stop();else{var J=W.$SEEN;if(J)J.postMessage('')}return}" +
            "var l=location.href;if(l.indexOf('about:')===0)return;if(!at(l))stop()}catch(e){stop()}})();"
    }

    private fun asksForDocument(accept: String?): Boolean =
        accept != null && (accept.contains("text/html", ignoreCase = true) || accept.contains("application/xhtml", ignoreCase = true))
}

/**
 * Adds [WebRules.documentScript] to a web view, to run at the start of every
 * document in every frame, the one check that also sees a document reached
 * by a redirect the web view's callbacks are never told of (a frame's, a
 * posted form's); and, before it, the listener through which each document
 * of the app says where it is, so that its origin is written down as one to
 * clear when the session ends ([SiteWipe.saw]). What a document posts is not
 * read: the origin is the one the web view names for the message. Call before
 * the first load. Where the system's web view lacks either feature
 * (`WebViewFeature.DOCUMENT_START_SCRIPT`, `WebViewFeature.WEB_MESSAGE_LISTENER`),
 * nothing is added, and a web page is not shown without them
 * ([WebRules.guardsInForce]). That is said once here in the log, and every
 * time by the page. Answers whether both were added.
 */
fun guardDocuments(view: WebView, origins: Origins): Boolean = guardDocuments(view, origins, ClientLog.told)

internal fun guardDocuments(view: WebView, origins: Origins, told: Told): Boolean {
    if (!WebViewFeature.isFeatureSupported(WebViewFeature.DOCUMENT_START_SCRIPT) || !WebViewFeature.isFeatureSupported(WebViewFeature.WEB_MESSAGE_LISTENER)) {
        told.once(
            "documents",
            "this system's web view cannot run a script at the start of a document, or cannot hear from one: a web page is not shown",
        )
        return false
    }
    return try {
        val allowed = origins.documentOrigins()
        // The listener first: the script at the start of a document finds its object there (the reference of `addDocumentStartJavaScript` says so).
        WebViewCompat.addWebMessageListener(view, WebRules.SEEN, WebRules.listenerRules(allowed)) { _, _, source, _, _ -> SiteWipe.saw(source.toString()) }
        WebViewCompat.addDocumentStartJavaScript(view, WebRules.documentScript(allowed), setOf("*"))
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
 * hosts) is answered 403. While the service workers of a session that ended
 * are being taken away, a worker's request for the client's own document of
 * that go is answered with it here ([SiteWipe.document]): the platform's
 * reference for this callback says that where an answer is returned, "the
 * return response and data will be used", so a worker that fetches what its
 * pages ask for hands the client's document on and the address is not sent.
 * Every other request of a worker that this callback is asked about goes by
 * what is owed ([SiteWipe.worker]). One to an origin that still holds what a
 * session's pages left, or from a worker of such an origin, is answered 403,
 * go or no go: the go's document starts the workers of the person who left,
 * and nothing they send this way leaves with the cookies of whoever is
 * signed in by then. While the cookie store stands empty for a go, one that
 * says nothing of where it is from is answered 403 too, and the rest, the
 * requests of the workers of pages that were on the screen, are neither
 * sent nor answered ([WorkerRequest.Fail]): sent, they would go without the
 * session of the person who is signed in. A socket a worker opens is not a
 * request this callback is asked about; that is why every go runs with no
 * cookie in the store ([WebViewCookies.takeOut]).
 * The setting is the process's: other code that sets a client of its own
 * replaces this one, and nothing tells. Throws when the system's web view
 * cannot take it.
 */
internal fun setServiceWorkerRules(origins: Origins) {
    ServiceWorkerController.getInstance().setServiceWorkerClient(
        object : ServiceWorkerClient() {
            override fun shouldInterceptRequest(request: WebResourceRequest): WebResourceResponse? {
                val url = request.url.toString()
                SiteWipe.document(url)?.let { return it }
                val what = WebRules.workerRequest(origins.ours(url), origins.webHost(url), SiteWipe.worker(url, WebRules.workerFrom(request.requestHeaders)))
                workerAsked?.invoke(url, request.requestHeaders.orEmpty(), what)
                return when (what) {
                    WorkerRequest.Send -> null
                    WorkerRequest.Refuse -> WebResourceResponse("text/plain", "utf-8", 403, "Forbidden", emptyMap(), ByteArrayInputStream(ByteArray(0)))
                    // No answer can be made out of this one: the web view ends the request as one that failed. Where a web view
                    // hands the worker an answer all the same, it is one that says to come again.
                    WorkerRequest.Fail -> WebResourceResponse("text/plain", "utf-8", 503, "Service Unavailable", mapOf("Retry-After" to "5", "Cache-Control" to "no-store"), Unreadable())
                }
            }
        },
    )
}

/** A body that cannot be read, nor its length asked: what the web view is handed for a request that is to end as one that failed. */
private class Unreadable : InputStream() {
    override fun read(): Int = throw IOException("not sent")

    override fun read(bytes: ByteArray, offset: Int, length: Int): Int = throw IOException("not sent")

    override fun available(): Int = throw IOException("not sent")
}

/**
 * For a test only: told each request of a service worker the client is asked
 * about, the headers the web view handed over with it, and what became of
 * it. Called on a thread of the web view's own.
 */
@Volatile
internal var workerAsked: ((url: String, headers: Map<String, String>, what: WorkerRequest) -> Unit)? = null

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
        var navigation = WebRules.navigation(origins.ours(url), origins.webHost(url), request.isForMainFrame, request.isRedirect, request.hasGesture())
        // Where a redirect or a frame leads is held back as a page is, while its origin still holds what the pages of an ended session left.
        if (navigation == WebNavigation.Load && SiteWipe.held(url)) navigation = WebNavigation.Stop
        if (WebRules.leftByRedirect(request.isForMainFrame, request.isRedirect, navigation)) onLeft()
        return when (navigation) {
            WebNavigation.Load -> {
                // A redirect that is followed, or a frame that goes on: a document is about to be on this origin.
                SiteWipe.saw(url)
                false
            }
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
            WebRequest.Send -> {
                if (WebRules.document(request.isForMainFrame, accept)) {
                    // A document on an origin that still holds what the pages of an ended session left is not loaded, in a frame either.
                    if (SiteWipe.held(url)) return if (request.isForMainFrame) empty(204, "No Content") else empty(403, "Forbidden")
                    // An origin a document is loaded from is one whose workers and caches go when the session ends.
                    SiteWipe.saw(url)
                }
                null
            }
            WebRequest.KeepPage -> empty(204, "No Content")
            WebRequest.Refuse -> empty(403, "Forbidden")
        }
    }

    override fun onPageStarted(view: WebView, url: String?, favicon: Bitmap?) {
        // The last word for what the checks before cannot see, such as a redirect of a posted form.
        if (!WebRules.shown(url != null && origins.ours(url), url) || (url != null && origins.ours(url) && SiteWipe.held(url))) {
            view.stopLoading()
            onLeft()
            return
        }
        // Where the page is now, whatever brought it there.
        url?.let { SiteWipe.saw(it) }
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
