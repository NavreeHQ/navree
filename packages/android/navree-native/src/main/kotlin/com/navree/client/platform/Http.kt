package com.navree.client.platform

import android.webkit.CookieManager
import com.navree.client.core.Body
import com.navree.client.core.Effect
import com.navree.client.core.Events
import com.navree.client.core.Part
import com.navree.client.design.FileRules
import kotlinx.serialization.json.JsonObject
import okhttp3.Call
import okhttp3.Callback
import okhttp3.ConnectionSpec
import okhttp3.Cookie
import okhttp3.CookieJar
import okhttp3.Headers
import okhttp3.HttpUrl
import okhttp3.HttpUrl.Companion.toHttpUrlOrNull
import okhttp3.Interceptor
import okhttp3.MediaType.Companion.toMediaTypeOrNull
import okhttp3.MultipartBody
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody
import okhttp3.RequestBody.Companion.toRequestBody
import okhttp3.Response
import okio.BufferedSource
import java.io.File
import java.io.IOException
import java.io.InterruptedIOException
import java.net.ConnectException
import java.net.URLDecoder
import java.net.UnknownHostException
import java.security.MessageDigest
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

/**
 * The cookies the answers to the client's own requests set, so they can be
 * told from the rest of the one store and given back to it. The web view
 * writes to that store too: a page's script sets a cookie, an answer inside
 * a page sets one, a host named for the web view keeps a session of its own,
 * a service worker writes one. None of those passes through here. Where the
 * store is emptied and the person who is signed in is to stay so (another
 * person signed in with no sign-out between; what pages stored is being
 * taken away), only what is remembered here goes back in: the cookies of
 * the sign-in that brought them among it.
 *
 * A cookie is one by its domain, path and name; a later answer's takes the
 * place of an earlier one, and an answer that expires a cookie takes it out.
 * A cookie is given back only where the store still held it with the value
 * the answer set when it was emptied: one a page has changed since is the
 * page's, and is not put back as it was.
 *
 * The store can be emptied and stay empty for a while before its own cookies
 * go back ([takeOut], then [giving] and [giveBack]): what was taken out is
 * remembered as out. What goes back is decided when the store says it is
 * empty, not before: an answer that came in between has set a later cookie in
 * place of an earlier one, or taken one away, and that is what holds.
 *
 * Which cookies these are is kept on the disk as well ([file], in a folder
 * no backup takes), so that a start of the app knows them: for each its
 * name, domain, path and attributes, and a SHA-256 of its value. Never the
 * value: that is in the store, and is read from there when the store is
 * about to be emptied. A sign-out deletes the file with everything else.
 */
internal class OwnCookies(private val file: File? = null, private val now: () -> Long = System::currentTimeMillis) {
    /**
     * One cookie an answer set. [probe] is an address on the cookie's host
     * and path: the store is asked there whether it still holds the cookie,
     * and the cookie is set there again. [rest] is what followed `name=value`
     * in the header that set it, its attributes as the answer gave them.
     * [mark] is a SHA-256 of the value, which is all the disk is told of it.
     */
    class Given internal constructor(val probe: String, val name: String, val rest: String, val domain: String, val path: String, val expiresAt: Long, val mark: String, value: String?) {
        /** As an answer sets it: [header] is the whole of what set it. */
        constructor(probe: String, header: String, name: String, value: String, domain: String, path: String, expiresAt: Long) :
            this(probe, name, restOf(header, name, value), domain, path, expiresAt, markOf(value), value)

        /** The value, while this run of the app knows it: as the answer set it, or as the store held it when it was last asked. */
        var value: String? = value
            internal set

        /** How many cookies answers had set when this one was set, itself among them. */
        internal var at = 0L

        /** What sets the cookie again; null while its value is not known. */
        val header: String? get() = value?.let { "$name=$it$rest" }
    }

    /** Which cookies go back into a store that is being emptied, and how many cookies answers had set when that was decided. */
    class Back internal constructor(internal val which: Set<String>, internal val at: Long)

