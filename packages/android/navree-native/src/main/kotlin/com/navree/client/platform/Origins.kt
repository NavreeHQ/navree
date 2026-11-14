package com.navree.client.platform

import okhttp3.HttpUrl
import okhttp3.HttpUrl.Companion.toHttpUrlOrNull
import java.util.Locale

/**
 * The app's origins: the one it was built for, and the others its
 * configuration allows. The client asks [ours] of every address it is about
 * to fetch, stream, show in the web view or load a picture from, whatever the
 * core said of it: the answer is the platform's own, read from the whole
 * address as it will be sent.
 *
 * An origin of [also] whose host starts with `*.` stands for every host under
 * that name (a host `*.shop.example` covers `patiala.shop.example`), never for
 * a name that merely ends in the same letters. Plain `http` is an
 * origin only for the machine itself (`localhost`, `127.0.0.1`, `[::1]`, and
 * `10.0.2.2`, the emulator's name for the machine it runs on), and only when
 * [loopbackHttp] says so, which a release build never does.
 *
 * ```
 * val origins = Origins("https://shop.example")
 * origins.ours("https://shop.example/orders") // true
 * origins.ours("https://shop.example//other.example/x") // false
 * ```
 */
class Origins(
    origin: String,
    also: List<String> = emptyList(),
    loopbackHttp: Boolean = false,
    /** Hosts pictures may also come from, over `https`, such as `images.shop.example` or `*.cdn.example`. */
    pictureHosts: List<String> = emptyList(),
    /**
     * Hosts that may serve what a page in the web view loads inside it
     * (scripts, styles, fonts, pictures, frames), never the page itself and
     * never a native request: `pay.example`, `*.cdn.example`, with a `:port`
     * where it is not 443. Over `https`; plain `http` only to the machine
     * itself, as for the origins.
     */
    webHosts: List<String> = emptyList(),
) {
    private val loopback = loopbackHttp
    private val home: Site? = Site.read(origin)?.takeIf { it.allowed(loopbackHttp) && it.bare }
    private val others: List<Site> = also.mapNotNull { text -> Site.read(text, wildcard = true)?.takeIf { it.allowed(loopbackHttp) && it.bare } }
    private val pictures: List<String> = pictureHosts.mapNotNull { host ->
        host.lowercase(Locale.ROOT).takeIf { Site.hostReads(it, wildcard = true) }
    }
    private val web: List<Pair<String, Int?>> = webHosts.mapNotNull { hostRule(it) }
    private val base: HttpUrl? = home?.let { origin.toHttpUrlOrNull() }

    /** Whether the configured origin can be used at all. */
    val usable: Boolean get() = home != null

    /**
     * Whether this whole address is the app's own: an `http` or `https`
     * address with no whitespace, control character or backslash anywhere, a
     * host of ASCII letters, digits, `-` and `.` (or a bracketed IPv6
     * literal), no user name, a path that does not begin with `//`, and the
     * scheme, host and port of the app's origin or of one it allows.
     */
    fun ours(address: String): Boolean {
        val site = Site.read(address) ?: return false
        if (site.scheme == "http" && site.host !in LOOPBACK) return false
        val built = home ?: return false
        return site.sameOrigin(built) || others.any { it.covers(site) }
    }

    /**
     * The whole address a picture loads from, read against the app's origin,
     * or null when it may not load: pictures come from the app's origins and,
     * over `https` only, from the hosts the configuration names.
     */
    fun picture(href: String): String? {
        if (href.isEmpty() || !clean(href)) return null
        val resolved = base?.resolve(href)?.toString() ?: return null
        return resolved.takeIf(::mayLoad)
    }

    /**
     * Whether the client's own connections may ask this whole address: one
     * of the app's origins, or a picture host over `https`.
     */
    fun mayLoad(address: String): Boolean = ours(address) || (Site.read(address)?.let { site ->
        site.scheme == "https" && site.port == 443 && pictures.any { Site.hostCovers(it, site.host) }
    } ?: false)

    /**
     * Whether this whole address is on a host of [webHosts]: what a page in
     * the web view may load inside it beside the app's own origins. Never
     * asked of a page itself, a native request, a stream or a picture.
     */
    fun webHost(address: String): Boolean {
        val site = Site.read(address) ?: return false
        val secure = site.scheme == "https"
        if (!secure && !(loopback && site.host in LOOPBACK)) return false
        return web.any { (host, port) -> Site.hostCovers(host, site.host) && site.port == (port ?: if (secure) 443 else 80) }
    }

    /**
     * The origins a document in the web view may have, as `[scheme, host,
     * port]` (a host may start with `*.`): the app's own and those of
     * [webHosts]. What the web view's document-start check compares with.
     */
    fun documentOrigins(): List<Triple<String, String, Int>> = buildList {
        for (site in listOfNotNull(home) + others) add(Triple(site.scheme, site.host, site.port))
        for ((host, port) in web) {
            add(Triple("https", host, port ?: 443))
            if (loopback && host in LOOPBACK) add(Triple("http", host, port ?: 80))
        }
    }

    internal companion object {
        val LOOPBACK = setOf("localhost", "127.0.0.1", "10.0.2.2", "[::1]")

        /** A host, or a `*.` pattern, with an optional `:port`; null when it does not read. */
        fun hostRule(text: String): Pair<String, Int?>? {
            val entry = text.trim().lowercase(Locale.ROOT)
            if (entry.isEmpty() || !clean(entry)) return null
            val colon = if (entry.startsWith("[")) entry.indexOf("]:").let { if (it < 0) -1 else it + 1 } else entry.lastIndexOf(':')
            val host = if (colon < 0) entry else entry.substring(0, colon)
            val portText = if (colon < 0) null else entry.substring(colon + 1)
            val reads = host in LOOPBACK || Site.hostReads(host, wildcard = true)
            if (!reads) return null
            val port = portText?.let { text -> text.takeIf { it.isNotEmpty() && it.length <= 5 && it.all { c -> c in '0'..'9' } }?.toInt()?.takeIf { it in 1..65535 } ?: return null }
            return host to port
        }
    }
}

