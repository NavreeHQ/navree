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
import java.io.FileOutputStream
import java.nio.file.Files
import java.nio.file.StandardCopyOption
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
 * that the client answers itself wherever the web view asks for it ([address],
 * [own], [DOCUMENT]), and what does the work is a script at the start of the
 * document ([script]): it runs before anything of the page, also where the
 * origin's own worker answers the address with a page of its own, which it
 * empties first.
 */
object WipeRules {
    /** The name of the object the script says what it did through: a message listener the web view gives only to the origins being cleared. */
    const val LISTENER = "navreeWipe"

    /** The longest the document of one origin is waited for, each time the origin is shown it. */
    const val ORIGIN_MS = 3_000L

    /** How often one origin is shown the client's document in one go before the go leaves it and moves on. */
    const val TRIES = 2

    /**
     * The first line of the mark while the whole of what the end of a session
     * deletes is owed: the sealed pages, the snapshot, the chosen files, the
     * notifications, the web view's cache and every cookie, and what pages
     * stored. A mark without it owes only what pages stored at the origins.
     */
    const val EVERYTHING = "everything"

    /** Whether a mark's text says the whole of the end of a session is owed. */
    fun everything(mark: String): Boolean = mark.lineSequence().firstOrNull()?.trim() == EVERYTHING

    /** The one document the web view is answered with: nothing in it. */
    const val DOCUMENT = "<!doctype html><html><head><meta charset=\"utf-8\"><title></title></head><body></body></html>"

    /**
     * The address on [origin] the client answers itself in one go: a path the
     * go made at random ([token]) and nothing else, so nothing a page or a
     * worker of the origin did beforehand can have been made ready for it.
     */
    fun address(origin: String, token: String): String = "$origin/$token"

    /** Whether an address is the client's own document of the go whose path is [token]: that path and nothing after it, on whichever origin. */
    fun own(url: String, token: String): Boolean = token.isNotEmpty() && Site.read(url) != null && url.substringAfter("://").substringAfter('/', "") == token

    /**
     * The origins to clear, each once: the app's own that name one host, the
     * hosts it names for the web view that name one host, and every origin a
     * document of the app was seen on (which is how a host under a `*.`
     * pattern is known).
     */
    fun origins(own: List<String>, hosts: List<String>, seen: Collection<String>): List<String> = (own + hosts + seen.sorted()).distinct()

    /**
     * The origins a file of them names: each line that reads as an origin, as
     * a browser writes it, each once. A line that does not read (one cut
     * short, or two run together, by a process that ended while it wrote) is
     * dropped, so one such line never makes the whole list unusable.
     */
    fun listed(text: String): List<String> = text.lineSequence().mapNotNull { line -> Site.read(line.trim())?.takeIf { it.bare }?.origin() }.distinct().toList()

    /** Origins as the lines of a file of them. */
    fun lines(origins: Collection<String>): String = origins.joinToString("") { it + "\n" }

    /**
     * Writes [text] as the whole of [file], so that the file is at every
     * moment the text it held before or the new one and never a part of
     * either: under another name beside it, forced to the disk, then put in
     * its place in one step.
     */
    fun put(file: File, text: String) {
        val fresh = File(file.parentFile, file.name + ".new")
        try {
            FileOutputStream(fresh).use { out ->
                out.write(text.encodeToByteArray())
                out.fd.sync()
            }
            Files.move(fresh.toPath(), file.toPath(), StandardCopyOption.ATOMIC_MOVE)
        } finally {
            fresh.delete()
        }
    }

    /** Whether two origins, written as `scheme://host[:port]`, are the same one. */
    fun same(one: String, other: String): Boolean {
        val first = Site.read(one) ?: return false
        return Site.read(other)?.sameOrigin(first) == true
    }

    /**
     * Whether a web page on [origin] is held back while a deletion is owed.
     * Until a go has ended ([left] is null) every page is. After it, only a
     * page on an origin the go left, which did not say it is clean, and a page
     * whose origin cannot be told ([origin] is null): the others are shown, so
     * one origin that cannot be cleared does not keep the rest of the app's
     * pages from the person.
     */
    fun held(left: Set<String>?, origin: String?): Boolean = left == null || origin == null || origin in left

    /**
     * Whose turn it is in one go at the origins. Each is shown the client's
     * document up to [tries] times; one that has not said by then that it is
     * clean is left ([left]) and the go moves on to the next.
     */
    class Turns(private val targets: List<String>, private val tries: Int = TRIES) {
        private var at = 0
        private var tried = 0
        private val kept = LinkedHashSet<String>()