    private val kept = LinkedHashMap<String, Given>()

    /** The cookies taken out of the store, to go back in: by what tells one from another. */
    private val out = HashSet<String>()
    private var count = 0L

    /** What the file holds, as far as this knows: the same is not written twice. */
    private var written: String? = null

    init {
        load()
    }

    private fun which(cookie: Given) = cookie.domain + "\n" + cookie.path + "\n" + cookie.name

    /**
     * An answer to one of the client's own requests set this cookie. [put]
     * puts it into the store, and runs here so that the store and what is
     * remembered of it never part: no cookie is given back in between.
     */
    @Synchronized
    fun set(cookie: Given, put: () -> Unit = {}) {
        put()
        val which = which(cookie)
        cookie.at = ++count
        kept.remove(which)
        // An answer that expires a cookie takes it away, and it is not one to give back any more.
        if (cookie.expiresAt > now()) {
            kept[which] = cookie
            // A client's own cookies are a handful; the oldest go first where an answer keeps setting new ones.
            while (kept.size > MOST) kept.remove(kept.keys.first())
        }
        save()
    }

    /** The session ended on this device, and every cookie went with it: nothing is remembered, on the disk either. */
    @Synchronized
    fun forget() {
        kept.clear()
        out.clear()
        save()
    }

    /**
     * The cookies the client's own answers set that have not run out and
     * that the store holds as they were set. [held] says what the store holds
     * for an address, as it would send it (`name=value; name=value`), or
     * null. The value of each is known from here on, also of one that was
     * read from the disk.
     */
    @Synchronized
    fun stillHeld(held: (address: String) -> String?): List<Given> {
        val time = now()
        return kept.values.filter { cookie ->
            if (cookie.expiresAt <= time) return@filter false
            val found = pairs(held(cookie.probe)).firstOrNull { (name, value) -> name == cookie.name && markOf(value) == cookie.mark } ?: return@filter false
            cookie.value = found.second
            true
        }
    }

    /**
     * The store is about to be emptied and to stay empty for a while: what
     * it holds of the client's own now ([stillHeld]) is remembered as taken
     * out, to go back in when the store is next given its own.
     */
    @Synchronized
    fun takeOut(held: (address: String) -> String?) {
        for (cookie in stillHeld(held)) out += which(cookie)
    }

    /**
     * The store is about to be emptied and given its own back: which cookies
     * those are. The ones taken out before, and the ones it holds now as the
     * answers set them.
     */
    @Synchronized
    fun giving(held: (address: String) -> String?): Back = Back(out + stillHeld(held).map(::which), count)

    /**
     * The store is empty now: its own cookies go back in, through [put].
     * Those decided on ([back]) as they are by now, so a later answer's
     * cookie and not the one it took the place of, and none an answer has
     * taken away since; and with them whatever answers have set since it was
     * decided, which the emptying took with the rest. From now those are the
     * client's own cookies, and none is out. Answers what went back. What is
     * remembered is written to the disk by [save], which the caller sees to
     * off the main thread.
     */
    @Synchronized
    fun giveBack(back: Back, put: (Given) -> Unit): List<Given> {
        val time = now()
        val given = kept.values.filter { cookie -> cookie.expiresAt > time && cookie.value != null && (which(cookie) in back.which || cookie.at > back.at) }
        for (cookie in given) put(cookie)
        out.clear()
        val went = given.toSet()
        kept.values.retainAll { it in went }
        return given
    }

    /**
     * Writes which cookies are remembered to the disk, where that differs
     * from what is there: one line for each, and no value in any. A file
     * that could not be written leaves the next start knowing less, and a
     * cookie it does not know is not given back.
     */
    @Synchronized
    fun save() {
        val target = file ?: return
        val text = kept.values.joinToString("") { cookie ->
            listOf(cookie.probe, cookie.name, cookie.rest, cookie.domain, cookie.path, cookie.expiresAt.toString(), cookie.mark).joinToString("\t") { escaped(it) } + "\n"
        }
        if (text == written) return
        try {
            if (text.isEmpty()) target.delete() else WipeRules.put(target, text, sync = false)
            written = text
        } catch (_: Throwable) {
            // Tried again the next time something changes.
            written = null
        }
    }

