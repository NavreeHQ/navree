package com.navree.client.platform

import android.content.Context
import android.os.Handler
import android.os.Looper
import android.webkit.RenderProcessGoneDetail
import android.webkit.WebResourceRequest
import android.webkit.WebResourceResponse
import android.webkit.WebStorage
import android.webkit.WebView
import android.webkit.WebViewClient
import androidx.webkit.WebViewCompat
import androidx.webkit.WebViewFeature
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.intOrNull
import java.io.ByteArrayInputStream
import java.io.File
import java.security.SecureRandom
import kotlin.concurrent.thread

/**
 * How the service workers and the caches of an origin are taken away on a
 * system web view that cannot delete every kind of site data itself: plain
 * functions, so each can be tested without a web view.
 *
 * A worker is unregistered, and a cache deleted, only by a script in a
 * document of that origin. A document loaded with `loadDataWithBaseURL` has the
 * origin of its base address, but the engine makes no service worker client
 * for it (its navigation is to a `data:` address), so the registrations cannot
 * be asked for there. So the client's document is a real address of the origin
 * that the client answers itself ([own], [DOCUMENT]), and what does the work
 * is a script at the start of the document ([script]): it runs before anything
 * of the page, also where the origin's own worker answers the address with a
 * page of its own, which it empties first.
 */
object WipeRules {
    /** The name of the object the script says what it did through: a message listener the web view gives only to the origins being cleared. */
    const val LISTENER = "navreeWipe"

    /** The mark, in its query, of the address the client answers itself. */
    private const val MARK = "navree-wipe"

    /** The one document the web view is answered with: nothing in it. */
    const val DOCUMENT = "<!doctype html><html><head><meta charset=\"utf-8\"><title></title></head><body></body></html>"

    /** The address on [origin] the client answers itself in the go named [word]. */
    fun address(origin: String, word: String): String = "$origin/?$MARK=$word"

    /** Whether a request is for the client's own document of the go named [word]: the main frame's, at an address that carries the word. */
    fun own(url: String, mainFrame: Boolean, word: String): Boolean = mainFrame && url.substringAfter('?', "") == "$MARK=$word"

    /**
     * The origins to clear, each once: the app's own that name one host, the
     * hosts it names for the web view that name one host, and every origin a
     * page or a frame of the app was seen to load a document from (which is how
     * a host under a `*.` pattern is known).
     */
    fun origins(own: List<String>, hosts: List<String>, seen: Collection<String>): List<String> = (own + hosts + seen.sorted()).distinct()

    /** Whether two origins, written as `scheme://host[:port]`, are the same one. */
    fun same(one: String, other: String): Boolean {
        val first = Site.read(one) ?: return false
        return Site.read(other)?.sameOrigin(first) == true
    }

    /**
     * The script put at the start of the document. It stops and empties the
     * document, as the check on a document of another origin does, so nothing of
     * a page the origin's worker may have answered with is drawn or run; then,
     * in the main frame only, it unregisters every service worker registration
     * of the origin, deletes every cache, counts the caches again, and says what
     * it did through [LISTENER]: `{"n": the go's word, "workers", "caches",
     * "left", "error"?}`. [word] is hex digits only: it is written into the script.
     */
    fun script(word: String): String {
        require(word.isNotEmpty() && word.all { it in '0'..'9' || it in 'a'..'f' }) { "the word of a go is hex digits" }
        return "(function(){" +
            "try{window.stop()}catch(e){}" +
            "try{new MutationObserver(function(l){for(var i=0;i<l.length;i++){var a=l[i].addedNodes;for(var j=0;j<a.length;j++){var n=a[j];if(n.parentNode)n.parentNode.removeChild(n)}}})" +
            ".observe(document,{childList:true,subtree:true})}catch(e){}" +
            "var d=document.documentElement;if(d)d.remove();" +
            "var L=self.$LISTENER;if(!L||self!==self.top)return;" +
            "var o={n:'$word',workers:0,caches:0,left:0};" +
            "var say=function(){try{L.postMessage(JSON.stringify(o))}catch(e){}};" +
            "var fail=function(e){o.error=String(e);say()};" +
            "try{var p=Promise.resolve();" +
            "if(self.navigator&&navigator.serviceWorker)p=p.then(function(){return navigator.serviceWorker.getRegistrations()})" +
            ".then(function(r){o.workers=r.length;var u=[];for(var i=0;i<r.length;i++)u.push(r[i].unregister());return Promise.all(u)});" +
            "if(self.caches)p=p.then(function(){return caches.keys()})" +
            ".then(function(k){o.caches=k.length;var u=[];for(var i=0;i<k.length;i++)u.push(caches.delete(k[i]));return Promise.all(u)})" +
            ".then(function(){return caches.keys()}).then(function(k){o.left=k.length});" +
            "p.then(say,fail)}catch(e){fail(e)}})();"
    }

