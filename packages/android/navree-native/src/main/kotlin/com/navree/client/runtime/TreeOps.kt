package com.navree.client.runtime

import com.navree.client.protocol.DialogNode
import com.navree.client.protocol.DisclosureNode
import com.navree.client.protocol.FormNode
import com.navree.client.protocol.GridNode
import com.navree.client.protocol.ListNode
import com.navree.client.protocol.MenuNode
import com.navree.client.protocol.Node
import com.navree.client.protocol.Op
import com.navree.client.protocol.PopoverNode
import com.navree.client.protocol.RegionNode
import com.navree.client.protocol.Screen
import com.navree.client.protocol.SectionNode
import com.navree.client.protocol.SheetNode
import com.navree.client.protocol.StackNode
import com.navree.client.protocol.TableNode

/**
 * The ops that change a tree, as pure functions of a screen. These are the
 * rules the behaviour vectors pin down; every client applies them the same.
 *
 * - `morph` and `replace` put the new node where the old one was. (What they
 *   do to typed values differs, and that is local state, not the tree.)
 * - `append` and `prepend` add nodes to a node's children. An `append` to a
 *   list also sets the list's `more`: the address of the page after, or
 *   nothing when that was the last page.
 * - `remove` takes the node out; a table's row goes by its row id.
 * - An op whose target is not in the tree changes nothing. Ids are not always
 *   unique on a page; every node with the id is changed.
 */
object TreeOps {
    /** The screen after `op`. Ops that do not touch the tree return it as it was. */
    fun apply(screen: Screen, op: Op): Screen = when (op) {
        is Op.Morph -> screen.changed(op.target) { listOf(op.node) }
        is Op.Replace -> screen.changed(op.target) { listOf(op.node) }
        is Op.Append -> screen.changed(op.target) { listOf(add(it, op.nodes, atEnd = true, more = op.more)) }
        is Op.Prepend -> screen.changed(op.target) { listOf(add(it, op.nodes, atEnd = false, more = null)) }
        is Op.Remove -> screen.changed(op.target) { emptyList() }.withoutTableRow(op.target)
        else -> screen
    }

    /** Whether `op` is one of the five that change the tree. */
    fun changesTree(op: Op): Boolean =
        op is Op.Morph || op is Op.Replace || op is Op.Append || op is Op.Prepend || op is Op.Remove

    /** The screen with the node `id` put through `change`, wherever it is: an action or part of the body. */
    fun rewrite(screen: Screen, id: String, change: (Node) -> List<Node>): Screen = screen.changed(id, change)

    /**
     * The next page of a table, taken from the screen the server answered
     * with: the table's rows are added and its `more` taken. `id` is the
     * table's own id or the id of the region it stands in.
     */
    fun addPage(screen: Screen, id: String, page: TableNode): Screen = screen.changed(id) { node ->
        listOf(
            when (node) {
                is TableNode -> node.copy(rows = node.rows + page.rows, more = page.more, summary = page.summary ?: node.summary)
                is RegionNode -> node.copy(
                    children = node.children.map { child ->
                        if (child is TableNode) child.copy(rows = child.rows + page.rows, more = page.more, summary = page.summary ?: child.summary) else child
                    },
                )
                else -> node
            },
        )
    }

    private fun Screen.changed(id: String, change: (Node) -> List<Node>): Screen {
        val newActions = actions.flatMap { action ->
            if (action.id == id) change(action) else listOf(action.rewrite(id, change))
        }
        val newBody = body?.let { root ->
            if (root.id == id) change(root).firstOrNull() else root.rewrite(id, change)
        }
        return copy(actions = newActions, body = newBody)
    }

    private fun Screen.withoutTableRow(id: String): Screen {
        fun strip(node: Node): Node {
            val inner = node.mapChildren { listOf(strip(it)) }
            return if (inner is TableNode && inner.rows.any { it.id == id }) {
                inner.copy(rows = inner.rows.filterNot { it.id == id })
            } else {
                inner
            }
        }
        return copy(body = body?.let(::strip))
    }

    private fun <T> joined(old: List<T>, new: List<T>, atEnd: Boolean): List<T> = if (atEnd) old + new else new + old

    private fun add(parent: Node, nodes: List<Node>, atEnd: Boolean, more: String?): Node = when (parent) {
        is ListNode -> parent.copy(children = joined(parent.children, nodes, atEnd), more = if (atEnd) more else parent.more)
        is StackNode -> parent.copy(children = joined(parent.children, nodes, atEnd))
        is GridNode -> parent.copy(children = joined(parent.children, nodes, atEnd))
        is SectionNode -> parent.copy(children = joined(parent.children, nodes, atEnd))
        is DisclosureNode -> parent.copy(children = joined(parent.children, nodes, atEnd))
        is RegionNode -> parent.copy(children = joined(parent.children, nodes, atEnd))
        is FormNode -> parent.copy(children = joined(parent.children, nodes, atEnd))
        is SheetNode -> parent.copy(children = joined(parent.children, nodes, atEnd))
        is DialogNode -> parent.copy(children = joined(parent.children, nodes, atEnd))
        is PopoverNode -> parent.copy(children = joined(parent.children, nodes, atEnd))
        is MenuNode -> parent.copy(items = joined(parent.items, nodes, atEnd))
        else -> parent
    }
}