        /** The origin whose turn it is, or null when the go is over. */
        val current: String? get() = targets.getOrNull(at)

        /** The origins that did not say they are clean, so far. */
        val left: Set<String> get() = kept.toSet()

        /** The origin in turn said it is clean: on to the next. */
        fun cleared() {
            if (at >= targets.size) return
            at++
            tried = 0
        }

        /** The origin in turn did not say in time that it is clean, or said something is left: it is shown the document again, or left. */
        fun missed() {
            if (at >= targets.size) return
            tried++
            if (tried < tries) return
            kept += targets[at]
            at++
            tried = 0
        }

        /** The go cannot go on: every origin not yet cleared is left. */
        fun stop() {
            while (at < targets.size) kept += targets[at++]
            tried = 0
        }
    }

    /**
     * The script put at the start of the document. It stops and empties the
     * document, as the check on a document of another origin does, so nothing of
     * a page the origin's worker may have answered with is drawn or run; then,
     * in the main frame only, it unregisters every service worker registration
     * of the origin, deletes every cache, counts the caches again, and says what
     * it did through [LISTENER]: `{"n": the go's word, "workers", "caches",
     * "left", "error"?}`. [word] is in the script and nowhere in the address,
     * so a page that reads its own address does not learn it. It is hex
     * digits only: it is written into the script.
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

/** What one go at the origins works in: a web view that is shown one address at a time, and a clock. A test gives its own. */
internal interface WipeSurface {
    /**
     * Makes the web view, with [script] at the start of every document of
     * [origins]. What such a document says goes to [heard], with the origin
     * and the frame the web view itself names for it; [broke] is called when
     * the web view is lost. Throws when it cannot be made.
     */
    fun open(origins: Set<String>, script: String, heard: (text: String?, source: String, mainFrame: Boolean) -> Unit, broke: () -> Unit)

    /** Shows [address]. */
    fun load(address: String)

    /** Calls [what] once after [ms], unless [cancel] or another call of this comes first. */
    fun after(ms: Long, what: () -> Unit)

    /** Takes back what [after] set. */
    fun cancel()

    /** The go is over: the web view goes, and its last document with it. */
    fun close()
}

/**
 * One go at the origins, each in its turn ([WipeRules.Turns]): an origin is
 * shown the client's document, at the address the go made ([token]), and has
 * [WipeRules.ORIGIN_MS] to say through the listener, with the go's [word],
 * that nothing is left. [done] is told, once, which origins did not. Every
 * call is on the one thread the surface calls back on.
 */
internal class WipeGo(
    targets: List<String>,
    private val token: String,
    private val word: String,
    private val surface: WipeSurface,
    private val done: (left: Set<String>) -> Unit,
) {
    private val rules = targets.toSet()
    private val turns = WipeRules.Turns(targets)
    private var ended = false

    fun start() {
        try {
            surface.open(rules, WipeRules.script(word), ::heard, ::broke)
        } catch (_: Throwable) {
            return broke()
        }
        next()
    }

    private fun next() {
        surface.cancel()
        val origin = turns.current ?: return end()
        surface.after(WipeRules.ORIGIN_MS) { missed() }
        try {
            surface.load(WipeRules.address(origin, token))
        } catch (_: Throwable) {
            broke()
        }
    }

    private fun missed() {
        if (ended) return
        turns.missed()
        next()
    }

    /** What a document said. Only the main frame of the origin in turn, with this go's word, is heard. */
    fun heard(text: String?, source: String, mainFrame: Boolean) {
        val origin = turns.current
        if (ended || origin == null || !mainFrame || !WipeRules.same(source, origin)) return
        val report = WipeRules.read(text, word) ?: return
        if (report.clean) turns.cleared() else turns.missed()
        next()
    }

    /** The web view is lost, or could not be made: every origin not yet cleared is left. */
    fun broke() {
        if (ended) return
        turns.stop()
        end()
    }

    private fun end() {
        if (ended) return
        ended = true
        surface.cancel()
        try {
            surface.close()
        } catch (_: Throwable) {
            // A web view whose process ended is gone already.
        }
        done(turns.left)
    }
}

/**
 * What the end of a session deletes on this device, as something owed: from
 * the moment the session ends until the web view has said that what its pages
 * stored is gone, a deletion is owed ([owe], [over]), a web page is shown only
 * where nothing of it can be left ([shows]), and what a run of the app did not
 * finish the next one does. The mark is a file no backup takes, so it
 * outlives the process, and it says how much is owed: the whole of the end of
 * a session ([owesAll]), until the cookies and everything else that is not a
 * page's own storage are gone and on the disk ([narrow]); after that only
 * what pages stored at the origins ([finish]).
 *
 * Where the system's web view deletes every kind of site data itself
 * (`WebViewFeature.DELETE_BROWSING_DATA`) that call is what is waited for.
 * Where it does not, the older call leaves the service workers and Cache
 * Storage, and [documents] takes those away origin by origin ([WipeRules]).
 * The origins are the ones the app names that are one host each and every
 * origin a document of the app was on in the web view, however it got there
 * ([saw]), kept in a second file until a deletion finished.
 *
 * An origin that does not say it is clean does not hold the others: it is
 * left, the mark stays and names it, its pages stay held back, and every
 * later go (one the person asks for with "Try again", and the next start's)
 * is at the origins that were left.
 */
object SiteWipe {
    private const val OWED = "navree-web-wipe"
    private const val SEEN = "navree-web-origins"

