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
import androidx.compose.runtime.mutableIntStateOf
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
     * of the origin, deletes every cache and every IndexedDB database, empties
     * local storage, waits a quarter of a second, and counts all four again:
     * what something of the session that was still running put back in that
     * time is counted as left. It says what it did through [LISTENER]: `{"n":
     * the go's word, "workers", "caches", "left", "error"?}`, where `left` is
     * everything the second count found. [word] is in the script and nowhere in the address,
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
            // What the origin holds, asked for anew each time it is counted: registrations, caches, databases, local storage.
            "var W=function(){return self.navigator&&navigator.serviceWorker?navigator.serviceWorker.getRegistrations():Promise.resolve([])};" +
            "var C=function(){return self.caches?caches.keys():Promise.resolve([])};" +
            "var B=function(){return self.indexedDB&&indexedDB.databases?indexedDB.databases():Promise.resolve([])};" +
            "var S=function(){try{return self.localStorage||null}catch(e){return null}};" +
            "var G=function(n){return new Promise(function(y){try{var q=indexedDB.deleteDatabase(n);q.onsuccess=q.onerror=q.onblocked=function(){y()}}catch(e){y()}})};" +
            "try{var p=W().then(function(r){o.workers=r.length;var u=[];for(var i=0;i<r.length;i++)u.push(r[i].unregister());return Promise.all(u)})" +
            ".then(C).then(function(k){o.caches=k.length;var u=[];for(var i=0;i<k.length;i++)u.push(caches.delete(k[i]));return Promise.all(u)})" +
            ".then(B).then(function(b){var u=[];for(var i=0;i<b.length;i++)u.push(G(b[i].name));return Promise.all(u)})" +
            ".then(function(){var s=S();if(s)s.clear();return new Promise(function(y){setTimeout(y,250)})})" +
            ".then(function(){return Promise.all([W(),C(),B()])})" +
            ".then(function(a){var s=S();o.left=a[0].length+a[1].length+a[2].length+(s?s.length:0)});" +
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

    /** The web view is lost or could not be made, or another go takes this one's place ([WipeGoes]): every origin not yet cleared is left. */
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
 * The go on its way, of which there is one at a time. A go begun while
 * another runs takes its place: the older one was begun for an end of a
 * session that a later one has overtaken, so it ends there and then, leaving
 * the origins it had not cleared ([WipeGo.broke]), and its web view goes with
 * it. So the one address answered as the client's own document ([going]) is
 * always that of the only go there is, and no go's address is left to reach a
 * server because another go's took its place.
 *
 * A go clears an origin by loading a document there, and that starts the
 * service workers the origin's pages registered: the workers of the person
 * who left. What such a worker then asks for on its own (a queue of writes it
 * sends again when it starts, a sync, a refresh of its caches) would leave
 * with the cookies the store holds at that moment, which after a change of
 * person, or once the next person has signed in, are somebody else's. So
 * while a go is on its way, a service worker's requests are refused ([stops]),
 * all but the go's own address: every one of them while the go is at every
 * origin, when no page of the app is shown and a worker has nobody to ask
 * for; and, while the go is only at the origins an earlier go left, those to
 * the origins it is at, since the pages of the others are on the screen again
 * and their workers with them.
 *
 * Every call is on the one thread the surfaces call back on, but for [going]
 * and [stops], which are asked on the thread a service worker's requests are
 * asked on.
 */
internal class WipeGoes {
    /** What a service worker's request is held against: the path of the go's address, the origins the go is at, and whether those are every origin. */
    private class Way(val token: String, val targets: Set<String>, val everywhere: Boolean)

    @Volatile
    private var way: Way? = null

    private var live: WipeGo? = null

    /** The path of the address the go on its way loads; null when no go is on its way. */
    val going: String? get() = way?.token

