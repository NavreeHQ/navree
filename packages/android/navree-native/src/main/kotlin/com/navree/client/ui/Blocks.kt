package com.navree.client.ui

import com.navree.client.core.ScreenView
import com.navree.client.design.CompositionRules
import com.navree.client.design.ContentRules
import com.navree.client.design.ListRules
import com.navree.client.design.PagerRules
import com.navree.client.design.SelectionRules
import com.navree.client.design.childNodes
import com.navree.client.design.nodeId
import com.navree.client.design.walk
import com.navree.protocol.Navree

/** Where an item sits in a group that shares one container. */
internal enum class Group { None, Only, First, Middle, Last }

/**
 * One item of a screen's scrolling column. A screen's body is flattened into
 * these so a long list is drawn row by row as it scrolls, never all at once.
 */
internal sealed interface Block {
    val key: String

    /** Any node, drawn whole, with the screen's side margins. */
    data class Single(override val key: String, val node: Navree.Node, val form: String?) : Block

    /** A row, a switch or a feed row, edge to edge, or inside a group's container. */
    data class Item(override val key: String, val node: Navree.Node, val group: Group, val form: String?) : Block

    /** A table's row drawn as a list row. */
    data class TableItem(override val key: String, val table: Navree.Table, val row: Navree.TableRow, val form: String?) : Block

    /** A table's filters, as a row of chips. `search` says its search box is drawn here too, not in the app bar. */
    data class Filters(override val key: String, val form: Navree.Form, val search: Boolean) : Block

    data class Header(override val key: String, val text: String, val subtitle: String? = null) : Block

    data class Space(override val key: String, val dp: Int) : Block

    /** The end of a list that has more: drawing it asks for the next page. `id` is the list's, or the table's region's. */
    data class More(override val key: String, val id: String) : Block

    /** The start of a list that has rows before its first: drawing it asks for them. */
    data class Before(override val key: String, val id: String) : Block

    /** A region that is still on its way. */
    data class Waiting(override val key: String, val region: String) : Block

    /** A node that fills the window: an empty state that is the whole screen. */
    data class Fill(override val key: String, val node: Navree.Node) : Block

    /**
     * Cards side by side on a wide window, a line of the dashboard's grid. A
     * line with fewer cards than the window has columns stretches them across
     * the line (`columns` is how many it has), so a card alone spans it.
     */
    data class Cards(override val key: String, val nodes: List<Navree.Node>, val columns: Int, val form: String?) : Block
}

/**
 * Whether a part can say how tall it would be before it is laid out, so the
 * parts of a line can be made as tall as the tallest. A grid, a meter and a
 * stack that turns know the width they have from the room the line gives them
 * and measure nothing, so they can. A part that measures what it is given
 * while it lays itself out (a carousel, an adaptive part, the heat maps, tabs,
 * a table, a list, two panes) cannot: its line keeps each part's own height.
 */
internal fun measuresAhead(node: Navree.Node): Boolean = node.walk().none {
    it is Navree.Carousel || it is Navree.Adaptive || it is Navree.CalendarHeatmap || it is Navree.MatrixHeatmap ||
        it is Navree.Tabs || it is Navree.Table || it is Navree.List || it is Navree.Split
}

/**
 * Whether the screen asks for parts side by side: some cards stand in a line,
 * or a stack turns along the line in a wide room. Its one column is then as
 * wide as they need.
 */
internal fun cardsSideBySide(blocks: List<Block>): Boolean = blocks.any { block ->
    (block is Block.Cards && block.nodes.size > 1) ||
        (block is Block.Single && (block.node as? Navree.Stack)?.let { it.wide != null && it.wide != it.axis } == true)
}

/** Whether the node draws anything where it stands. Overlays are drawn over the screen, not in it. */
internal fun inFlow(node: Navree.Node, screen: ScreenView): Boolean = when {
    node is Navree.Stack && node.hidden -> false
    screen.hidden(node.nodeId) -> false
    node is Navree.Sheet || node is Navree.Dialog || node is Navree.Popover -> false
    node is Navree.Toolbar -> false
    // A day pager that leads the screen stands in the app bar.
    node is Navree.DayPager && screen.screen?.let { PagerRules.inBar(it) } === node -> false
    // The buttons for chosen rows are the contextual app bar's.
    node is Navree.Form && node.id != null && node.id == bulkFormId(screen) -> false
    else -> true
}

private val bulkForms = java.util.WeakHashMap<Navree.Screen, String>()

/** The id of the form whose buttons the contextual app bar holds while rows are chosen, or null. */
internal fun bulkFormId(screen: ScreenView): String? {
    val tree = screen.screen ?: return null
    val known = bulkForms.getOrPut(tree) {
        SelectionRules.selectable(tree.body)?.let { SelectionRules.bulkForm(tree.body, it)?.id }.orEmpty()
    }
    return known.ifEmpty { null }
}

