package com.navree.client.ui

import com.navree.client.design.ContentRules
import com.navree.client.design.ListRules
import com.navree.client.protocol.Axis
import com.navree.client.protocol.DialogNode
import com.navree.client.protocol.EmptyNode
import com.navree.client.protocol.FieldNode
import com.navree.client.protocol.FormNode
import com.navree.client.protocol.ListNode
import com.navree.client.protocol.Node
import com.navree.client.protocol.PopoverNode
import com.navree.client.protocol.RegionNode
import com.navree.client.protocol.RowNode
import com.navree.client.protocol.SectionNode
import com.navree.client.protocol.SectionStyle
import com.navree.client.protocol.SheetNode
import com.navree.client.protocol.StackNode
import com.navree.client.protocol.TableNode
import com.navree.client.protocol.TableRow
import com.navree.client.protocol.ToggleInput
import com.navree.client.runtime.Local

/** Where an item sits in a group that shares one container. */
internal enum class Group { None, Only, First, Middle, Last }

/**
 * One item of a screen's scrolling column. A screen's body is flattened into
 * these so a long list is drawn row by row as it scrolls, never all at once.
 */
internal sealed interface Block {
    val key: String

    /** Any node, drawn whole, with the screen's side margins. */
    data class Single(override val key: String, val node: Node, val form: String?) : Block

    /** A row or a switch, edge to edge, or inside a group's container. */
    data class Item(override val key: String, val node: Node, val group: Group, val form: String?) : Block

    /** A table's row drawn as a list row. */
    data class TableItem(override val key: String, val table: TableNode, val row: TableRow) : Block

    /** A table's filters, as a row of chips. `search` says its search box is drawn here too, not in the app bar. */
    data class Filters(override val key: String, val form: FormNode, val search: Boolean) : Block

    data class Header(override val key: String, val text: String, val subtitle: String? = null) : Block

    data class Footer(override val key: String, val text: String) : Block

    data class Space(override val key: String, val dp: Int) : Block

    /** The end of a list that has more: drawing it asks for the next page. */
    data class More(override val key: String, val list: String, val more: String) : Block

    /** A region that is still on its way. */
    data class Waiting(override val key: String) : Block

    /** A node that fills the window: an empty state that is the whole screen. */
    data class Fill(override val key: String, val node: Node) : Block
}

/** Whether the node draws anything where it stands. Overlays are drawn over the screen, not in it. */
internal fun inFlow(node: Node, local: Local): Boolean = when {
    local.isHidden(node.id, (node as? StackNode)?.hidden ?: false) -> false
    node is SheetNode || node is DialogNode || node is PopoverNode -> false
    else -> true
}

/**
 * Flattens a body into blocks. Vertical stacks, lists, regions, plain and
 * inset sections, forms and (on a narrow width) tables are taken apart;
 * everything else is one block.
 *
 * `omit` is a node drawn elsewhere (a form's button that the app bar holds).
 * `hoisted` is the query whose search box the app bar holds.
 */