/** Schemes the system may be asked to open an address with. */
private val HANDED_TO_SYSTEM = setOf("http", "https", "mailto", "tel", "sms", "geo")

/**
 * Whether an address the core hands out may be given to the system to open:
 * `http`, `https`, `mailto`, `tel`, `sms` or `geo`, written cleanly, and a web
 * address whose parts read.
 */
fun handedToSystem(address: String): Boolean {
    if (address.isEmpty() || !clean(address)) return false
    val scheme = schemeOf(address) ?: return false
    if (scheme !in HANDED_TO_SYSTEM) return false
    return scheme !in setOf("http", "https") || Site.read(address) != null
}

/**
 * Whether an address may open in the system's browser inside the app (a
 * Custom Tab): a web address whose parts read, over `https`, or plain `http`
 * to the machine itself.
 */
fun webAddress(address: String): Boolean {
    val site = Site.read(address) ?: return false
    return site.scheme == "https" || site.host in Origins.LOOPBACK
}

/**
 * Whether a link the app was opened with can be the return of the sign-in
 * session that is open: for a `callback` that is a whole web address, the
 * same scheme, host, port and path (any query); for a callback that is a
 * scheme, or an address of the app's own scheme, that scheme; with no callback, an address of the app's origins. Any other
 * link is an ordinary link, even while a sign-in is open.
 */
fun signInReturn(link: String, callback: String?, origins: Origins): Boolean {
    if (link.isEmpty() || !clean(link)) return false
    if (callback.isNullOrEmpty()) return origins.ours(link)
    // A scheme alone (`shop`), or an address of an app's own scheme (`com.charkha.supply:/signed-in`).
    if (!callback.contains("://")) return schemeOf(link) == (schemeOf(callback) ?: callback.lowercase(Locale.ROOT))
    val wanted = Site.read(callback) ?: return false
    val got = Site.read(link) ?: return false
    return got.sameOrigin(wanted) && pathOf(link) == pathOf(callback)
}

/** The path of a whole address, without its query or fragment; `/` when it has none. */
private fun pathOf(address: String): String {
    val rest = address.substringAfter("://")
    val start = rest.indexOfAny(charArrayOf('/', '?', '#'))
    if (start < 0 || rest[start] != '/') return "/"
    return rest.substring(start).substringBefore('?').substringBefore('#')
}