/**
 * Flattens a body into blocks. Vertical stacks, lists, regions, plain and
 * inset sections, forms, panes and (on a narrow width) tables are taken
 * apart; everything else is one block.
 *
 * `omit` is a node drawn elsewhere (a form's button that the app bar holds).
 * `hoisted` is the query whose search box the app bar holds.
 */
internal fun blocks(
    body: Navree.Node?,
    screen: ScreenView,
    widthDp: Int,
    omit: Navree.Node? = null,
    hoisted: Navree.Form? = null,
    fontScale: Float = 1f,
): List<Block> {
    val out = ArrayList<Block>()
    val cardColumns = ContentRules.cardColumns(widthDp, fontScale)
    fun card(node: Navree.Node) = (node is Navree.Section && node.style == Navree.SectionStyle.CARD) || node is Navree.Card
    val used = HashSet<String>()

    fun key(path: String, id: String?): String {
        val wanted = id?.let { "id:$it" } ?: path
        return if (used.add(wanted)) wanted else path.also { used.add(it) }
    }

    fun groupable(node: Navree.Node): Boolean = node is Navree.Row || (node is Navree.Field && node.input is Navree.ToggleInput)

    fun add(node: Navree.Node, path: String, form: String?, region: String?, top: Boolean) {
        if (node === omit || !inFlow(node, screen)) return
        when (node) {
            is Navree.Stack -> if (CompositionRules.splitsIntoBlocks(node, widthDp.toFloat())) {
                val gap = ContentRules.gap(node.gap)
                var any = false
                val shown = node.children.count { it !== omit && inFlow(it, screen) }
                var run = ArrayList<Pair<Int, Navree.Node>>()
                fun flush() {
                    for (line in run.chunked(cardColumns)) {
                        if (any && gap > 0) out += Block.Space(key("$path/${line.first().first}/gap", null), gap)
                        out += Block.Cards(key("$path/${line.first().first}/cards", null), line.map { it.second }, ContentRules.lineColumns(line.size, cardColumns), form)
                        any = true
                    }
                    run = ArrayList()
                }
                node.children.forEachIndexed { index, child ->
                    // On a wide window, cards that follow each other stand side by side.
                    if (cardColumns > 1 && card(child) && child !== omit && inFlow(child, screen)) {
                        run.add(index to child)
                        return@forEachIndexed
                    }
                    if (run.isNotEmpty()) flush()
                    if (child !== omit && inFlow(child, screen)) {
                        val before = out.size
                        if (any && gap > 0) out += Block.Space(key("$path/$index/gap", null), gap)
                        val spaced = out.size
                        add(child, "$path/$index", form, region, top = top && shown == 1)
                        // A part that drew nothing takes no room either.
                        if (out.size == spaced) {
                            if (spaced > before) out.removeAt(out.lastIndex)
                        } else {
                            any = true
                        }
                    }
                }
                if (run.isNotEmpty()) flush()
            } else {
                // A row of parts whose only part is drawn elsewhere draws nothing.
                if (node.children.any { it !== omit && inFlow(it, screen) }) out += Block.Single(key(path, node.id), node, form)
            }
            is Navree.Region -> {
                val shown = node.children.filter { inFlow(it, screen) }
                if (shown.isEmpty() && (node.lazy || screen.regions[node.id]?.loading == true)) {
                    out += Block.Waiting(key("$path/waiting", null), node.id)
                } else {
                    node.children.forEachIndexed { index, child -> add(child, "$path/$index", form, node.id, top = top && shown.size == 1) }
                }
            }
            is Navree.Pane -> {
                if (node.title != null) out += Block.Header(key("$path/header", null), node.title!!)
                node.children.forEachIndexed { index, child -> add(child, "$path/$index", form, region, top = top) }
            }
            // A list whose rows are moved is drawn whole, so a row can be dragged past the others.
            is Navree.List -> if (node.reorder != null) {
                out += Block.Single(key(path, node.id), node, form)
            } else {
                node.id?.let { id -> if (node.before != null) out += Block.Before(key("$path/before", null), id) }
                node.children.forEachIndexed { index, child ->
                    if (inFlow(child, screen)) {
                        if (child is Navree.Row || child is Navree.FeedRow) {
                            out += Block.Item(key("$path/$index", child.nodeId), child, Group.None, form)
                        } else {
                            add(child, "$path/$index", form, region, top = false)
                        }
                    }
                }
                val id = node.id
                if (node.more != null && id != null) out += Block.More(key("$path/more", null), id)
            }
            is Navree.Section -> if (node.style == Navree.SectionStyle.CARD) {
                out += Block.Single(key(path, node.id), node, form)
            } else {
                val title = node.title
                if (title != null) out += Block.Header(key("$path/header", null), title, node.subtitle)
                if (node.actions.isNotEmpty()) {
                    out += Block.Single(key("$path/actions", null), Navree.Stack(axis = Navree.Axis.HORIZONTAL, gap = 2, wrap = true, children = node.actions), form)
                }
                // A list inside a section gives the section its rows.
                val items = node.children.filter { inFlow(it, screen) }
                    .flatMap { child -> if (child is Navree.List && child.more == null && child.reorder == null) child.children.filter { inFlow(it, screen) } else listOf(child) }
                if (node.style == Navree.SectionStyle.INSET && items.isNotEmpty() && items.all(::groupable)) {
                    items.forEachIndexed { index, item ->
                        val group = when {
                            items.size == 1 -> Group.Only
                            index == 0 -> Group.First
                            index == items.lastIndex -> Group.Last
                            else -> Group.Middle
                        }
                        out += Block.Item(key("$path/$index", item.nodeId), item, group, form)
                    }
                } else {
                    var any = title != null
                    node.children.forEachIndexed { index, child ->
                        if (inFlow(child, screen)) {
                            if (any && child !is Navree.Row && child !is Navree.List) out += Block.Space(key("$path/$index/gap", null), 8)
                            add(child, "$path/$index", form, region, top = false)
                            any = true
                        }
                    }
                }
                node.footer.forEachIndexed { index, child -> add(child, "$path/footer/$index", form, region, top = false) }
            }
            is Navree.Form -> node.children.forEachIndexed { index, child -> add(child, "$path/$index", node.id, region, top = false) }
            is Navree.Table -> {
                val query = node.query
                if (query != null) out += Block.Filters(key("$path/filters", null), query, search = query !== hoisted)
                val empty = node.empty
                if (node.rows.isEmpty() && empty != null) {
                    out += Block.Single(key("$path/empty", empty.nodeId), empty, form)
                } else if (ListRules.tableAsList(widthDp, node.columns.size)) {
                    node.rows.forEachIndexed { index, row -> out += Block.TableItem(key("$path/$index", row.id), node, row, form) }
                } else {
                    out += Block.Single(key(path, node.id), node, form)
                }
                // A table's next rows are asked for by the region it stands in.
                val id = region ?: node.id
                if (node.more != null && id != null) out += Block.More(key("$path/more", null), id)
            }
            is Navree.Empty -> out += if (top) Block.Fill(key(path, node.id), node) else Block.Single(key(path, node.id), node, form)
            // A row, a feed row and a switch are list items: they keep their own insets and run edge to edge.
            is Navree.Row, is Navree.FeedRow -> out += Block.Item(key(path, node.nodeId), node, Group.None, form)
            is Navree.Field -> out += if (node.input is Navree.ToggleInput) {
                Block.Item(key(path, node.id), node, Group.None, form)
            } else {
                Block.Single(key(path, node.id), node, form)
            }
            else -> out += Block.Single(key(path, node.nodeId), node, form)
        }
    }

    if (body != null) add(body, "b", null, null, top = true)
    return out
}