internal fun blocks(
    body: Node?,
    local: Local,
    widthDp: Int,
    omit: Node? = null,
    hoisted: FormNode? = null,
): List<Block> {
    val out = ArrayList<Block>()
    val used = HashSet<String>()

    fun key(path: String, id: String?): String {
        val wanted = id?.let { "id:$it" } ?: path
        return if (used.add(wanted)) wanted else path.also { used.add(it) }
    }

    fun groupable(node: Node): Boolean = node is RowNode || (node is FieldNode && node.input is ToggleInput)

    fun add(node: Node, path: String, form: String?, region: String?, top: Boolean) {
        if (node === omit || !inFlow(node, local)) return
        when (node) {
            is StackNode -> if (node.axis == Axis.Vertical) {
                val gap = ContentRules.gap(node.gap)
                var any = false
                node.children.forEachIndexed { index, child ->
                    if (child !== omit && inFlow(child, local)) {
                        val before = out.size
                        if (any && gap > 0) out += Block.Space(key("$path/$index/gap", null), gap)
                        val spaced = out.size
                        add(child, "$path/$index", form, region, top = top && node.children.count { inFlow(it, local) } == 1)
                        // A part that drew nothing takes no room either.
                        if (out.size == spaced) {
                            if (spaced > before) out.removeAt(out.lastIndex)
                        } else {
                            any = true
                        }
                    }
                }
            } else {
                // A row of parts whose only part is drawn elsewhere draws nothing.
                if (node.children.any { it !== omit && inFlow(it, local) }) out += Block.Single(key(path, node.id), node, form)
            }
            is RegionNode -> {
                val shown = node.children.filter { inFlow(it, local) }
                if (shown.isEmpty() && node.lazy) {
                    out += Block.Waiting(key("$path/waiting", null))
                } else {
                    node.children.forEachIndexed { index, child -> add(child, "$path/$index", form, node.id ?: region, top = top && shown.size == 1) }
                }
            }
            is ListNode -> {
                node.children.forEachIndexed { index, child ->
                    if (inFlow(child, local)) {
                        if (child is RowNode) {
                            out += Block.Item(key("$path/$index", child.id), child, Group.None, form)
                        } else {
                            add(child, "$path/$index", form, region, top = false)
                        }
                    }
                }
                val id = node.id
                val more = node.more
                if (more != null && id != null) out += Block.More(key("$path/more", null), id, more)
            }
            is SectionNode -> if (node.style == SectionStyle.Card) {
                out += Block.Single(key(path, node.id), node, form)
            } else {
                val title = node.title
                if (title != null) out += Block.Header(key("$path/header", null), title, node.subtitle)
                if (node.actions.isNotEmpty()) {
                    out += Block.Single(key("$path/actions", null), StackNode(axis = Axis.Horizontal, gap = 2, wrap = true, children = node.actions), form)
                }
                // A list inside a section gives the section its rows.
                val items = node.children.filter { inFlow(it, local) }
                    .flatMap { child -> if (child is ListNode && child.more == null) child.children.filter { inFlow(it, local) } else listOf(child) }
                if (node.style == SectionStyle.Inset && items.isNotEmpty() && items.all(::groupable)) {
                    items.forEachIndexed { index, item ->
                        val group = when {
                            items.size == 1 -> Group.Only
                            index == 0 -> Group.First
                            index == items.lastIndex -> Group.Last
                            else -> Group.Middle
                        }
                        out += Block.Item(key("$path/$index", item.id), item, group, form)
                    }
                } else {
                    var any = title != null
                    node.children.forEachIndexed { index, child ->
                        if (inFlow(child, local)) {
                            if (any && child !is RowNode) out += Block.Space(key("$path/$index/gap", null), 8)
                            add(child, "$path/$index", form, region, top = false)
                            any = true
                        }
                    }
                }
                node.footer.forEachIndexed { index, child -> add(child, "$path/footer/$index", form, region, top = false) }
            }
            is FormNode -> node.children.forEachIndexed { index, child -> add(child, "$path/$index", node.key, region, top = false) }
            is TableNode -> {
                val query = node.query
                if (query != null) out += Block.Filters(key("$path/filters", null), query, search = query !== hoisted)
                val empty = node.empty
                if (node.rows.isEmpty() && empty != null) {
                    out += Block.Single(key("$path/empty", empty.id), empty, form)
                } else if (ListRules.tableAsList(widthDp, node.columns.size)) {
                    node.rows.forEachIndexed { index, row -> out += Block.TableItem(key("$path/$index", row.id), node, row) }
                } else {
                    out += Block.Single(key(path, node.id), node, form)
                }
                val id = node.id ?: region
                val more = node.more
                if (more != null && id != null) out += Block.More(key("$path/more", null), id, more)
            }
            is EmptyNode -> out += if (top) Block.Fill(key(path, node.id), node) else Block.Single(key(path, node.id), node, form)
            // A row and a switch are list items: they keep their own insets and run edge to edge.
            is RowNode -> out += Block.Item(key(path, node.id), node, Group.None, form)
            is FieldNode -> out += if (node.input is ToggleInput) {
                Block.Item(key(path, node.id), node, Group.None, form)
            } else {
                Block.Single(key(path, node.id), node, form)
            }
            else -> out += Block.Single(key(path, node.id), node, form)
        }
    }

    if (body != null) add(body, "b", null, null, top = true)
    return out
}

/** The index of the block that holds the node with this id, or the field asked for as `form/name`. */
internal fun blockOf(blocks: List<Block>, target: String): Int = blocks.indexOfFirst { block ->
    val node = when (block) {
        is Block.Single -> block.node
        is Block.Item -> block.node
        is Block.Fill -> block.node
        else -> null
    }
    val form = when (block) {
        is Block.Single -> block.form
        is Block.Item -> block.form
        else -> null
    }
    node != null && holds(node, form, target)
}

private fun holds(node: Node, form: String?, target: String): Boolean {
    if (node.id == target) return true
    if (node is FieldNode && "$form/${node.name}" == target) return true
    val inner = if (node is FormNode) node.key else form
    val children = when (node) {
        is StackNode -> node.children
        is SectionNode -> node.children
        is FormNode -> node.children
        is ListNode -> node.children
        is RegionNode -> node.children
        else -> emptyList()
    }
    return children.any { holds(it, inner, target) }
}