    /** What a document said it did: how many workers it unregistered and caches it deleted, and whether nothing is left and nothing failed. */
    data class Report(val workers: Int, val caches: Int, val clean: Boolean)

    /** Reads what the script said in the go named [word]; null for anything else (another go's word, what does not read). */
    fun read(message: String?, word: String): Report? {
        val fields = message?.let { text -> runCatching { Json.parseToJsonElement(text) as? JsonObject }.getOrNull() } ?: return null
        fun said(name: String) = (fields[name] as? JsonPrimitive)?.contentOrNull
        fun count(name: String) = (fields[name] as? JsonPrimitive)?.intOrNull
        if (said("n") != word) return null
        val left = count("left") ?: return null
        return Report(count("workers") ?: 0, count("caches") ?: 0, clean = left == 0 && fields["error"] == null)
    }
}

/**
 * What the pages of a session that ended left in the web view, as something
 * owed: from the moment the session ends until the web view has said it is
 * gone, a deletion is owed ([owe], [over]), no web page is shown ([clean]),
 * and what a run of the app did not finish the next one does ([finish]). The
 * mark is a file no backup takes, so it outlives the process.
 *
 * Where the system's web view deletes every kind of site data itself
 * (`WebViewFeature.DELETE_BROWSING_DATA`) that call is what is waited for.
 * Where it does not, the older call leaves the service workers and Cache
 * Storage, and [documents] takes those away origin by origin ([WipeRules]).
 * The origins are the ones the app names that are one host each and every
 * origin a page or a frame of the app loaded a document from ([saw]), kept in a
 * second file until a deletion finished.
 */
object SiteWipe {
    private const val OWED = "navree-web-wipe"
    private const val SEEN = "navree-web-origins"

    /** The longest the document of one origin is waited for. */
    internal const val ORIGIN_MS = 3_000L

    private val lock = Any()
    private val main = Handler(Looper.getMainLooper())
    private var app: Context? = null
    private var origins: Origins? = null
    private var owed = false
    private var running = false
    private var seen: MutableSet<String>? = null

    /** Says which app and which origins this is for, and reads whether an earlier run of the app left a deletion owed. */
    fun arm(context: Context, origins: Origins) {
        val application = context.applicationContext
        synchronized(lock) {
            app = application
            this.origins = origins
            owed = owed || File(application.noBackupFilesDir, OWED).exists()
        }
    }

    /** Whether a deletion is owed. */
    val owes: Boolean get() = synchronized(lock) { owed }

    /** A page or a frame of the app is about to load a document from this address: its origin is one to clear. Any thread. */
    fun saw(address: String) {
        val origin = synchronized(lock) { origins }?.originOf(address) ?: return
        synchronized(lock) {
            val folder = app?.noBackupFilesDir ?: return
            val known = seen ?: load(folder).also { seen = it }
            if (known.add(origin)) runCatching { File(folder, SEEN).appendText(origin + "\n") }
        }
    }

    private fun load(folder: File): MutableSet<String> =
        runCatching { File(folder, SEEN).readLines().filterTo(LinkedHashSet()) { it.isNotBlank() } }.getOrElse { LinkedHashSet() }

    /** A session ended: what its pages stored is owed a deletion from now, and one is on its way. It blocks on a small file: not on the main thread. */
    fun owe() {
        synchronized(lock) {
            owed = true
            running = true
            app?.let { context -> runCatching { File(context.noBackupFilesDir, OWED).writeText("") } }
        }
    }

    /** The deletion on its way ended; [clean] when the web view said everything is gone, and nothing is owed any more. Not on the main thread. */
    fun over(clean: Boolean) {
        synchronized(lock) {
            running = false
            if (!clean) return
            owed = false
            seen = LinkedHashSet()
            app?.let { context ->
                runCatching { File(context.noBackupFilesDir, OWED).delete() }
                runCatching { File(context.noBackupFilesDir, SEEN).delete() }
            }
        }
    }

    /**
     * Lets go of what is owed without doing it. For a test only, when it is over: a deletion a test that ended a session
     * left unfinished is that test's own failure, not the refusal of every web page the tests after it show.
     */
    internal fun forget() = over(true)

    /**
     * Whether nothing of an earlier session's pages is waiting to be taken away:
     * a web page is shown only then. Asking while something is owed and nothing
     * is being done about it starts another go, so "Try again" on the page that
     * was refused is a way on. Call on the main thread.
     */
    fun clean(): Boolean {
        val (owing, busy) = synchronized(lock) { owed to running }
        if (!owing) return true
        if (!busy) finish()
        return false
    }