/** The index of the block that holds the node with this id. */
internal fun blockOf(blocks: List<Block>, target: String): Int = blocks.indexOfFirst { block ->
    val node = when (block) {
        is Block.Single -> block.node
        is Block.Item -> block.node
        is Block.Fill -> block.node
        else -> null
    }
    node != null && holds(node, target)
}

private fun holds(node: Navree.Node, target: String): Boolean =
    node.nodeId == target || node.childNodes().any { holds(it, target) }

/** The toolbars of a body, drawn floating over the content rather than in it. */
internal fun toolbars(body: Navree.Node?, screen: ScreenView): List<Navree.Toolbar> {
    if (body == null) return emptyList()
    val out = ArrayList<Navree.Toolbar>()
    fun visit(node: Navree.Node) {
        if (node is Navree.Stack && node.hidden || screen.hidden(node.nodeId)) return
        when (node) {
            is Navree.Toolbar -> out += node
            is Navree.Sheet, is Navree.Dialog, is Navree.Popover -> Unit
            else -> node.childNodes().forEach(::visit)
        }
    }
    visit(body)
    return out
}

/** A screen's toolbars: those among its actions and those in its body. */
internal fun screenToolbars(screen: Navree.Screen, view: ScreenView): List<Navree.Toolbar> =
    screen.actions.filterIsInstance<Navree.Toolbar>().filter { !view.hidden(it.id) } + toolbars(screen.body, view)