    /**
     * Whether a request a service worker makes to [url] is refused because a
     * go is on its way. Never the go's own address, which is answered with
     * the client's document. While the go is at every origin, any other
     * address; while it is at the origins an earlier go left, an address on
     * one of those.
     */
    fun stops(url: String): Boolean {
        val now = way ?: return false
        if (WipeRules.own(url, now.token)) return false
        if (now.everywhere) return true
        val origin = Site.read(url)?.origin() ?: return false
        return now.targets.any { WipeRules.same(it, origin) }
    }

    /**
     * Begins a go at [targets] in [surface], whose address is the path
     * [token] and whose document answers with [word], after ending the go on
     * its way, if there is one. [everywhere] says the targets are every
     * origin there is to clear, and not only those an earlier go left. [done]
     * is told, once, which origins the new go left.
     */
    fun start(targets: List<String>, token: String, word: String, surface: WipeSurface, everywhere: Boolean = true, done: (left: Set<String>) -> Unit) {
        // The older go says what it left before the new one's address becomes the one that is answered.
        live?.broke()
        way = Way(token, targets.toSet(), everywhere)
        val go = WipeGo(targets, token, word, surface) { left ->
            if (way?.token == token) {
                way = null
                live = null
            }
            done(left)
        }
        live = go
        go.start()
    }

    /** Ends the go on its way, if there is one, as a web view that is lost ends it: what it had not cleared is left. */
    fun stop() {
        live?.broke()
    }
}

/**
 * What the end of a session still owes of what the web view holds, as plain
 * state, so it can be tested without a device: whether anything is owed, how
 * much of it (the whole of the end of a session, or only what pages stored),
 * the origins a go left, whether something is being done about it, and the two
 * files that tell the next run of the app ([folder] says where they are; null
 * while the app is not known).
 *
 * Each end of a session has a number ([owe]), and what is done for one end
 * says nothing about a later one: [narrow] and [over] name the end they were
 * begun for, and are dropped when a later one has come. Asking whether a page
 * is held back ([held]) changes nothing and starts nothing: a go begins only
 * where one is asked for ([begin]).
 *
 * [SiteWipe] holds the app's one, under its lock: nothing here guards itself
 * against two threads.
 */
internal class WipeDebt(private val folder: () -> File?) {
    /** Whether a deletion is owed. */
    var owed = false
        private set

    /** While a deletion is owed: whether it is the whole of the end of a session, the cookies and the rest with what pages stored. */
    var all = false
        private set

    /** Whether something is being done about what is owed: the end of a session's own deletion, or a go. */
    var running = false
        private set

    /** While a deletion is owed: the origins a go left, or null while no go has ended and every origin is still to clear. */
    var left: Set<String>? = null
        private set

    /** The number of the last end of a session. It grows with every deletion that becomes owed, so that what was begun for an earlier one says nothing about this one. */
    var number = 0L
        private set

    private var seen: MutableSet<String>? = null

    /** Reads whether an earlier run of the app left a deletion owed, how much of it, and for which origins. */
    fun read() {
        if (owed) return
        val mark = File(folder() ?: return, OWED)
        if (!mark.exists()) return
        owed = true
        // A mark that cannot be read owes everything.
        val text = runCatching { mark.readText() }.getOrDefault(WipeRules.EVERYTHING)
        all = WipeRules.everything(text)
        // The origins an earlier go left, where the mark names them; a mark that names none owes every origin.
        left = WipeRules.listed(text).toSet().ifEmpty { null }
    }

    /** A document of the app is on [origin]: it is one to clear, written to the file before this returns. */
    fun saw(origin: String) {
        val folder = folder() ?: return
        val known = seen ?: load(folder).also { seen = it }
        // The whole list is written again, in one step. One that could not be written is not known, so the next sighting writes it.
        if (known.add(origin) && runCatching { WipeRules.put(File(folder, SEEN), WipeRules.lines(known)) }.isFailure) known.remove(origin)
    }

    private fun load(folder: File): MutableSet<String> =
        runCatching { WipeRules.listed(File(folder, SEEN).readText()).toCollection(LinkedHashSet()) }.getOrElse { LinkedHashSet() }