    /**
     * Does what is still owed: what the older call covers, then the documents.
     * Never the cookies, which may be a new session's by now; no page of a new
     * session has been shown while this was owed, so nothing else of it is in
     * the web view. Call on the main thread.
     */
    fun finish() {
        val (context, them) = synchronized(lock) {
            if (!owed || running) return
            val context = app ?: return
            val them = origins ?: return
            running = true
            context to them
        }
        try {
            WebStorage.getInstance().deleteAllData()
            documents(context, them) { clean -> thread(name = "navree-web-wipe", isDaemon = true) { over(clean) } }
        } catch (_: Throwable) {
            thread(name = "navree-web-wipe", isDaemon = true) { over(false) }
        }
    }

    /** Whether this system's web view can be given the client's document: a script at the start of a document, and a listener for what it says. */
    internal fun able(): Boolean =
        WebViewFeature.isFeatureSupported(WebViewFeature.DOCUMENT_START_SCRIPT) && WebViewFeature.isFeatureSupported(WebViewFeature.WEB_MESSAGE_LISTENER)

    /**
     * Shows each origin to clear ([WipeRules.origins]) the client's own
     * document, one after the other in one web view that is never drawn, and
     * waits for each to say its workers are unregistered and its caches
     * deleted. [done] is told whether every one did; one that does not say so
     * in [ORIGIN_MS], or says something is left, ends the go as not done. Call
     * on the main thread; [done] runs on it, once.
     */
    fun documents(context: Context, origins: Origins, done: (Boolean) -> Unit) {
        val application = context.applicationContext
        val known = synchronized(lock) { (seen ?: load(application.noBackupFilesDir).also { seen = it }).toList() }
        val targets = WipeRules.origins(origins.ownOrigins(), origins.webHostOrigins(), known)
        if (targets.isEmpty()) return done(true)
        if (!able()) return done(false)
        Go(application, targets, done).start()
    }

    /** One go at the origins, in turn. */
    private class Go(context: Context, private val targets: List<String>, private val done: (Boolean) -> Unit) {
        private val word = ByteArray(16).also { SecureRandom().nextBytes(it) }.joinToString("") { "%02x".format(it) }
        private val view = WebView(context)
        private var at = 0
        private var ended = false
        private val late = Runnable { end(false) }

        fun start() {
            try {
                // As a page of the app has them, so what a page could store this document can reach; and no way to a file or a provider.
                view.settings.javaScriptEnabled = true
                view.settings.domStorageEnabled = true
                view.settings.allowFileAccess = false
                view.settings.allowContentAccess = false
                view.webViewClient = object : WebViewClient() {
                    // Called on a thread of the web view's own. Nothing this web view asks for is sent anywhere: its one document is the client's.
                    override fun shouldInterceptRequest(view: WebView, request: WebResourceRequest): WebResourceResponse =
                        if (WipeRules.own(request.url.toString(), request.isForMainFrame, word)) answer(200, "OK", "text/html", WipeRules.DOCUMENT) else answer(403, "Forbidden", "text/plain", "")

                    // Whatever is in this web view goes nowhere of its own accord, and no redirect is followed.
                    override fun shouldOverrideUrlLoading(view: WebView, request: WebResourceRequest): Boolean = true

                    override fun onRenderProcessGone(view: WebView, detail: RenderProcessGoneDetail): Boolean {
                        main.post { end(false) }
                        return true
                    }
                }
                val rules = targets.toSet()
                // The listener first: the script at the start of a document finds its object there (the reference of `addDocumentStartJavaScript` says so).
                WebViewCompat.addWebMessageListener(view, WipeRules.LISTENER, rules) { _, message, source, mainFrame, _ -> heard(message.data, source.toString(), mainFrame) }
                WebViewCompat.addDocumentStartJavaScript(view, WipeRules.script(word), rules)
                next()
            } catch (_: Throwable) {
                end(false)
            }
        }

        private fun next() {
            main.removeCallbacks(late)
            if (at == targets.size) return end(true)
            main.postDelayed(late, ORIGIN_MS)
            view.loadUrl(WipeRules.address(targets[at], word))
        }

        /** What a document said, with the origin and the frame the web view itself names for it. */
        private fun heard(text: String?, source: String, mainFrame: Boolean) {
            if (ended || at >= targets.size || !mainFrame || !WipeRules.same(source, targets[at])) return
            val report = WipeRules.read(text, word) ?: return
            if (!report.clean) return end(false)
            at++
            next()
        }

        private fun end(clean: Boolean) {
            if (ended) return
            ended = true
            main.removeCallbacks(late)
            try {
                // Its last document goes with it, and with that the last thing a worker it unregistered controlled.
                view.stopLoading()
                view.destroy()
            } catch (_: Throwable) {
                // A web view whose process ended is gone already.
            }
            done(clean)
        }

        private fun answer(status: Int, reason: String, type: String, body: String): WebResourceResponse =
            WebResourceResponse(type, "utf-8", status, reason, mapOf("Cache-Control" to "no-store"), ByteArrayInputStream(body.encodeToByteArray()))
    }
}