    private val lock = Any()
    private val main = Handler(Looper.getMainLooper())
    private var app: Context? = null
    private var origins: Origins? = null
    private var owed = false
    private var running = false

    /** While a deletion is owed: whether it is the whole of the end of a session, the cookies and the rest with what pages stored. */
    private var all = false

    /** While a deletion is owed: the origins a go left, or null while no go has ended and every origin is still to clear. */
    private var left: Set<String>? = null

    /** Grows with every deletion that becomes owed, so that a go begun for an earlier one says nothing about this one. */
    private var round = 0L
    private var seen: MutableSet<String>? = null

    /** The path of the address the go on its way loads. Read on the thread a service worker's requests are asked on. */
    @Volatile
    private var going: String? = null

    /** Says which app and which origins this is for, and reads whether an earlier run of the app left a deletion owed, and for which origins. */
    fun arm(context: Context, origins: Origins) {
        val application = context.applicationContext
        synchronized(lock) {
            app = application
            this.origins = origins
            if (owed) return
            val mark = File(application.noBackupFilesDir, OWED)
            if (!mark.exists()) return
            owed = true
            // A mark that cannot be read owes everything.
            val text = runCatching { mark.readText() }.getOrDefault(WipeRules.EVERYTHING)
            all = WipeRules.everything(text)
            // The origins an earlier go left, where the mark names them; a mark that names none owes every origin.
            left = WipeRules.listed(text).toSet().ifEmpty { null }
        }
    }

    /** Whether a deletion is owed. */
    val owes: Boolean get() = synchronized(lock) { owed }

    /**
     * Whether the whole of the end of a session is owed: a run of the app was
     * told that the session ended and did not get as far as the cookies. The
     * client then does all of it again, as at the end of a session, before
     * anything of this run is read, kept or shown ([AndroidEffects]).
     */
    val owesAll: Boolean get() = synchronized(lock) { owed && all }

    /**
     * A document of the app is, or is about to be, at this address or on this
     * origin: its origin is one to clear, written to the file before this
     * returns. The web view's client says so for a request for a document, for
     * a redirect it follows and for a page that starts, and every document
     * says it for itself ([guardDocuments]). An address that is not on the
     * app's origins or its web hosts is not written. Any thread.
     */
    fun saw(address: String) {
        val origin = synchronized(lock) { origins }?.originOf(address) ?: return
        synchronized(lock) {
            val folder = app?.noBackupFilesDir ?: return
            val known = seen ?: load(folder).also { seen = it }
            // The whole list is written again, in one step. One that could not be written is not known, so the next sighting writes it.
            if (known.add(origin) && runCatching { WipeRules.put(File(folder, SEEN), WipeRules.lines(known)) }.isFailure) known.remove(origin)
        }
    }

    private fun load(folder: File): MutableSet<String> =
        runCatching { WipeRules.listed(File(folder, SEEN).readText()).toCollection(LinkedHashSet()) }.getOrElse { LinkedHashSet() }

    /**
     * A session ended: the whole of what that deletes is owed from now, what
     * its pages stored at every origin with it, and the deletion is on its
     * way. It blocks on a small file: not on the main thread.
     */
    fun owe() {
        synchronized(lock) {
            owed = true
            all = true
            running = true
            left = null
            round++
            app?.let { context -> runCatching { WipeRules.put(File(context.noBackupFilesDir, OWED), WipeRules.EVERYTHING + "\n") } }
        }
    }