    /** Reads what an earlier run of the app remembered. A line that does not read is left out; a cookie that has run out, too. */
    private fun load() {
        val target = file ?: return
        val text = try {
            if (target.isFile) target.readText() else return
        } catch (_: Throwable) {
            return
        }
        val time = now()
        for (row in text.lineSequence()) {
            val fields = row.split('\t')
            if (fields.size != 7) continue
            val expiresAt = fields[5].toLongOrNull() ?: continue
            val cookie = Given(plain(fields[0]), plain(fields[1]), plain(fields[2]), plain(fields[3]), plain(fields[4]), expiresAt, fields[6], null)
            if (cookie.probe.isEmpty() || cookie.name.isEmpty() || !MARK.matches(cookie.mark) || expiresAt <= time) continue
            cookie.at = ++count
            kept[which(cookie)] = cookie
        }
        while (kept.size > MOST) kept.remove(kept.keys.first())
    }

    private fun pairs(header: String?): List<Pair<String, String>> =
        header.orEmpty().split(';').mapNotNull { part ->
            val at = part.indexOf('=')
            if (at <= 0) null else part.substring(0, at).trim() to part.substring(at + 1).trim()
        }

    companion object {
        /** The name of the file, in the folder no backup takes. */
        const val FILE = "navree-own-cookies"

        private const val MOST = 200
        private val MARK = Regex("[0-9a-f]{64}")

        /** What follows `name=value` in a header that sets a cookie: its attributes, each with the `;` before it. */
        internal fun restOf(header: String, name: String, value: String): String {
            val first = "$name=$value"
            if (header.startsWith(first)) return header.substring(first.length)
            val at = header.indexOf(';')
            return if (at < 0) "" else header.substring(at)
        }

        /** A SHA-256 of a cookie's value, as hex digits. */
        internal fun markOf(value: String): String =
            MessageDigest.getInstance("SHA-256").digest(value.encodeToByteArray()).joinToString("") { "%02x".format(it) }

        /** A field as one line of the file holds it: no tab and no line break in it. */
        private fun escaped(field: String): String = field.replace("%", "%25").replace("\t", "%09").replace("\n", "%0A").replace("\r", "%0D")

        private fun plain(field: String): String = field.replace("%09", "\t").replace("%0A", "\n").replace("%0D", "\r").replace("%25", "%")
    }
}

/**
 * The session's cookies, kept in the system's own store for web views: the one
 * store the native requests and the web view both read and write, so a page
 * that falls back to the web view is signed in, and signing out clears both.
 */
