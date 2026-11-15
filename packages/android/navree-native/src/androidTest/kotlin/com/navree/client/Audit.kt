package com.navree.client

import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsNode
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull

/**
 * What a screen reader is told by the screen in front, in the order it reads
 * it, and the controls whose name or size is a fault: written beside each
 * picture as `<picture>.sem.txt`.
 */
object Audit {
    /** The smallest side a control a finger can hit has, in dp (Android's own accessibility guidance). */
    const val TARGET_DP = 48

    /** A control that is a fault where it stands, with why it stays: none today. A line is a regular expression over the fault's text. */
    val allowed: List<Pair<Regex, String>> = emptyList()

    class Fault(message: String) : AssertionError(message)

    /** The faults of a tree: a control a finger can hit that has no name, or that is wholly in view and smaller than 48 dp. */
    fun faults(roots: List<SemanticsNode>): List<String> {
        val faults = ArrayList<String>()
        roots.forEach { walk(it, 0, ArrayList(), faults) }
        return faults
    }

    /** Fails when the picture's tree has a fault that is not listed in [allowed] with its reason. */
    fun require(picture: String, faults: List<String>) {
        val unlisted = faults.filter { fault -> allowed.none { (pattern, _) -> pattern.containsMatchIn(fault) } }
        if (unlisted.isNotEmpty()) throw Fault("$picture: ${unlisted.size} control(s) a screen reader or a finger cannot use well: ${unlisted.take(6)}")
    }

    /** The merged tree as a screen reader has it: one line a stop, indented by depth, then the faults. */
    fun report(roots: List<SemanticsNode>): String {
        val lines = ArrayList<String>()
        val faults = ArrayList<String>()
        // A menu, a question or a picker is a window of its own, and is a root beside the screen's.
        roots.forEachIndexed { index, root ->
            if (index > 0) lines += "# Window ${index + 1}"
            walk(root, 0, lines, faults)
        }
        return buildString {
            appendLine("# What is read, in order")
            lines.forEach(::appendLine)
            appendLine()
            appendLine("# Faults: ${faults.size}")
            faults.forEach(::appendLine)
        }
    }

    private fun name(node: SemanticsNode): String {
        val config = node.config
        val description = config.getOrNull(SemanticsProperties.ContentDescription)?.joinToString(", ").orEmpty()
        val text = config.getOrNull(SemanticsProperties.Text)?.joinToString(", ") { it.text }.orEmpty()
        val edited = config.getOrNull(SemanticsProperties.EditableText)?.text.orEmpty()
        return listOf(description, text, edited).filter { it.isNotEmpty() }.distinct().joinToString(" | ")
    }

    private fun walk(node: SemanticsNode, depth: Int, lines: MutableList<String>, faults: MutableList<String>) {
        val config = node.config
        val density = node.layoutInfo.density.density
        val widthDp = node.size.width / density
        val heightDp = node.size.height / density
        // What a finger can hit: the node's own size widened to the minimum touch size by the component, where it is.
        val touch = node.touchBoundsInRoot
        val touchWidthDp = touch.width / density
        val touchHeightDp = touch.height / density
        // A control cut by the edge of what shows (a lane scrolled half out of view, a card at the bottom of the page) has only its visible part to hit: it is judged where it is whole.
        val whole = touch.width > 0f && touch.height > 0f && touch.width + 1f >= node.size.width && touch.height + 1f >= node.size.height
        val named = name(node)
        val clickable = config.contains(SemanticsActions.OnClick) || config.contains(SemanticsActions.OnLongClick)
        val role = config.getOrNull(SemanticsProperties.Role)?.toString()
        val flags = buildList {
            if (config.contains(SemanticsProperties.Heading)) add("heading")
            if (clickable) add("click")
            if (config.contains(SemanticsProperties.Disabled)) add("disabled")
            config.getOrNull(SemanticsProperties.Selected)?.let { add(if (it) "selected" else "not-selected") }
            config.getOrNull(SemanticsProperties.ToggleableState)?.let { add("toggle=$it") }
            config.getOrNull(SemanticsProperties.StateDescription)?.let { add("state=$it") }
            config.getOrNull(SemanticsProperties.LiveRegion)?.let { add("live=$it") }
        }
        if (named.isNotEmpty() || clickable || role != null || flags.isNotEmpty()) {
            lines += "${"  ".repeat(depth)}${role ?: "-"} \"$named\" ${flags.joinToString(" ")} ${widthDp.toInt()}x${heightDp.toInt()}dp at ${(node.boundsInRoot.left / density).toInt()},${(node.boundsInRoot.top / density).toInt()}"
        }
        if (clickable && !config.contains(SemanticsProperties.Disabled)) {
            if (named.isEmpty()) faults += "UNNAMED control ${widthDp.toInt()}x${heightDp.toInt()}dp at ${node.boundsInRoot}"
            if (whole && minOf(touchWidthDp, touchHeightDp) < TARGET_DP - 0.5f && widthDp > 0f) {
                faults += "SMALL ${touchWidthDp.toInt()}x${touchHeightDp.toInt()}dp \"$named\""
            }
        }
        node.children.forEach { walk(it, depth + 1, lines, faults) }
    }
}