    /**
     * The cookies, and everything else of the end of a session that is not
     * what pages stored, are gone and on the disk: from now the mark owes only
     * what pages stored at the origins. Not on the main thread.
     */
    fun narrow() {
        synchronized(lock) {
            if (!owed || !all) return
            all = false
            app?.let { context -> runCatching { WipeRules.put(File(context.noBackupFilesDir, OWED), "") } }
        }
    }

    /**
     * The deletion on its way ended. [left] names the origins that did not say
     * they are clean. None: nothing is owed any more. Some: they stay owed and
     * the mark names them, while the pages of every other origin are shown
     * again. Null: nothing could be done at all, and what was owed before is
     * owed still. Not on the main thread.
     */
    fun over(left: Set<String>?): Unit = over(left, null)

    private fun over(left: Set<String>?, of: Long?) {
        synchronized(lock) {
            // A go begun before a later end of a session says nothing about what that one owes.
            if (of != null && of != round) return
            running = false
            // While the whole is owed nothing a go says ends it: the cookies come first ([narrow]).
            if (!owed || all || left == null) return
            val folder = app?.noBackupFilesDir
            // A go at every origin, and no page shown since the deletion became owed: the list of origins seen starts again, from those that were left.
            if (this.left == null) {
                seen = LinkedHashSet(left)
                folder?.let { runCatching { if (left.isEmpty()) File(it, SEEN).delete() else WipeRules.put(File(it, SEEN), WipeRules.lines(left)) } }
            }
            if (left.isEmpty()) {
                owed = false
                this.left = null
                folder?.let { runCatching { File(it, OWED).delete() } }
            } else {
                this.left = left
                folder?.let { runCatching { WipeRules.put(File(it, OWED), WipeRules.lines(left)) } }
            }
        }
    }

    /**
     * Lets go of what is owed without doing it. For a test only, when it is over: a deletion a test that ended a session
     * left unfinished is that test's own failure, not the refusal of every web page the tests after it show.
     */
    internal fun forget() {
        synchronized(lock) {
            owed = false
            all = false
            running = false
            left = null
            round++
            seen = LinkedHashSet()
            app?.let { context -> for (name in listOf(OWED, SEEN)) runCatching { File(context.noBackupFilesDir, name).delete() } }
        }
    }

    /**
     * Whether a web page at [address] may be shown: nothing is owed, or a go
     * has ended and the page's origin is not one it left ([WipeRules.held]).
     * Asking for a page that is held back while nothing is being done about it
     * starts another go, so "Try again" on the page that was refused is a way
     * on. Call on the main thread.
     */
    fun shows(address: String): Boolean {
        val busy = synchronized(lock) {
            if (!owed || !WipeRules.held(left, origins?.originOf(address))) return true
            running
        }
        if (!busy) finish()
        return false
    }

    /**
     * Whether a document at [address] is held back as a page on its origin
     * is: what the web view's client asks for a frame, and for where a
     * redirect leads. Any thread.
     */
    fun held(address: String): Boolean = synchronized(lock) { owed && WipeRules.held(left, origins?.originOf(address)) }

    /**
     * Does what is still owed of what pages stored: what the older call
     * covers, while no page has been shown since the deletion became owed,
     * then the documents, at every origin or at those an earlier go left.
     * Never the cookies, which may be a new session's by now: while the whole
     * of the end of a session is owed ([owesAll]) this does nothing, and the
     * next start of the app does all of it. Call on the main thread.
     */
    fun finish() {
        val (context, them, began) = synchronized(lock) {
            if (!owed || running || all) return
            val context = app ?: return
            val them = origins ?: return
            running = true
            Triple(context, them, round)
        }
        try {
            // Once pages are shown again, what the web view holds is a later session's as well, and the older call takes everything: it is not made then.
            if (synchronized(lock) { left } == null) WebStorage.getInstance().deleteAllData()
            documents(context, them) { left -> thread(name = "navree-web-wipe", isDaemon = true) { over(left, began) } }
        } catch (_: Throwable) {
            thread(name = "navree-web-wipe", isDaemon = true) { over(null, began) }
        }
    }

    /** Whether this system's web view can be given the client's document: a script at the start of a document, and a listener for what it says. */
    internal fun able(): Boolean =
        WebViewFeature.isFeatureSupported(WebViewFeature.DOCUMENT_START_SCRIPT) && WebViewFeature.isFeatureSupported(WebViewFeature.WEB_MESSAGE_LISTENER)