class WebViewCookies(
    private val manager: () -> CookieManager = { CookieManager.getInstance() },
    /** Where it is kept, across starts of the app, which cookies the answers to the client's own requests set ([OwnCookies]); null to keep that for this run only. */
    kept: File? = null,
) : CookieJar {
    /** The cookies the answers to the client's own requests set: what is given back when the store is emptied and the person is to stay signed in. */
    internal val own = OwnCookies(kept)

    /**
     * Told each time an answer to one of the client's own requests is about
     * to put a cookie into the store: from then the store may hold somebody's
     * session again. On the thread the cookie is saved on, before it is.
     */
    @Volatile
    var onOwn: (() -> Unit)? = null

    override fun saveFromResponse(url: HttpUrl, cookies: List<Cookie>) {
        if (cookies.isNotEmpty()) onOwn?.invoke()
        val store = manager()
        store.setAcceptCookie(true)
        for (cookie in cookies) {
            val header = cookie.toString()
            // The store is asked about a cookie, and given it back, at an address on the cookie's own host and path, which need not
            // be the answer's; nothing else of the answer's address is remembered.
            val probe = runCatching { url.newBuilder().encodedPath(cookie.path).query(null).fragment(null).build() }.getOrElse { url.newBuilder().encodedPath("/").query(null).fragment(null).build() }.toString()
            own.set(OwnCookies.Given(probe, header, cookie.name, cookie.value, cookie.domain, cookie.path, cookie.expiresAt)) { store.setCookie(url.toString(), header) }
        }
        store.flush()
    }

    override fun loadForRequest(url: HttpUrl): List<Cookie> {
        val header = manager().getCookie(url.toString()) ?: return emptyList()
        return header.split(';').mapNotNull { Cookie.parse(url, it.trim()) }
    }

    /**
     * Every cookie of the store gone: the session ended on this device. Call
     * on a thread with a looper (the main thread): [done] runs on it once the
     * store says they are gone.
     */
    fun clear(done: () -> Unit = {}) {
        own.forget()
        manager().removeAllCookies { done() }
    }

    /**
     * Every cookie of the store goes, and none comes back yet: what pages
     * stored at the origins is about to be taken away through a document on
     * each, which starts the service workers of the person before, and
     * whatever such a worker sends, by whatever way, it sends with no session
     * to carry. The cookies the answers to the client's own requests set are
     * remembered as taken out ([OwnCookies.takeOut]) and go back in with
     * [keepOwn].
     *
     * Waits for the store to say its cookies are gone, no longer than
     * [waitMs], and answers whether it said so in time. Not on the main
     * thread, and in the store's turn.
     */
    fun takeOut(onLooper: (() -> Unit) -> Unit, waitMs: Long): Boolean {
        // No web view on this device: no cookie store, and nothing in it.
        val store = try {
            manager()
        } catch (_: Throwable) {
            return true
        }
        own.takeOut { address -> store.getCookie(address) }
        val done = CountDownLatch(1)
        onLooper { store.removeAllCookies { done.countDown() } }
        return done.await(waitMs, TimeUnit.MILLISECONDS)
    }

    /**
     * Every cookie of the store goes: what a page's script set, what an
     * answer inside a page set, what a service worker wrote, the cookies of
     * every host named for the web view. Then the cookies the answers to the
     * client's own requests set are given back, each as it was set
     * ([OwnCookies.giveBack]): those the store held so, and those taken out
     * before ([takeOut]). The sign-in that brought the person who is here now
     * is among them, so they stay signed in.
     *
     * The store is emptied on a thread with a looper ([onLooper] runs a piece
     * of work on one), and its cookies are given back there the moment it
     * says they are gone. This waits for that, no longer than [waitMs], then
     * writes the store to the disk, and answers whether it was done in time.
     * A store that answers later is given its cookies back then all the same,
     * as the answers have left them by then. Not on the main thread, and in
     * the store's turn, so no fetch or stream of the client's own reads the
     * store in between.
     */
    fun keepOwn(onLooper: (() -> Unit) -> Unit, waitMs: Long): Boolean {
        // No web view on this device: no cookie store, and nothing in it.
        val store = try {
            manager()
        } catch (_: Throwable) {
            return true
        }
        val back = own.giving { address -> store.getCookie(address) }
        val done = CountDownLatch(1)
        onLooper {
            store.removeAllCookies {
                own.giveBack(back) { cookie -> cookie.header?.let { store.setCookie(cookie.probe, it) } }
                done.countDown()
            }
        }
        if (!done.await(waitMs, TimeUnit.MILLISECONDS)) return false
        // What is remembered now is what went back: written down here, off the main thread.
        own.save()
        store.flush()
        return true
    }

    /** Writes the store to disk now. It blocks while it does: not on the main thread. */
    fun flush() {
        manager().flush()
    }
}

/** The `Cookie` header the store has for an address, or null when it has none. */
fun cookieHeader(jar: CookieJar, url: HttpUrl): String? =
    jar.loadForRequest(url).takeIf { it.isNotEmpty() }?.joinToString("; ") { "${it.name}=${it.value}" }

/** The number of the session a request was made for (see [StoredState]): its cookies are kept only for that session. */
data class Session(val number: Long)

