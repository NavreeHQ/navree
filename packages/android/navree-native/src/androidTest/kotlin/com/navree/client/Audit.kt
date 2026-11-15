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

    /** The merged tree as a screen reader has it: one line a stop, indented by depth, then the faults. */
    fun report(root: SemanticsNode): String {
        val lines = ArrayList<String>()
        val faults = ArrayList<String>()
        walk(root, 0, lines, faults)
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
            if (minOf(widthDp, heightDp) < TARGET_DP - 0.5f && widthDp > 0f) {
                faults += "SMALL ${widthDp.toInt()}x${heightDp.toInt()}dp \"$named\""
            }
        }
        node.children.forEach { walk(it, depth + 1, lines, faults) }
    }
}
