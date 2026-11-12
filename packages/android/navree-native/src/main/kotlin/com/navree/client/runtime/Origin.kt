package com.navree.client.runtime

import com.navree.client.protocol.Route
import java.net.URI
import java.net.URISyntaxException

/** Where an address leads. */
sealed interface Target {
    /** A path and query on the app's own origin. */
    data class Inside(val url: String) : Target

    /** Another site, or a scheme the system handles: it opens outside the app. */
    data class Outside(val url: String) : Target

    /** Nothing this client opens: a script, a file, a broken address. */
    data object Refused : Target
}

/**
 * The one origin the app talks to. Every address a tree or a web page hands
 * the client goes through [resolve]; only [Target.Inside] is ever fetched or
 * shown in the web view.
 */
class Origin private constructor(
    val scheme: String,
    val host: String,
    val port: Int,
) {
    /** `https://host`, with the port when it is not the scheme's own. */
    val base: String = buildString {
        append(scheme).append("://").append(host)
        if (port != defaultPort(scheme)) append(':').append(port)
    }

    /** The full address of a path on this origin. */
    fun absolute(url: String): String = base + (if (url.startsWith("/")) url else "/$url")

    /**
     * Reads `href` as found on the page at `from` (a path on this origin).
     * A fragment is dropped: a tree has no anchors.
     */
    fun resolve(href: String, from: String = "/"): Target {
        val text = href.trim()
        if (text.isEmpty() || text.startsWith("#")) return Target.Refused
        // A backslash is read as a slash by browsers; `/\host` would leave the origin.
        if (text.any { it == '\\' || it.isISOControl() }) return Target.Refused
        val scheme = SCHEME.find(text)?.groupValues?.get(1)?.lowercase()
        return when {
            text.startsWith("//") -> web("${this.scheme}:$text")
            scheme == null -> relative(text, from)
            scheme == "http" || scheme == "https" -> web(text)
            scheme in HANDED_TO_SYSTEM -> Target.Outside(text)
            else -> Target.Refused
        }
    }

    /** Whether a full address is on this origin. The web view asks before it loads anything. */
    fun owns(address: String): Boolean = resolve(address) is Target.Inside

    private fun relative(text: String, from: String): Target {
        if (text.startsWith("?")) {
            // The same page with another query. (The platform's resolver drops
            // the last path segment here, which the URL standard does not.)
            return relative(Routes.path(from) + text, "/")
        }
        val base = try {
            URI(this.base + (if (from.startsWith("/")) from else "/$from"))
        } catch (_: URISyntaxException) {
            URI("${this.base}/")
        }
        val resolved = try {
            base.resolve(URI(text)).normalize()
        } catch (_: URISyntaxException) {
            return Target.Refused
        } catch (_: IllegalArgumentException) {
            return Target.Refused
        }
        return inside(resolved)
    }

    private fun web(text: String): Target {
        val uri = try {
            URI(text).normalize()
        } catch (_: URISyntaxException) {
            return Target.Refused
        }
        val host = uri.host ?: return Target.Refused
        val scheme = uri.scheme?.lowercase() ?: return Target.Refused
        val port = if (uri.port == -1) defaultPort(scheme) else uri.port
        val same = scheme == this.scheme && host.equals(this.host, ignoreCase = true) && port == this.port
        return if (same && uri.rawUserInfo == null) inside(uri) else Target.Outside(uri.toString())
    }

    private fun inside(uri: URI): Target {
        val path = uri.rawPath?.takeIf { it.isNotEmpty() } ?: "/"
        // After normalising, a path that still climbs has left the root.
        if (path.startsWith("/..")) return Target.Refused
        val query = uri.rawQuery?.let { "?$it" } ?: ""
        return Target.Inside(path + query)
    }

    override fun equals(other: Any?): Boolean =
        other is Origin && other.scheme == scheme && other.host == host && other.port == port

    override fun hashCode(): Int = base.hashCode()

    override fun toString(): String = base

    companion object {
        private val SCHEME = Regex("^([a-zA-Z][a-zA-Z0-9+.-]*):")
        private val HANDED_TO_SYSTEM = setOf("mailto", "tel", "sms", "geo")
        private val LOOPBACK = setOf("localhost", "127.0.0.1", "10.0.2.2", "[::1]")

        private fun defaultPort(scheme: String): Int = if (scheme == "http") 80 else 443

        /**
         * Reads an origin. It must be `https`; plain `http` is taken only for
         * the machine itself (and the emulator's name for its host) and only
         * when `loopbackHttp` says so, which a release build never does.
         */
        fun parse(text: String, loopbackHttp: Boolean = false): Origin? {
            val uri = try {
                URI(text.trim())
            } catch (_: URISyntaxException) {
                return null
            }
            val scheme = uri.scheme?.lowercase() ?: return null
            val host = uri.host?.lowercase() ?: return null
            if (uri.rawUserInfo != null) return null
            val allowed = scheme == "https" || (scheme == "http" && loopbackHttp && host in LOOPBACK)
            if (!allowed) return null
            val port = if (uri.port == -1) defaultPort(scheme) else uri.port
            return Origin(scheme, host, port)
        }
    }
}

/** The shell's rules for how addresses are presented. */
object Routes {
    /** The first route whose pattern matches the path of `url`. */
    fun find(routes: List<Route>, url: String): Route? = routes.firstOrNull { matches(it.match, url) }

    /**
     * Whether `url`'s path fits `pattern`: `*` stands for one segment, a
     * trailing `**` for whatever is left, and anything else for itself.
     */
    fun matches(pattern: String, url: String): Boolean {
        val wanted = segments(pattern)
        val given = segments(path(url))
        for ((index, part) in wanted.withIndex()) {
            if (part == "**") return index == wanted.lastIndex
            val segment = given.getOrNull(index) ?: return false
            if (part != "*" && part != segment) return false
        }
        return wanted.size == given.size
    }

    /** The path of an address on the origin, without its query. */
    fun path(url: String): String = url.substringBefore('?').substringBefore('#').ifEmpty { "/" }

    private fun segments(path: String): List<String> = path.split('/').filter { it.isNotEmpty() }
}