/**
 * Keeps the cookies an answer sets, in the store's turn and only while the
 * session the request was made for is still the one on the device: a late
 * answer of a session that has ended brings none of its cookies back.
 */
class SessionCookies(private val state: StoredState, private val jar: CookieJar) : Interceptor {
    override fun intercept(chain: Interceptor.Chain): Response {
        val request = chain.request()
        val response = chain.proceed(request)
        val session = request.tag(Session::class.java) ?: return response
        val cookies = Cookie.parseAll(request.url, response.headers)
        if (cookies.isNotEmpty()) state.write(session.number) { jar.saveFromResponse(request.url, cookies) }
        return response
    }
}

/**
 * The value an `x-xsrf-token` header carries: the `XSRF-TOKEN` cookie for the
 * address, percent-decoded. Null when there is none.
 */
fun xsrfToken(jar: CookieJar, url: HttpUrl): String? =
    jar.loadForRequest(url).firstOrNull { it.name == "XSRF-TOKEN" }?.value?.let {
        runCatching { URLDecoder.decode(it, "UTF-8") }.getOrDefault(it)
    }

/**
 * The most bytes the body of one answer may have: 8 MiB. A page is the tree of
 * one screen, with long lists loaded a page at a time, so the largest answers
 * seen are tens of kilobytes; a body is held as bytes, as text and once more
 * inside the event to the core, so 8 MiB stays well inside the memory a phone
 * gives an app. A longer body is answered as a failed request.
 */
const val MOST_BODY_BYTES: Long = 8L * 1024 * 1024

/**
 * The connections the client makes: TLS 1.2 or 1.3 with the modern cipher
 * suites only, and clear text only where the app allows plain `http` to the
 * machine itself (each address is checked again by [Origins.ours] before it
 * is asked).
 */
fun connectionSpecs(loopbackHttp: Boolean): List<ConnectionSpec> =
    if (loopbackHttp) listOf(ConnectionSpec.MODERN_TLS, ConnectionSpec.CLEARTEXT) else listOf(ConnectionSpec.MODERN_TLS)

/**
 * The request a `fetch` effect asks for, with the store's cookies as they are
 * now, or null when it cannot be sent: an address that is not one of the
 * app's origins, a header value HTTP cannot carry (a line break, a character
 * outside ASCII), a method that cannot have the body it was given. The caller
 * answers null as a failed request.
 */
fun fetchRequest(
    effect: Effect.Fetch,
    origins: Origins,
    cookies: CookieJar,
    files: (String) -> RequestBody? = { null },
    session: Long? = null,
): Request? = try {
    val url = effect.url.takeIf(origins::ours)?.toHttpUrlOrNull()
    if (url == null) {
        null
    } else {
        val builder = Request.Builder().url(url)
        for ((name, value) in effect.headers) builder.header(name, value)
        cookieHeader(cookies, url)?.let { builder.header("Cookie", it) }
        if (effect.xsrf) xsrfToken(cookies, url)?.let { builder.header("x-xsrf-token", it) }
        session?.let { builder.tag(Session::class.java, Session(it)) }
        val method = effect.method.uppercase()
        val type = effect.headers.firstOrNull { it.first.equals("content-type", ignoreCase = true) }?.second
        val body: RequestBody? = when (val sent = effect.body) {
            is Body.Form -> sent.text.toRequestBody(type?.toMediaTypeOrNull())
            is Body.Json -> sent.text.toRequestBody(type?.toMediaTypeOrNull())
            is Body.Multipart -> MultipartBody.Builder().setType(MultipartBody.FORM).apply {
                for (part in sent.parts) {
                    when (part) {
                        is Part.Text -> addFormDataPart(part.name, part.value)
                        // The file's name as the client shows it: never a path, no control or format character. OkHttp
                        // writes it quoted, with a line break and a quote written as %0D, %0A and %22.
                        is Part.File -> files(part.token)?.let { addFormDataPart(part.name, FileRules.displayName(part.filename), it) }
                    }
                }
            }.build()
            null -> if (method == "GET" || method == "HEAD") null else ByteArray(0).toRequestBody(type?.toMediaTypeOrNull())
        }
        if (body is MultipartBody) builder.removeHeader("content-type")
        builder.method(method, body)
        builder.build()
    }
} catch (_: Exception) {
    null
}

