package com.navree.client.design

import com.navree.protocol.Navree
import kotlin.math.roundToInt

/**
 * Rows the person puts in another order, or moves to another list, as
 * Material's lists do it: a drag handle at the row's end, the row lifted and
 * following the finger, and the same moves offered as actions (move up, move
 * down, move to a list) for TalkBack and for anyone who does not drag.
 */
object ReorderRules {
    /** A row can be moved when its list takes moves and both have ids. */
    fun draggable(list: Navree.List, row: Navree.Row): Boolean = list.reorder != null && list.id != null && row.id != null

    /** How many rows a drag of `offsetPx` crossed, for rows `rowPx` tall: half a row counts as one. */
    fun steps(offsetPx: Float, rowPx: Int): Int = if (rowPx <= 0) 0 else (offsetPx / rowPx).roundToInt()

    /**
     * Where the row at `from` lands when moved `steps` places in a list whose
     * rows have these ids: the id of the row it then stands before, or null at
     * the end. Null as a whole when it does not move.
     */
    fun landing(ids: List<String>, from: Int, steps: Int): Landing? {
        if (from !in ids.indices) return null
        val to = (from + steps).coerceIn(0, ids.lastIndex)
        if (to == from) return null
        val rest = ids.filterIndexed { index, _ -> index != from }
        return Landing(to, rest.getOrNull(to))
    }

    /** Where a moved row lands: its new place, and the row it stands before (null: at the end). */
    data class Landing(val index: Int, val before: String?)

    /**
     * Whether a row may be dropped into `to`: its own list always; another
     * only when both lists are of the same group and `to` is not full.
     */
    fun accepts(from: Navree.List, to: Navree.List): Boolean {
        if (from === to || (from.id != null && from.id == to.id)) return true
        val group = from.reorder?.group ?: return false
        val other = to.reorder ?: return false
        if (other.group != group || to.id == null) return false
        val limit = other.limit ?: return true
        return to.children.count { it is Navree.Row } < limit
    }

    /**
     * The other lists a row of `from` may be moved to, each with the name the
     * page gives it (the title of the section or pane around it, else its id),
     * in the order they stand.
     */
    fun targets(body: Navree.Node?, from: Navree.List): List<Pair<Navree.List, String>> {
        if (body == null) return emptyList()
        val out = ArrayList<Pair<Navree.List, String>>()
        fun visit(node: Navree.Node, title: String?) {
            val named = when (node) {
                is Navree.Section -> node.title ?: title
                is Navree.Pane -> node.title ?: title
                else -> title
            }
            if (node is Navree.List && node !== from && node.id != from.id && node.reorder != null && accepts(from, node)) {
                out += node to (named ?: node.id ?: "")
            }
            for (child in node.childNodes()) visit(child, named)
        }
        visit(body, null)
        return out
    }
}
