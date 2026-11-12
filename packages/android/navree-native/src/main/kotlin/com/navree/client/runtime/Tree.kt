package com.navree.client.runtime

import com.navree.client.protocol.AvatarNode
import com.navree.client.protocol.BadgeNode
import com.navree.client.protocol.BannerNode
import com.navree.client.protocol.ButtonNode
import com.navree.client.protocol.ChartNode
import com.navree.client.protocol.CodeNode
import com.navree.client.protocol.DialogNode
import com.navree.client.protocol.DisclosureNode
import com.navree.client.protocol.DividerNode
import com.navree.client.protocol.EmptyNode
import com.navree.client.protocol.FactsNode
import com.navree.client.protocol.FieldNode
import com.navree.client.protocol.FormNode
import com.navree.client.protocol.GridNode
import com.navree.client.protocol.IconNode
import com.navree.client.protocol.ImageNode
import com.navree.client.protocol.LinkNode
import com.navree.client.protocol.ListNode
import com.navree.client.protocol.MenuNode
import com.navree.client.protocol.NativeNode
import com.navree.client.protocol.Node
import com.navree.client.protocol.PopoverNode
import com.navree.client.protocol.ProgressNode
import com.navree.client.protocol.RegionNode
import com.navree.client.protocol.RichNode
import com.navree.client.protocol.RowNode
import com.navree.client.protocol.Screen
import com.navree.client.protocol.SectionNode
import com.navree.client.protocol.SheetNode
import com.navree.client.protocol.SpacerNode
import com.navree.client.protocol.SplitNode
import com.navree.client.protocol.StackNode
import com.navree.client.protocol.Submit
import com.navree.client.protocol.TableNode
import com.navree.client.protocol.TabsNode
import com.navree.client.protocol.TextNode
import com.navree.client.protocol.UnknownNode
import com.navree.client.protocol.WebNode

/** The nodes directly inside this one, in document order. */
fun Node.children(): List<Node> = when (this) {
    is StackNode -> children
    is GridNode -> children
    is SectionNode -> actions + children + footer
    is ListNode -> children
    is RowNode -> listOfNotNull(leading) + detail + listOfNotNull(trailing, badge) + swipe + menu
    is SplitNode -> listOfNotNull(primary, detail)
    is TabsNode -> items.flatMap { it.children }
    is DisclosureNode -> children
    is RegionNode -> children
    is BannerNode -> actions
    is EmptyNode -> actions
    is FactsNode -> items.flatMap { it.value }
    is TableNode -> listOfNotNull(query) + rows.flatMap { it.cells } + listOfNotNull(empty)
    is LinkNode -> children
    is MenuNode -> items
    is FormNode -> children
    is SheetNode -> children + actions
    is DialogNode -> children + actions
    is PopoverNode -> children
    is NativeNode -> listOfNotNull(alt)
    is UnknownNode -> listOfNotNull(alt)
    is TextNode, is RichNode, is ImageNode, is IconNode, is AvatarNode, is BadgeNode,
    is ProgressNode, is ChartNode, is CodeNode, is ButtonNode, is FieldNode,
    is DividerNode, is SpacerNode, is WebNode,
    -> emptyList()
}

/**
 * This node with each direct child put through `change`. A child may become
 * none, one or several; a slot that holds one node keeps the first.
 */
