package com.navree.client.design

import com.navree.protocol.Navree
import kotlin.math.min

/**
 * A list whose rows are moved (a board's lane of cards, a checklist) is laid
 * lazily: only the rows near the window are drawn, whatever the list holds.
 * What that takes is decided here, from numbers the lazy list reports (where
 * each visible item stands and how tall it is), never from every row.
 */
object LaneRules {
    /** Where one visible row of a lane stands: its place in the lane, and its offset and height in the list's frame, in px. */
    data class Slot(val place: Int, val offset: Int, val size: Int)

    /**
     * The place a held row has reached. The row is `held` (its place in the
     * lane now) and its top is at `top`, `size` tall, in the list's frame; the
     * others are `slots`, and the row's own place stands at `heldTop` (null
     * where the lazy list does not report it). The row passes a neighbour when
     * the middle of the held row goes beyond the middle of that neighbour, so a
     * row that has just swapped does not swap back. The middle is kept inside
     * the window (`viewStart` to `viewEnd`), and the row passes a neighbour
     * only if the place it then stands in is wholly inside the window: a row
     * that stood outside the window would be one the lazy list does not lay,
     * and the finger would lose it. Rows that are not visible are not passed.
     */
    fun landing(held: Int, top: Float, size: Int, slots: List<Slot>, viewStart: Int, viewEnd: Int, heldTop: Int?): Int {
        if (size <= 0 || slots.isEmpty() || heldTop == null) return held
        val centre = (top + size / 2f).coerceIn(viewStart.toFloat(), viewEnd.toFloat())
        val ordered = slots.sortedBy { it.place }
        var place = held
        var passed = 0
        for (slot in ordered) {
            if (slot.place <= held) continue
            if (slot.place != place + 1) break
            if (slot.offset + slot.size / 2f >= centre) break
            if (heldTop + passed + slot.size + size > viewEnd) break
            passed += slot.size
            place = slot.place
        }
        if (place != held) return place
        for (slot in ordered.asReversed()) {
            if (slot.place >= held) continue
            if (slot.place != place - 1) break
            if (slot.offset + slot.size / 2f <= centre) break
            if (heldTop - passed - slot.size < viewStart) break
            passed += slot.size
            place = slot.place
        }
        return place
    }

    /**
     * How far the list scrolls by itself in a frame, in px (negative: towards
     * its start), while a row is held with its top at `top`, `size` tall: not at
     * all while the row is clear of both edges of the window, and faster the
     * further into an edge's `edge` px it is, up to `most`. It goes only the
     * way the finger last moved (`towards`): a row that was picked up beside an
     * edge does not scroll the list before the finger has gone that way, and a
     * finger that has not moved at all scrolls nothing.
     */
    fun scrollBy(top: Float, size: Int, viewStart: Int, viewEnd: Int, edge: Float, most: Float, towards: Float): Float {
        if (edge <= 0f || most <= 0f) return 0f
        val up = if (towards < 0f) ((viewStart + edge) - top) / edge else 0f
        val down = if (towards > 0f) ((top + size) - (viewEnd - edge)) / edge else 0f
        return when {
            up > 0f -> -most * min(1f, up)
            down > 0f -> most * min(1f, down)
            else -> 0f
        }
    }

    /** What the end of a lane does. */
    enum class End {
        /** The lane holds all its rows. */
        Nothing,

        /** The lane has more rows: drawing its end asks for them (the core asks once). */
        Asks,

        /** The last ask failed: the end says so, with a way to try again. */
        Retry,
    }

    /** What the end of a list does: it asks for the next rows when the list says there are more (`hasMore`) and names itself. */
    fun end(hasMore: Boolean, listId: String?, failed: Boolean): End = when {
        !hasMore || listId == null -> End.Nothing
        failed -> End.Retry
        else -> End.Asks
    }

    /** The key of a row of a lane in the lazy list: the same row has the same key wherever it stands, and no row of another list has it. */
    fun key(listId: String?, rowId: String): String = "move:${listId.orEmpty()}:$rowId"

    /** The id a key made by [key] names for this list, or null when it is another item's. */
    fun idOf(listId: String?, key: Any?): String? {
        val prefix = "move:${listId.orEmpty()}:"
        return (key as? String)?.takeIf { it.startsWith(prefix) }?.substring(prefix.length)
    }

    /**
     * The ids of the rows of a list that are moved, in its order. A row that
     * cannot be moved (no id, a list that takes no moves) and a row whose id an
     * earlier row has is not among them.
     */
    fun movable(list: Navree.List): List<String> {
        val seen = HashSet<String>()
        val out = ArrayList<String>()
        for (child in list.children) {
            if (child !is Navree.Row || !ReorderRules.draggable(list, child)) continue
            val id = child.id ?: continue
            if (seen.add(id)) out += id
        }
        return out
    }

    /** The ids in `order` with the one at `from` moved to `to`. */
    fun moved(order: List<String>, from: Int, to: Int): List<String> {
        if (from !in order.indices || to !in order.indices || from == to) return order
        val out = order.toMutableList()
        out.add(to, out.removeAt(from))
        return out
    }

    /** The id of the row `held` stands before in `order`: null when it is the last. Null as well when it is not in `order`. */
    fun before(order: List<String>, held: String): String? {
        val at = order.indexOf(held)
        return if (at < 0) null else order.getOrNull(at + 1)
    }

    /**
     * `items` with the ones that are rows of the lane (`idOf` names them) put in
     * the order `order` names, each in a place a row of the lane stood in: the
     * items between them keep theirs. `order` that does not name exactly the
     * rows there are is not used.
     */
    fun <T> arranged(items: List<T>, idOf: (T) -> String?, order: List<String>?): List<T> {
        if (order == null) return items
        val slots = ArrayList<Int>()
        val byId = HashMap<String, T>()
        items.forEachIndexed { at, item ->
            val id = idOf(item)
            if (id != null) {
                slots += at
                byId[id] = item
            }
        }
        if (order.size != slots.size || byId.size != slots.size || !order.all { it in byId }) return items
        val out = items.toMutableList()
        slots.forEachIndexed { k, slot -> out[slot] = byId.getValue(order[k]) }
        return out
    }

    /**
     * Whether a grid whose parts are lanes shows them one at a time (a window
     * of one column): the lane in hand then joins the screen's own list, and a
     * row of lanes (each its own list as tall as the window) is for a wide one.
     * The room is worked out as the grid's own drawing works it out.
     */
    fun oneAtATime(grid: Navree.Grid, parts: List<Navree.Node>, paneDp: Int, marginDp: Int, fontScale: Float): Boolean {
        if (!ListRules.areLanes(parts)) return false
        val room = ContentRules.contentDp(paneDp.toFloat(), ContentRules.TABLE_MOST_DP, marginDp)
        val columns = ListRules.gridColumns(
            room.toInt(),
            ListRules.gridLeast(grid.min, parts),
            ContentRules.gap(grid.gap),
            grid.cols,
            fontScale,
            parts.size,
            controls = parts.any { it is Navree.Field },
        )
        return ListRules.oneLaneAtATime(parts, columns)
    }
}