    /**
     * A session ended: the whole of what that deletes is owed from now, what
     * its pages stored at every origin with it, and the deletion is on its
     * way. The mark has reached the disk when this returns. Answers the
     * number of this end of a session.
     */
    fun owe(): Long {
        owed = true
        all = true
        running = true
        left = null
        number++
        folder()?.let { runCatching { WipeRules.put(File(it, OWED), WipeRules.EVERYTHING + "\n") } }
        return number
    }

    /**
     * Another person is signed in than the one before, with no sign-out
     * between: what the pages of the person before stored is owed from now,
     * at every origin, and never the cookies, which are already the new
     * person's. So the mark owes only what pages stored, and it has reached
     * the disk when this returns. Nothing is on its way yet: a go that was,
     * for something owed before, says nothing about this ([number] moves on),
     * and the one asked for next takes its place. While the whole of an end
     * of a session is owed that stands, since it takes what pages stored with
     * it. Answers whether what is owed changed.
     */
    fun owePages(): Boolean {
        if (owed && all) return false
        owed = true
        all = false
        running = false
        left = null
        number++
        folder()?.let { runCatching { WipeRules.put(File(it, OWED), "") } }
        return true
    }

    /**
     * The cookies, and everything else of the end of the session numbered
     * [of] that is not what pages stored, are gone and on the disk: from now
     * the mark owes only what pages stored at the origins. Dropped when a
     * later session has ended since: the cookies of that one are not gone.
     */
    fun narrow(of: Long) {
        if (of != number || !owed || !all) return
        all = false
        folder()?.let { runCatching { WipeRules.put(File(it, OWED), "") } }
    }

    /**
     * The deletion begun for the end of a session numbered [of] ended. [left]
     * names the origins that did not say they are clean. None: nothing is
     * owed any more. Some: they stay owed and the mark names them, while the
     * pages of every other origin are shown again. Null: nothing could be
     * done at all, and what was owed before is owed still. Dropped when a
     * later session has ended since, whose own deletion is on its way.
     * Answers whether which pages are held back has changed.
     */
    fun over(left: Set<String>?, of: Long): Boolean {
        // A deletion begun before a later end of a session says nothing about what that one owes.
        if (of != number) return false
        running = false
        // While the whole is owed nothing a go says ends it: the cookies come first ([narrow]).
        if (!owed || all || left == null) return false
        val folder = folder()
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
        return true
    }

    /**
     * A go at what pages stored is asked for. It may begin when something is
     * owed, nothing is being done about it, and it is not the whole of the
     * end of a session (whose cookies only its own deletion or the next start
     * takes). Answers the number of the end of a session the go is for, and
     * from then something is being done; null when no go begins.
     */
    fun begin(): Long? {
        if (!owed || running || all) return null
        running = true
        return number
    }

    /** Whether a page on [origin] is held back ([WipeRules.held]): something is owed, and no go has ended yet or the origin is one it left. */
    fun held(origin: String?): Boolean = owed && WipeRules.held(left, origin)

    /**
     * The origins a go is at now: those an earlier go left, or, while none
     * has ended, every one known ([WipeRules.origins]: the app's [own], its
     * web [hosts], and those a document of the app was seen on).
     */
    fun targets(own: List<String>, hosts: List<String>): List<String> {
        val known = seen ?: (folder()?.let(::load) ?: LinkedHashSet()).also { seen = it }
        return left?.toList() ?: WipeRules.origins(own, hosts, known.toList())
    }

    /** Lets go of what is owed without doing it ([SiteWipe.forget]). */
    fun forget() {
        owed = false
        all = false
        running = false
        left = null
        number++
        seen = LinkedHashSet()
        folder()?.let { for (name in listOf(OWED, SEEN)) runCatching { File(it, name).delete() } }
    }