fun Node.mapChildren(change: (Node) -> List<Node>): Node {
    fun one(node: Node?): Node? = node?.let { change(it).firstOrNull() }
    fun many(nodes: List<Node>): List<Node> = nodes.flatMap(change)
    return when (this) {
        is StackNode -> copy(children = many(children))
        is GridNode -> copy(children = many(children))
        is SectionNode -> copy(actions = many(actions), children = many(children), footer = many(footer))
        is ListNode -> copy(children = many(children))
        is RowNode -> copy(
            leading = one(leading),
            detail = many(detail),
            trailing = one(trailing),
            badge = one(badge) as? BadgeNode,
            swipe = many(swipe).filterIsInstance<ButtonNode>(),
            menu = many(menu),
        )
        is SplitNode -> copy(primary = one(primary), detail = one(detail))
        is TabsNode -> copy(items = items.map { it.copy(children = many(it.children)) })
        is DisclosureNode -> copy(children = many(children))
        is RegionNode -> copy(children = many(children))
        is BannerNode -> copy(actions = many(actions))
        is EmptyNode -> copy(actions = many(actions))
        is FactsNode -> copy(items = items.map { it.copy(value = many(it.value)) })
        is TableNode -> copy(
            query = one(query) as? FormNode,
            rows = rows.map { it.copy(cells = many(it.cells)) },
            empty = one(empty),
        )
        is LinkNode -> copy(children = many(children))
        is MenuNode -> copy(items = many(items))
        is FormNode -> copy(children = many(children))
        is SheetNode -> copy(children = many(children), actions = many(actions))
        is DialogNode -> copy(children = many(children), actions = many(actions))
        is PopoverNode -> copy(children = many(children))
        is NativeNode -> copy(alt = one(alt))
        is UnknownNode -> copy(alt = one(alt))
        is TextNode, is RichNode, is ImageNode, is IconNode, is AvatarNode, is BadgeNode,
        is ProgressNode, is ChartNode, is CodeNode, is ButtonNode, is FieldNode,
        is DividerNode, is SpacerNode, is WebNode,
        -> this
    }
}

/** This node and every node under it, depth first, in document order. */
fun Node.walk(): Sequence<Node> = sequence {
    yield(this@walk)
    for (child in children()) yieldAll(child.walk())
}

/** The first node with this id, this node included. */
fun Node.find(id: String): Node? = walk().firstOrNull { it.id == id }

/**
 * This tree with every node whose id is `id` put through `change`. The root
 * is not looked at: whoever holds it does that.
 */
fun Node.rewrite(id: String, change: (Node) -> List<Node>): Node =
    mapChildren { child -> if (child.id == id) change(child) else listOf(child.rewrite(id, change)) }

/** Every node of a screen: its actions, then its body. */
fun Screen.walk(): Sequence<Node> = sequence {
    for (action in actions) yieldAll(action.walk())
    body?.let { yieldAll(it.walk()) }
}

/** The first node of the screen with this id. Ids are not always unique: see [overlay] and [form]. */
fun Screen.find(id: String): Node? = walk().firstOrNull { it.id == id }

/** The form that goes by this key: its id, or `@` and its address when it has none. */
fun Screen.form(key: String): FormNode? = walk().firstOrNull { it is FormNode && it.key == key } as? FormNode

/** The sheets, dialogs and popovers of a screen, wherever the tree holds them. */
fun Screen.overlays(): List<Node> =
    walk().filter { it is SheetNode || it is DialogNode || it is PopoverNode }.toList()

/**
 * What `open` and `close` mean by an id: a sheet, a dialog, a popover or a
 * disclosure. A form inside a dialog may carry the same id as the dialog.
 */
fun Screen.overlay(id: String): Node? = walk().firstOrNull {
    it.id == id && (it is SheetNode || it is DialogNode || it is PopoverNode || it is DisclosureNode)
}

/** The regions of a screen. */
fun Screen.regions(): List<RegionNode> = walk().filterIsInstance<RegionNode>().toList()

/** The ids of this node and of every node under it. */
fun Node.ids(): Set<String> = walk().mapNotNull { it.id }.toSet()

/** The button that sends this form: the first among its children whose action is a submit of it. */
fun FormNode.submitButton(): ButtonNode? = walk().filterIsInstance<ButtonNode>().firstOrNull { button ->
    val action = button.action
    action is Submit && (action.form == null || action.form == id)
}

/** The fields of a form, in document order. */
fun FormNode.fields(): List<FieldNode> = walk().filterIsInstance<FieldNode>().toList()
