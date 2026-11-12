package com.navree.client.design

import com.navree.protocol.Navree

/*
 * Reading a tree the core has made ready: what a node holds, its id, and the
 * few things the placement rules look for. Nothing here changes a tree.
 */

/** The node's id, when it has one. */
val Navree.Node.nodeId: String?
    get() = when (this) {
        is Navree.Stack -> id
        is Navree.Grid -> id
        is Navree.Section -> id
        is Navree.List -> id
        is Navree.Row -> id
        is Navree.Split -> id
        is Navree.Tabs -> id
        is Navree.Disclosure -> id
        is Navree.Divider -> id
        is Navree.Spacer -> id
        is Navree.Region -> id
        is Navree.Text -> id
        is Navree.Rich -> id
        is Navree.Image -> id
        is Navree.Icon -> id
        is Navree.Avatar -> id
        is Navree.Badge -> id
        is Navree.Progress -> id
        is Navree.Banner -> id
        is Navree.Empty -> id
        is Navree.Facts -> id
        is Navree.Table -> id
        is Navree.Chart -> id
        is Navree.Code -> id
        is Navree.Button -> id
        is Navree.Link -> id
        is Navree.Menu -> id
        is Navree.Form -> id
        is Navree.Field -> id
        is Navree.Sheet -> id
        is Navree.Dialog -> id
        is Navree.Popover -> id
        is Navree.NativeView -> id
        is Navree.WebRegion -> id
        is Navree.Toolbar -> id
        is Navree.Gauge -> id
        is Navree.Figure -> id
        is Navree.Timeline -> id
        is Navree.FeedRow -> id
        is Navree.Message -> id
        is Navree.Pane -> id
        is Navree.Node.Unknown -> id
        else -> null
    }

/** The nodes directly inside this one, in order, overlays included. */
fun Navree.Node.childNodes(): List<Navree.Node> = when (this) {
    is Navree.Stack -> children
    is Navree.Grid -> children
    is Navree.Section -> actions + children + footer
    is Navree.List -> children
    is Navree.Row -> listOfNotNull(leading, trailing, badge) + detail + menu + swipe
    is Navree.Split -> listOf(primary, detail)
    is Navree.Tabs -> items.flatMap { it.children }
    is Navree.Disclosure -> children
    is Navree.Region -> children
    is Navree.Banner -> listOfNotNull(icon) + actions
    is Navree.Empty -> listOfNotNull(icon) + actions
    is Navree.Facts -> items.flatMap { it.value }
    is Navree.Table -> listOfNotNull(query, empty) + rows.flatMap { it.cells + it.actions }
    is Navree.Link -> children
    is Navree.Menu -> items
    is Navree.Form -> children
    is Navree.Sheet -> children + actions
    is Navree.Dialog -> children + actions
    is Navree.Popover -> children
    is Navree.Toolbar -> items.flatMap { it.menu }
    is Navree.Timeline -> items.flatMap { it.detail }
    is Navree.FeedRow -> detail
    is Navree.Message -> body
    is Navree.Pane -> children
    is Navree.NativeView -> listOfNotNull(alt)
    is Navree.Node.Unknown -> listOfNotNull(alt)
    else -> emptyList()
}

/** This node and every node inside it, depth first, in order. */
fun Navree.Node.walk(): Sequence<Navree.Node> = sequence {
    val stack = ArrayDeque<Navree.Node>()
    stack.addLast(this@walk)
    while (stack.isNotEmpty()) {
        val node = stack.removeLast()
        yield(node)
        val inner = node.childNodes()
        for (index in inner.indices.reversed()) stack.addLast(inner[index])
    }
}

/** The fields of a form, nested ones included, but not those of a form inside it. */
fun Navree.Form.fields(): List<Navree.Field> {
    val out = ArrayList<Navree.Field>()
    fun visit(node: Navree.Node) {
        when (node) {
            is Navree.Field -> out += node
            is Navree.Form -> Unit
            else -> node.childNodes().forEach(::visit)
        }
    }
    children.forEach(::visit)
    return out
}

/** The button that sends the form: the last one whose action submits it. */
fun Navree.Form.submitButton(): Navree.Button? {
    var found: Navree.Button? = null
    fun visit(node: Navree.Node) {
        when (node) {
            is Navree.Button -> if (node.action is Navree.Action.Submit && (node.action as Navree.Action.Submit).form.let { it == null || it == id }) found = node
            is Navree.Form, is Navree.Sheet, is Navree.Dialog, is Navree.Popover -> Unit
            else -> node.childNodes().forEach(::visit)
        }
    }
    children.forEach(::visit)
    return found
}

/** The sheets, dialogs and popovers of a screen, wherever they stand in it. */
fun Navree.Screen.overlays(): List<Navree.Node> =
    (sequenceOf(body) + actions.asSequence()).flatMap { it.walk() }
        .filter { it is Navree.Sheet || it is Navree.Dialog || it is Navree.Popover }
        .toList()

/** The role a node plays as an action: a button's, a toolbar item's, or none. */
val Navree.Node.role: Navree.Role
    get() = when (this) {
        is Navree.Button -> role
        else -> Navree.Role.NONE
    }

/** Whether this action writes: a post, a submit of a form that posts, or a request that is not a plain read. */
fun Navree.Action?.writes(): Boolean = when (this) {
    is Navree.Action.Request -> method != Navree.Method.GET
    is Navree.Action.Submit -> true
    else -> false
}
