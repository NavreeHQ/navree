package com.navree.client.design

import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive

/** One stroke of an icon as path data in the 24 by 24 box. `fill` says the shape is filled as well as stroked. */
data class IconPath(val data: String, val fill: Boolean = false)

/**
 * Reads an icon's shapes into path data. The shapes may come as a list of
 * path data, as a list of shape objects, or as the SVG markup of the shapes
 * (`<path d="…"/><circle cx="12" cy="12" r="3"/>`). Circles, ellipses,
 * rectangles, lines, polylines and polygons become the paths that draw them.
 * Anything that is not a shape is skipped: nothing here is run or loaded.
 */
object IconShapes {
    fun read(shapes: JsonElement?): List<IconPath> = when (shapes) {
        null, is JsonNull -> emptyList()
        is JsonPrimitive -> markup(shapes.content)
        is JsonArray -> shapes.flatMap { item ->
            when (item) {
                is JsonNull -> emptyList()
                is JsonPrimitive -> listOf(IconPath(item.content))
                is JsonObject -> listOfNotNull(shape(item))
                else -> emptyList()
            }
        }
        is JsonObject -> listOfNotNull(shape(shapes))
    }

    private val ELEMENT = Regex("<([a-zA-Z]+)\\b([^>]*)>")
    private val ATTRIBUTE = Regex("([a-zA-Z][a-zA-Z0-9-]*)\\s*=\\s*\"([^\"]*)\"")

    /** The shapes in a run of SVG elements. */
    fun markup(text: String): List<IconPath> = ELEMENT.findAll(text).mapNotNull { element ->
        val attributes = ATTRIBUTE.findAll(element.groupValues[2]).associate { it.groupValues[1] to it.groupValues[2] }
        shape(element.groupValues[1].lowercase(), attributes)
    }.toList()

    private fun shape(shape: JsonObject): IconPath? {
        val attributes = shape.mapNotNull { (key, value) ->
            (value as? JsonPrimitive)?.takeIf { it !is JsonNull }?.let { key to it.content }
        }.toMap()
        val kind = attributes["t"] ?: attributes["shape"] ?: if ("d" in attributes) "path" else return null
        return shape(kind, attributes)
    }

    private fun shape(kind: String, a: Map<String, String>): IconPath? {
        fun n(name: String): Double = a[name]?.toDoubleOrNull() ?: 0.0
        val fill = a["fill"]?.let { it != "none" && it != "false" } ?: false
        val data = when (kind) {
            "path" -> a["d"]
            "circle" -> ellipse(n("cx"), n("cy"), n("r"), n("r"))
            "ellipse" -> ellipse(n("cx"), n("cy"), n("rx"), n("ry"))
            "line" -> "M${f(n("x1"))} ${f(n("y1"))}L${f(n("x2"))} ${f(n("y2"))}"
            "rect" -> rect(n("x"), n("y"), n("width"), n("height"), a["rx"]?.toDoubleOrNull(), a["ry"]?.toDoubleOrNull())
            "polyline" -> points(a["points"], close = false)
            "polygon" -> points(a["points"], close = true)
            else -> null
        }
        return data?.takeIf { it.isNotBlank() }?.let { IconPath(it, fill) }
    }

    private fun f(value: Double): String =
        if (value == Math.rint(value)) value.toLong().toString() else value.toString()

    private fun ellipse(cx: Double, cy: Double, rx: Double, ry: Double): String? {
        if (rx <= 0.0 || ry <= 0.0) return null
        return "M${f(cx - rx)} ${f(cy)}a${f(rx)} ${f(ry)} 0 1 0 ${f(rx * 2)} 0a${f(rx)} ${f(ry)} 0 1 0 ${f(-rx * 2)} 0Z"
    }

    private fun rect(x: Double, y: Double, width: Double, height: Double, rxGiven: Double?, ryGiven: Double?): String? {
        if (width <= 0.0 || height <= 0.0) return null
        val rx = (rxGiven ?: ryGiven ?: 0.0).coerceIn(0.0, width / 2)
        val ry = (ryGiven ?: rxGiven ?: 0.0).coerceIn(0.0, height / 2)
        if (rx == 0.0 || ry == 0.0) return "M${f(x)} ${f(y)}h${f(width)}v${f(height)}h${f(-width)}Z"
        val arc = "a${f(rx)} ${f(ry)} 0 0 1"
        return "M${f(x + rx)} ${f(y)}" +
            "h${f(width - 2 * rx)}$arc ${f(rx)} ${f(ry)}" +
            "v${f(height - 2 * ry)}$arc ${f(-rx)} ${f(ry)}" +
            "h${f(-(width - 2 * rx))}$arc ${f(-rx)} ${f(-ry)}" +
            "v${f(-(height - 2 * ry))}$arc ${f(rx)} ${f(-ry)}Z"
    }

    private fun points(text: String?, close: Boolean): String? {
        val numbers = text?.split(Regex("[\\s,]+"))?.mapNotNull { it.toDoubleOrNull() } ?: return null
        if (numbers.size < 4) return null
        val pairs = numbers.chunked(2).filter { it.size == 2 }
        val path = pairs.mapIndexed { index, (x, y) -> (if (index == 0) "M" else "L") + "${f(x)} ${f(y)}" }.joinToString("")
        return if (close) path + "Z" else path
    }
}
