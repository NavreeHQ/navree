package com.navree.client.runtime

import com.navree.client.protocol.ChartNode
import com.navree.client.protocol.ColourInput
import com.navree.client.protocol.FieldNode
import com.navree.client.protocol.FileInput
import com.navree.client.protocol.NativeNode
import com.navree.client.protocol.Node
import com.navree.client.protocol.Screen
import com.navree.client.protocol.UnknownInput
import com.navree.client.protocol.UnknownNode
import com.navree.client.protocol.WebNode

/**
 * What this build of the client cannot draw. A screen that holds any of it,
 * with no `alt` to draw in its place, is shown whole in the web view: a page
 * is never drawn with a hole in it.
 */
object Coverage {
    /** The parts of the screen that cannot be drawn, named as a coverage report names them. */
    fun missing(screen: Screen): List<String> = screen.walk().mapNotNull(::missing).distinct().toList()

    private fun missing(node: Node): String? = when (node) {
        is UnknownNode -> if (node.alt == null) "kind:${node.kind}" else null
        is WebNode -> "kind:web"
        is NativeNode -> if (node.alt == null) "native:${node.name}" else null
        is ChartNode -> "kind:chart"
        is FieldNode -> when (val input = node.input) {
            is UnknownInput -> "input:${input.kind}"
            is ColourInput -> "input:colour"
            is FileInput -> "input:file"
            else -> null
        }
        else -> null
    }
}