    /**
     * The client's own document, when [url] is the address the go on its way
     * loads; null for any other address. Asked where a service worker's
     * requests are asked ([setServiceWorkerRules]): a worker of the origin
     * that fetches what its pages ask for is handed the client's document, and
     * the address is never sent to a server. Any thread.
     */
    internal fun document(url: String): WebResourceResponse? =
        going?.takeIf { WipeRules.own(url, it) }?.let { answer(200, "OK", "text/html", WipeRules.DOCUMENT) }

    /**
     * Shows each origin to clear the client's own document, one after the
     * other in one web view that is never drawn, and waits for each to say its
     * workers are unregistered and its caches deleted ([WipeGo]). The origins
     * are every one known ([WipeRules.origins]), or those an earlier go left.
     * [done] is told which did not say so; null when the go could not be made
     * (a system web view without the script at the start of a document or the
     * listener, or one that does not take the rules for a service worker's
     * requests). Call on the main thread; [done] runs on it, once.
     */
    fun documents(context: Context, origins: Origins, done: (left: Set<String>?) -> Unit) {
        val application = context.applicationContext
        val (known, only) = synchronized(lock) { (seen ?: load(application.noBackupFilesDir).also { seen = it }).toList() to left }
        val targets = only?.toList() ?: WipeRules.origins(origins.ownOrigins(), origins.webHostOrigins(), known)
        if (targets.isEmpty()) return done(emptySet())
        // The go's address is answered where a service worker's requests are asked as well, so those rules have to be in force.
        if (!able() || !guardWorkers(origins)) return done(null)
        val token = hex()
        going = token
        WipeGo(targets, token, hex(), WebSurface(application, token)) { left ->
            if (going == token) going = null
            done(left)
        }.start()
    }

    /** Sixteen bytes from the system's source of random numbers, as hex digits. */
    private fun hex(): String = ByteArray(16).also { SecureRandom().nextBytes(it) }.joinToString("") { "%02x".format(it) }

    private fun answer(status: Int, reason: String, type: String, body: String): WebResourceResponse =
        WebResourceResponse(type, "utf-8", status, reason, mapOf("Cache-Control" to "no-store"), ByteArrayInputStream(body.encodeToByteArray()))

    /** The web view a go works in: never drawn, and nothing it asks for is sent anywhere. */
    private class WebSurface(private val context: Context, private val token: String) : WipeSurface {
        private var web: WebView? = null
        private var late: Runnable? = null

        override fun open(origins: Set<String>, script: String, heard: (text: String?, source: String, mainFrame: Boolean) -> Unit, broke: () -> Unit) {
            val made = WebView(context)
            web = made
            // As a page of the app has them, so what a page could store this document can reach; and no way to a file or a provider.
            made.settings.javaScriptEnabled = true
            made.settings.domStorageEnabled = true
            made.settings.allowFileAccess = false
            made.settings.allowContentAccess = false
            made.webViewClient = object : WebViewClient() {
                // Called on a thread of the web view's own. Nothing this web view asks for is sent anywhere: its one document is the client's.
                override fun shouldInterceptRequest(view: WebView, request: WebResourceRequest): WebResourceResponse =
                    if (request.isForMainFrame && WipeRules.own(request.url.toString(), token)) answer(200, "OK", "text/html", WipeRules.DOCUMENT) else answer(403, "Forbidden", "text/plain", "")

                // Whatever is in this web view goes nowhere of its own accord, and no redirect is followed.
                override fun shouldOverrideUrlLoading(view: WebView, request: WebResourceRequest): Boolean = true

                override fun onRenderProcessGone(view: WebView, detail: RenderProcessGoneDetail): Boolean {
                    main.post { broke() }
                    return true
                }
            }
            // The listener first: the script at the start of a document finds its object there (the reference of `addDocumentStartJavaScript` says so).
            WebViewCompat.addWebMessageListener(made, WipeRules.LISTENER, origins) { _, message, source, mainFrame, _ -> heard(message.data, source.toString(), mainFrame) }
            WebViewCompat.addDocumentStartJavaScript(made, script, origins)
        }

        override fun load(address: String) {
            web?.loadUrl(address)
        }

        override fun after(ms: Long, what: () -> Unit) {
            cancel()
            val run = Runnable { what() }
            late = run
            main.postDelayed(run, ms)
        }

        override fun cancel() {
            late?.let(main::removeCallbacks)
            late = null
        }

        override fun close() {
            val gone = web ?: return
            web = null
            // Its last document goes with it, and with that the last thing a worker it unregistered controlled.
            gone.stopLoading()
            gone.destroy()
        }
    }
}