    companion object {
        /** The mark that a deletion is owed, and how much of it. */
        const val OWED = "navree-web-wipe"

        /** The origins a document of the app was seen on. */
        const val SEEN = "navree-web-origins"
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
 * what pages stored at the origins ([finish]). What is owed and what the mark
 * says of it is plain state ([WipeDebt]), kept here under a lock.
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
 * is at the origins that were left. A go begins only there: when a session
 * ends, when another person is signed in with no sign-out between
 * ([owePages]), when the app starts, and when the person asks. Asking whether
 * a page may be shown begins none.
 */
object SiteWipe {
    private val lock = Any()
    private val main = Handler(Looper.getMainLooper())
    private var app: Context? = null
    private var origins: Origins? = null
    private val debt = WipeDebt { app?.noBackupFilesDir }

    /** Grows whenever what is owed changes; kept as state, so a screen that reads [changes] is drawn again when it does. */
    private val changed = mutableIntStateOf(0)

    /** The go on its way: one at a time. */
    private val goes = WipeGoes()

    /** Says which app and which origins this is for, and reads whether an earlier run of the app left a deletion owed, and for which origins. */
    fun arm(context: Context, origins: Origins) {
        val application = context.applicationContext
        synchronized(lock) {
            app = application
            this.origins = origins
            debt.read()
        }
    }

    /** Whether a deletion is owed. */
    val owes: Boolean get() = synchronized(lock) { debt.owed }

    /**
     * A number that is another one whenever what is owed has changed. A web
     * page reads it where it is drawn and asks [shows] again when it differs:
     * so a page that is on the screen when a session ends is taken down at
     * once, with its web view, and one that was held back is shown when the
     * deletion is done.
     */
    val changes: Int get() = changed.intValue

    /**
     * Whether the whole of the end of a session is owed: a run of the app was
     * told that the session ended and did not get as far as the cookies. The
     * client then does all of it again, as at the end of a session, before
     * anything of this run is read, kept or shown ([AndroidEffects]).
     */
    val owesAll: Boolean get() = synchronized(lock) { debt.owed && debt.all }

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
        synchronized(lock) { debt.saw(origin) }
    }

    /**
     * A session ended: the whole of what that deletes is owed from now, what
     * its pages stored at every origin with it, and the deletion is on its
     * way. Called the moment the client is told, on the thread it is told on,
     * before anything is waited for: the mark is a few bytes, and it has
     * reached the disk when this returns. Answers the number of this end of a
     * session, which the deletion begun for it names when it says how far it
     * is ([narrow], [over]).
     */
    fun owe(): Long = synchronized(lock) {
        changed.intValue++
        debt.owe()
    }

    /**
     * Another person is signed in than the one before, with no sign-out
     * between: what the pages of the person before stored in the web view is
     * owed from now, at every origin, and no web page is shown until it is
     * gone. Never the cookies, which are the new person's. The mark has
     * reached the disk when this returns, so a start of the app goes on with
     * what a run did not finish; the deletion itself is [finish], asked for
     * next. Call on the thread the client is driven on.
     */
    fun owePages() {
        synchronized(lock) { if (debt.owePages()) changed.intValue++ }
    }

    /**
     * The cookies, and everything else of the end of the session numbered
     * [of] that is not what pages stored, are gone and on the disk: from now
     * the mark owes only what pages stored at the origins. Says nothing when
     * a later session has ended since. Not on the main thread.
     */
    fun narrow(of: Long) {
        synchronized(lock) { debt.narrow(of) }
    }

    /**
     * The deletion begun for the end of a session numbered [of] ended. [left]
     * names the origins that did not say they are clean. None: nothing is
     * owed any more. Some: they stay owed and the mark names them, while the
     * pages of every other origin are shown again. Null: nothing could be
     * done at all, and what was owed before is owed still. Says nothing when
     * a later session has ended since: that one's own deletion is on its way,
     * and only it says when its pages may be shown. Not on the main thread.
     */
    fun over(left: Set<String>?, of: Long) {
        synchronized(lock) { if (debt.over(left, of)) changed.intValue++ }
    }

    /**
     * Lets go of what is owed without doing it. For a test only, when it is over: a deletion a test that ended a session
     * left unfinished is that test's own failure, not the refusal of every web page the tests after it show.
     */
    internal fun forget() {
        synchronized(lock) {
            debt.forget()
            changed.intValue++
        }
        // A go the test left on its way ends too: while one runs, a service worker's requests are refused, the next test's among them.
        main.post { goes.stop() }
    }

    /**
     * Whether a web page at [address] may be shown: nothing is owed, or a go
     * has ended and the page's origin is not one it left ([WipeRules.held]).
     * Asking changes nothing and begins nothing, however often a screen that
     * is drawn again asks: for a page that is held back, "Try again" is what
     * has another go ([finish]). Any thread.
     */
    fun shows(address: String): Boolean = !held(address)

    /**
     * Whether a document at [address] is held back as a page on its origin
     * is: what the web view's client asks for a frame, and for where a
     * redirect leads. Any thread.
     */
    fun held(address: String): Boolean = synchronized(lock) { debt.held(origins?.originOf(address)) }

    /**
     * Does what is still owed of what pages stored: what the older call
     * covers, while no page has been shown since the deletion became owed,
     * then the documents, at every origin or at those an earlier go left.
     * Never the cookies, which may be a new session's by now: while the whole
     * of the end of a session is owed ([owesAll]) this does nothing, and the
     * next start of the app does all of it. Nor anything while a deletion is
     * on its way already. Asked for at a start of the app that finds
     * something owed, when another person is signed in with no sign-out
     * between, and by "Try again" on a page that is held back. Call on the
     * main thread.
     */
    fun finish() {
        val (context, them, began) = synchronized(lock) {
            val context = app ?: return
            val them = origins ?: return
            val began = debt.begin() ?: return
            Triple(context, them, began)
        }
        try {
            // Once pages are shown again, what the web view holds is a later session's as well, and the older call takes everything: it is not made then.
            if (synchronized(lock) { debt.left } == null) WebStorage.getInstance().deleteAllData()
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
        goes.going?.takeIf { WipeRules.own(url, it) }?.let { answer(200, "OK", "text/html", WipeRules.DOCUMENT) }

    /**
     * Whether a request a service worker makes to [url] is refused because of
     * what is owed: a go is on its way ([WipeGoes.stops]), or the address is
     * on an origin whose pages are held back ([held]), go or no go. A worker
     * the go's document started is the worker of the person who left, and it
     * may outlive the go at an origin the go could not clear: it sends
     * nothing of its own to an origin for as long as that origin's pages are
     * not shown, so nothing of the person who left goes out with the cookies
     * of whoever is signed in by then. Asked where a service worker's
     * requests are asked, after [document]. Any thread.
     */
    internal fun stops(url: String): Boolean = goes.stops(url) || held(url)

    /**
     * Shows each origin to clear the client's own document, one after the
     * other in one web view that is never drawn, and waits for each to say its
     * workers are unregistered and its caches deleted ([WipeGo]). The origins
     * are every one known ([WipeRules.origins]), or those an earlier go left.
     * A go that is still on its way when this one begins ends there
     * ([WipeGoes]): there is one go at a time. [done] is told which origins
     * did not say so; null when the go could not be made (a system web view
     * without the script at the start of a document or the listener, or one
     * that does not take the rules for a service worker's requests). Call on
     * the main thread; [done] runs on it, once.
     */
    fun documents(context: Context, origins: Origins, done: (left: Set<String>?) -> Unit) {
        val application = context.applicationContext
        // Every origin, while no go has ended and every page is held back; after that, the origins a go left.
        val (targets, everywhere) = synchronized(lock) { debt.targets(origins.ownOrigins(), origins.webHostOrigins()) to (debt.left == null) }
        if (targets.isEmpty()) return done(emptySet())
        // The go's address is answered where a service worker's requests are asked as well, and a worker's other requests are
        // refused there while the go runs, so those rules have to be in force.
        if (!able() || !guardWorkers(origins)) return done(null)
        val token = hex()
        goes.start(targets, token, hex(), WebSurface(application, token), everywhere) { left -> done(left) }
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