/**
 * The headers of an answer the core is given, names in lower case and values
 * as they came: every `x-navree-*` header, and `cache-control`, by which the
 * core decides whether a page may be kept on the device. No other header.
 */
fun passedHeaders(headers: Headers): List<Pair<String, String>> =
    headers.names()
        .filter { it.startsWith("x-navree-", ignoreCase = true) || it.equals("cache-control", ignoreCase = true) }
        .flatMap { name -> headers.values(name).map { name.lowercase() to it } }

/**
 * The bytes of a body, or null when it has more than [most]: read only as far
 * as one byte past the limit, so a body that never ends costs no more.
 */
fun readCapped(source: BufferedSource, most: Long): ByteArray? {
    if (source.request(most + 1)) return null
    return source.readByteArray()
}

/**
 * Refuses, before any connection is made, a request to an address that is
 * not one of the app's origins: the last word on every request the client's
 * own connections make, whoever built it.
 */
class OriginGuard(private val allowed: (String) -> Boolean) : Interceptor {
    override fun intercept(chain: Interceptor.Chain): Response {
        val url = chain.request().url.toString()
        if (!allowed(url)) throw IOException("not one of the app's origins")
        return chain.proceed(chain.request())
    }
}

/**
 * Carries out the core's `fetch` effects with OkHttp: exactly the headers it
 * gives, the cookie store's cookies, no redirect followed (a 3xx goes back to
 * the core as it came, after its cookies are stored), and the answer handed
 * back whatever its status, with the `x-navree-*` headers and `cache-control`.
 *
 * The core decides which addresses are asked; the platform checks each again
 * ([Origins.ours]) and answers one it refuses, or a request that cannot be
 * built, as a failed request, the failure the core already understands.
 *
 * Each request reads its cookies in the store's turn ([StoredState]), so one
 * the core asked for before the end of a session goes out with that session's
 * cookies, and one asked for after goes out only once the wipe has finished.
 */
/**
 * Whether an answer is the last a write that carried files will get, so the
 * copies of its files can be let go: an answer under 400 that is neither a 307
 * nor a 308. Those two the core answers by asking again with the same body,
 * the same files in it (`follow` in the core's `reduce.rs`), and the files must
 * still be there for it; after a 301, 302 or 303 it only reads. A refusal (400
 * and over) and a failure keep the copies too: the form stays on the screen
 * with its files, for the person to send again.
 */
internal fun writeEnded(status: Int): Boolean = status < 400 && status != 307 && status != 308

/**
 * The requests the core has asked for whose turn in the store has not come.
 * The core may cancel a request at any time: before its turn, and it is then
 * not sent; while it is on its way, which is the connection's to end; after
 * it was answered; or, at the end of a session, one this client was never
 * given. Only the first is anything to remember, and only until the turn
 * comes: a cancel that finds nothing to cancel keeps nothing.
 */
internal class Unsent {
    private val waiting = ConcurrentHashMap.newKeySet<Long>()

    /** The core asked for [request]: it waits for its turn. */
    fun asked(request: Long) {
        waiting.add(request)
    }

    /** The turn of [request] has come: whether it is still to be sent. Its turn comes once. */
    fun due(request: Long): Boolean = waiting.remove(request)

    /** The core cancels [request]: if its turn has not come, it is not sent when it does. */
    fun cancel(request: Long) {
        waiting.remove(request)
    }
}