/** No whitespace, control or format character and no backslash anywhere. */
internal fun clean(text: String): Boolean = text.none { c ->
    c.isWhitespace() || c.isISOControl() || c == '\\' || Character.getType(c) == Character.FORMAT.toInt()
}

/** The scheme an address starts with, in lower case, when it has one. */
internal fun schemeOf(text: String): String? {
    val colon = text.indexOf(':')
    if (colon <= 0) return null
    val scheme = text.substring(0, colon)
    val first = scheme[0]
    val rest = scheme.drop(1)
    val reads = (first in 'a'..'z' || first in 'A'..'Z') &&
        rest.all { it in 'a'..'z' || it in 'A'..'Z' || it in '0'..'9' || it == '+' || it == '.' || it == '-' }
    return if (reads) scheme.lowercase(Locale.ROOT) else null
}

/** The scheme, host and port of a whole web address, read strictly: what does not read is refused, never repaired. */
internal data class Site(
    val scheme: String,
    val host: String,
    val port: Int,
    /** No path, query or fragment beyond a lone `/`: what an origin in the configuration is. */
    val bare: Boolean = true,
) {
    fun allowed(loopbackHttp: Boolean): Boolean = scheme == "https" || (scheme == "http" && loopbackHttp && host in Origins.LOOPBACK)

    fun sameOrigin(other: Site): Boolean = scheme == other.scheme && host == other.host && port == other.port

    /** Whether this origin of the configuration covers [site]. */
    fun covers(site: Site): Boolean = scheme == site.scheme && port == site.port && hostCovers(host, site.host)

    companion object {
        fun read(text: String, wildcard: Boolean = false): Site? {
            if (text.isEmpty() || !clean(text)) return null
            val split = text.indexOf("://")
            if (split <= 0) return null
            val scheme = text.substring(0, split).lowercase(Locale.ROOT)
            if (scheme != "http" && scheme != "https") return null
            val rest = text.substring(split + 3)
            val end = rest.indexOfAny(charArrayOf('/', '?', '#')).let { if (it < 0) rest.length else it }
            val authority = rest.substring(0, end)
            val tail = rest.substring(end)
            if (authority.isEmpty() || '@' in authority || tail.startsWith("//")) return null
            val host: String
            val portText: String?
            if (authority.startsWith('[')) {
                val close = authority.indexOf(']')
                if (close < 0) return null
                host = authority.substring(0, close + 1)
                val inner = host.substring(1, host.length - 1)
                if (inner.isEmpty() || !inner.all { it.isHexDigitAscii() || it == ':' || it == '.' }) return null
                val after = authority.substring(close + 1)
                portText = when {
                    after.isEmpty() -> null
                    after.startsWith(':') -> after.substring(1)
                    else -> return null
                }
            } else {
                val colon = authority.indexOf(':')
                host = if (colon < 0) authority else authority.substring(0, colon)
                portText = if (colon < 0) null else authority.substring(colon + 1)
                if (!hostReads(host.lowercase(Locale.ROOT), wildcard)) return null
            }
            val port = when (portText) {
                null -> if (scheme == "http") 80 else 443
                else -> {
                    if (portText.isEmpty() || portText.length > 5 || !portText.all { it in '0'..'9' }) return null
                    portText.toInt().takeIf { it in 1..65535 } ?: return null
                }
            }
            return Site(scheme, host.lowercase(Locale.ROOT), port, bare = tail.isEmpty() || tail == "/")
        }

        /** A host name of ASCII letters, digits and `-`, in labels that are not empty; `*.` in front only where a pattern is allowed. */
        fun hostReads(host: String, wildcard: Boolean): Boolean {
            val name = if (wildcard && host.startsWith("*.")) host.substring(2) else host
            if (name.isEmpty()) return false
            return name.split('.').all { label -> label.isNotEmpty() && label.all { it in 'a'..'z' || it in '0'..'9' || it == '-' } }
        }

        /** Whether a host, or a `*.` pattern, covers [host]: under the name, never a name that merely ends in its letters. */
        fun hostCovers(pattern: String, host: String): Boolean = if (pattern.startsWith("*.")) {
            val under = pattern.substring(1)
            host.endsWith(under) && host.length > under.length
        } else {
            pattern == host
        }

        private fun Char.isHexDigitAscii() = this in '0'..'9' || this in 'a'..'f' || this in 'A'..'F'
    }
}
