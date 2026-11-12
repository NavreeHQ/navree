package com.navree.client.runtime

import com.navree.client.protocol.ButtonNode
import com.navree.client.protocol.DialogNode
import com.navree.client.protocol.DisclosureNode
import com.navree.client.protocol.NavItem
import com.navree.client.protocol.Node
import com.navree.client.protocol.PopoverNode
import com.navree.client.protocol.Present
import com.navree.client.protocol.Screen
import com.navree.client.protocol.SheetNode
import com.navree.client.protocol.Shell
import com.navree.client.protocol.Tone
import com.navree.client.protocol.ToastAction
import com.navree.client.protocol.Upgrade

/** Why a load failed. The words shown for each belong to the client, in the user's language. */
enum class Failure {
    /** The server could not be reached. */
    Offline,

    /** The server answered with an error. */
    Server,

    /** The answer was not something this client understands. */
    Protocol,
}

/** How far a document that is fetched once has got. */
sealed interface Load {
    data object Idle : Load
    data object Loading : Load
    data object Ready : Load
    data class Failed(val failure: Failure) : Load
}

/** What an entry shows. */
sealed interface Content {
    /** Nothing yet. */
    data object Blank : Content

    /** A native tree. */
    data class Native(val screen: Screen) : Content

    /** The page in the web view, with what could not be drawn natively. */
    data class Web(val url: String, val why: List<String> = emptyList()) : Content
}

/** A request to move focus. The number changes with every request, so the same target can be asked for twice. */
data class FocusRequest(val target: String, val serial: Long)

/** A destructive action that waits for the user's yes. */
data class PendingConfirm(val button: ButtonNode)

/**
 * What a screen holds that the server did not send: what was typed, which
 * overlays are open, what commands changed. A fresh tree for the same screen
 * keeps the typed values; a `replace` of a node drops what was under it.
 */
data class Local(
    /** Typed values, by form id and then field name. A field outside a form is under the empty id. */
    val values: Map<String, Map<String, FieldValue>> = emptyMap(),
    /** Field errors the server sent, by form id and then field name. */
    val errors: Map<String, Map<String, List<String>>> = emptyMap(),
    /** Forms and buttons with a request on its way, by id. */
    val busy: Set<String> = emptySet(),
    /** Overlays and disclosures opened here, oldest first. */
    val opened: List<String> = emptyList(),
    /** Overlays and disclosures that arrived open and were closed here. */
    val closed: Set<String> = emptySet(),
    /** Nodes a command hid (true) or showed (false). */
    val hidden: Map<String, Boolean> = emptyMap(),
    /** Nodes a command disabled (true) or enabled (false). */
    val disabled: Map<String, Boolean> = emptyMap(),
    /** Text a command set on a node. */
    val texts: Map<String, String> = emptyMap(),
    /** Values a command set on hidden fields, by the field's id or name. */
    val hiddenValues: Map<String, String> = emptyMap(),
    /** The other named states a command switched on, by node id. */
    val states: Map<String, Set<String>> = emptyMap(),
    /** Regions that have been asked for since the tree arrived, by id. */
    val regionsAsked: Set<String> = emptySet(),
    /** What the search field holds, when the user has typed in it. */
    val search: String? = null,
    val focus: FocusRequest? = null,
    val confirm: PendingConfirm? = null,
    /** Lists whose next page is on its way, by id. */
    val loadingMore: Set<String> = emptySet(),
) {
    /** Whether the overlay or disclosure is open now. */
    fun isOpen(id: String?, arrivedOpen: Boolean): Boolean =
        id != null && (id in opened || (arrivedOpen && id !in closed))

    fun isOpen(node: Node): Boolean = when (node) {
        is SheetNode -> isOpen(node.id, node.open)
        is DialogNode -> isOpen(node.id, node.open)
        is PopoverNode -> isOpen(node.id, false)
        is DisclosureNode -> isOpen(node.id, node.open)
        else -> false
    }

    /** Whether a command hid the node, or, with no word from a command, the tree did. */
    fun isHidden(id: String?, inTree: Boolean = false): Boolean = if (id == null) inTree else hidden[id] ?: inTree

    fun value(form: String?, name: String): FieldValue? = values[form ?: ""]?.get(name)

    fun errors(form: String?, name: String): List<String> = errors[form ?: ""]?.get(name).orEmpty()
}