class CoreHttp(
    val cookies: CookieJar,
    private val origins: Origins,
    loopbackHttp: Boolean = false,
    private val state: StoredState = StoredState { it.run() },
    private val files: (String) -> RequestBody? = { null },
    /** The tokens of the files a request carried, once the write has its last answer ([writeEnded]). */
    private val sent: (List<String>) -> Unit = {},
    /** Where an answer goes: posted to the thread the core is driven on. */
    private val answer: (JsonObject) -> Unit,
) {
    val client: OkHttpClient = OkHttpClient.Builder()
        // Cookies are read in the store's turn when a request is made, and kept by SessionCookies.
        .cookieJar(CookieJar.NO_COOKIES)
        .followRedirects(false)
        .followSslRedirects(false)
        .connectionSpecs(connectionSpecs(loopbackHttp))
        .addInterceptor(OriginGuard(origins::mayLoad))
        .addInterceptor(SessionCookies(state, cookies))
        .connectTimeout(15, TimeUnit.SECONDS)
        .readTimeout(60, TimeUnit.SECONDS)
        .build()

    private val calls = ConcurrentHashMap<Long, Call>()

    /** The requests asked for whose turn has not come: one the core cancels meanwhile is not sent. */
    private val unsent = Unsent()

    fun fetch(effect: Effect.Fetch) {
        val session = state.generation
        unsent.asked(effect.request)
        state.always {
            if (unsent.due(effect.request)) send(effect, session)
        }
    }

    private fun send(effect: Effect.Fetch, session: Long) {
        val request = fetchRequest(effect, origins, cookies, files, session)
        val call = try {
            request?.let { client.newCall(it).apply { timeout().timeout(effect.timeoutMs, TimeUnit.MILLISECONDS) } }
        } catch (_: Exception) {
            null
        }
        if (call == null) {
            answer(Events.responseFailed(effect.request, "other"))
            return
        }
        calls[effect.request] = call
        call.enqueue(
            object : Callback {
                override fun onFailure(call: Call, e: IOException) {
                    calls.remove(effect.request)
                    if (call.isCanceled()) return
                    answer(Events.responseFailed(effect.request, failure(e)))
                }

                override fun onResponse(call: Call, response: Response) {
                    calls.remove(effect.request)
                    val event = try {
                        response.use { read(effect.request, it) }
                    } catch (e: IOException) {
                        if (call.isCanceled()) return
                        Events.responseFailed(effect.request, failure(e))
                    } catch (_: Exception) {
                        Events.responseFailed(effect.request, "other")
                    }
                    // The write has its last answer: the copies of the files it carried are let go.
                    if (writeEnded(response.code)) fileTokens(effect).takeIf { it.isNotEmpty() }?.let(sent)
                    answer(event)
                }
            },
        )
    }

    /**
     * The core cancels a request. One on its way is ended; one whose turn has
     * not come is not sent. One that was answered already, or that this
     * client was never asked for (the core cancels what it asked while a
     * session was ending), is nothing to cancel, and nothing is kept of it.
     */
    fun cancel(request: Long) {
        val call = calls.remove(request)
        if (call == null) unsent.cancel(request) else call.cancel()
    }

    private fun read(request: Long, response: Response): JsonObject {
        val headers = passedHeaders(response.headers)
        val redirect = response.code in REDIRECTS
        val body = if (redirect) {
            ""
        } else {
            val bytes = if (response.body.contentLength() > MOST_BODY_BYTES) null else readCapped(response.body.source(), MOST_BODY_BYTES)
            bytes ?: return Events.responseFailed(request, "other")
            String(bytes, response.body.contentType()?.charset(Charsets.UTF_8) ?: Charsets.UTF_8)
        }
        return Events.response(
            request = request,
            status = response.code,
            contentType = response.header("Content-Type"),
            location = if (redirect) response.header("Location") else null,
            headers = headers,
            body = body,
        )
    }

    private fun fileTokens(effect: Effect.Fetch): List<String> =
        (effect.body as? Body.Multipart)?.parts?.filterIsInstance<Part.File>()?.map { it.token }.orEmpty()

    private fun failure(e: IOException): String = when (e) {
        is UnknownHostException, is ConnectException -> "offline"
        is InterruptedIOException -> "timeout"
        else -> "other"
    }

    private companion object {
        val REDIRECTS = setOf(301, 302, 303, 307, 308)
    }
}
