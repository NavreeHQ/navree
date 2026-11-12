package com.navree.client.protocol

/**
 * One change the server makes to the screen in front. The list is the one the
 * browser runtime applies, with nodes where the web has markup. A client
 * applies ops in order; one whose target is not on the screen is skipped.
 */
@kotlinx.serialization.Serializable(with = OpSerializer::class)
sealed interface Op {
    /** Puts `node` in place of the node `target`, keeping what the user typed and what is open. */
    data class Morph(val target: String, val node: Node) : Op

    /** Puts `node` in place of the node `target`, keeping nothing. */
    data class Replace(val target: String, val node: Node) : Op

    /** Adds `nodes` after the last child of `target`. For a list, `more` is the address of the page after. */
    data class Append(val target: String, val nodes: List<Node>, val more: String? = null) : Op

    /** Adds `nodes` before the first child of `target`. */
    data class Prepend(val target: String, val nodes: List<Node>) : Op

    /** Removes the node `target`. */
    data class Remove(val target: String) : Op

    /** Shows field errors in a form, or clears them when `fields` is empty. */
    data class Errors(val form: String, val fields: Map<String, List<String>>) : Op

    /** Shows a short message that leaves by itself. */
    data class Toast(
        val tone: Tone,
        val text: String,
        val title: String? = null,
        val id: String? = null,
        val action: ToastAction? = null,
    ) : Op

    /** Opens the sheet, dialog, popover or disclosure `target`. */
    data class Open(val target: String) : Op

    /** Closes the sheet, dialog, popover or disclosure `target`. */
    data class Close(val target: String) : Op

    /** Moves focus to `target`. */
    data class Focus(val target: String) : Op

    /** Changes the address of the screen in front without loading anything. */
    data class Url(val href: String, val replace: Boolean) : Op

    /** Goes to `href` on this origin. */
    data class Navigate(val href: String) : Op

    /** Leaves for `href` on another site. */
    data class NavigateExternal(val href: String) : Op

    /** Loads the screen in front again. */
    data object Reload : Op

    /** Fetches the region `target` again. */
    data class Invalidate(val target: String) : Op

    /** An op from a newer protocol. It is skipped. */
    data class Unknown(val name: String) : Op
}

/** The one button a toast may have, such as Undo. */
data class ToastAction(
    val label: String,
    val href: String,
    val post: Boolean,
)