/** One screen in a stack. */
data class Entry(
    val key: Long,
    /** Its address on the origin: path and query. */
    val url: String,
    val content: Content = Content.Blank,
    /** A fetch of this page is on its way. */
    val loading: Boolean = false,
    /** The user asked for it again and is watching the indicator. */
    val refreshing: Boolean = false,
    /** The last fetch failed. With content on screen this is a passing message; without, it is the screen. */
    val failure: Failure? = null,
    /** What it shows may be out of date; it is fetched again when it comes to the front. */
    val stale: Boolean = false,
    /** Whether the app's navigation and the app bar are drawn around it. */
    val chrome: Boolean = true,
    val local: Local = Local(),
) {
    val screen: Screen? get() = (content as? Content.Native)?.screen
}

/** A top-level destination and the screens stacked in it. The stack is empty until it is first shown. */
data class Tab(
    val item: NavItem,
    val stack: List<Entry> = emptyList(),
)

/** Screens presented over everything else: a sheet or a full-screen cover, with its own stack. */
data class Modal(
    val present: Present,
    val stack: List<Entry>,
)

/** A short message waiting to be shown. */
data class Message(
    val id: Long,
    val tone: Tone = Tone.Neutral,
    /** The server's words, or none when `notice` says what happened. */
    val text: String? = null,
    val title: String? = null,
    val action: ToastAction? = null,
    /** A message of the client's own, such as a failed refresh. */
    val notice: Failure? = null,
    /** The entry whose address relative links in the action are read against. */
    val entry: Long? = null,
)

/** What a request that is on its way was for. */
sealed interface Pending {
    data object ShellLoad : Pending
    data class Page(val entry: Long) : Pending
    /**
     * A form or a button's own request. `write` says it may have changed
     * something; `inPlace` that a screen in the answer takes this screen's place.
     */
    data class Send(val entry: Long, val busy: String?, val write: Boolean, val inPlace: Boolean = false) : Pending
    data class More(val entry: Long, val list: String, val was: String) : Pending
    data class Region(val entry: Long, val region: String) : Pending
}

/** The last tree per address, newest last, bounded. */
data class TreeCache(
    val trees: Map<String, Screen> = emptyMap(),
    val limit: Int = 48,
) {
    operator fun get(url: String): Screen? = trees[url]

    fun put(url: String, screen: Screen): TreeCache {
        val next = LinkedHashMap(trees)
        next.remove(url)
        next[url] = screen
        while (next.size > limit) next.remove(next.keys.first())
        return copy(trees = next)
    }

    fun clear(): TreeCache = copy(trees = emptyMap())
}

/** Everything the client knows. The reducer is the only thing that makes a new one. */
data class ClientState(
    val origin: Origin,
    val shell: Shell? = null,
    val shellLoad: Load = Load.Idle,
    /** The build the cached trees belong to. */
    val build: String? = null,
    val tabs: List<Tab> = emptyList(),
    val selected: Int = 0,
    val modals: List<Modal> = emptyList(),
    val cache: TreeCache = TreeCache(),
    val messages: List<Message> = emptyList(),
    val pending: Map<Long, Pending> = emptyMap(),
    val upgrade: Upgrade? = null,
    /** An address the app was opened with before the shell had arrived. */
    val launch: String? = null,
    /** The last number handed out: entry keys, request numbers and toast ids all come from it. */
    val serial: Long = 0,
) {
    /** The stack in front: the top modal's, or the selected destination's. */
    val frontStack: List<Entry>
        get() = modals.lastOrNull()?.stack ?: tabs.getOrNull(selected)?.stack.orEmpty()

    /** The entry the user is looking at. */
    val front: Entry? get() = frontStack.lastOrNull()

    /** Every entry, wherever it is. */
    fun entries(): Sequence<Entry> = sequence {
        for (tab in tabs) yieldAll(tab.stack)
        for (modal in modals) yieldAll(modal.stack)
    }

    fun entry(key: Long): Entry? = entries().firstOrNull { it.key == key }

    /** Whether the entry sits in a modal. */
    fun isModal(key: Long): Boolean = modals.any { modal -> modal.stack.any { it.key == key } }

    /**
     * Whether a back press is the app's to handle. When it is not, the system
     * takes it, and its own back-to-home animation plays.
     */
    val canGoBack: Boolean
        get() = modals.isNotEmpty() || (tabs.getOrNull(selected)?.stack?.size ?: 0) > 1 || selected != 0
}
