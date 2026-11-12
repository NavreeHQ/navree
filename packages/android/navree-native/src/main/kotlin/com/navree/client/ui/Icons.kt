package com.navree.client.ui

import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.graphics.vector.addPathNodes
import androidx.compose.ui.unit.dp
import com.navree.client.design.IconPath
import com.navree.client.design.IconShapes
import com.navree.client.protocol.IconNode

/**
 * The few glyphs the client draws for its own controls: back, close, search
 * and the like. They are the Material icons, as path data, so the client
 * needs no icon library. The back arrow mirrors in right-to-left layouts.
 */
internal object Glyphs {
    private fun filled(data: String, mirror: Boolean = false): ImageVector =
        ImageVector.Builder(
            defaultWidth = 24.dp,
            defaultHeight = 24.dp,
            viewportWidth = 24f,
            viewportHeight = 24f,
            autoMirror = mirror,
        ).addPath(pathData = addPathNodes(data), fill = SolidColor(Color.Black)).build()

    val Back by lazy { filled("M20 11H7.83l5.59-5.59L12 4l-8 8 8 8 1.41-1.41L7.83 13H20v-2z", mirror = true) }
    val Close by lazy { filled("M19 6.41L17.59 5 12 10.59 6.41 5 5 6.41 10.59 12 5 17.59 6.41 19 12 13.41 17.59 19 19 17.59 13.41 12z") }
    val More by lazy {
        filled(
            "M12 8c1.1 0 2-.9 2-2s-.9-2-2-2-2 .9-2 2 .9 2 2 2zm0 2c-1.1 0-2 .9-2 2s.9 2 2 2 2-.9 2-2-.9-2-2-2z" +
                "m0 6c-1.1 0-2 .9-2 2s.9 2 2 2 2-.9 2-2-.9-2-2-2z",
        )
    }
    val Search by lazy {
        filled(
            "M15.5 14h-.79l-.28-.27C15.41 12.59 16 11.11 16 9.5 16 5.91 13.09 3 9.5 3S3 5.91 3 9.5 5.91 16 9.5 16" +
                "c1.61 0 3.09-.59 4.23-1.57l.27.28v.79l5 4.99L20.49 19l-4.99-5zm-6 0C7.01 14 5 11.99 5 9.5S7.01 5 9.5 5 14 7.01 14 9.5 11.99 14 9.5 14z",
        )
    }
    val Menu by lazy { filled("M3 18h18v-2H3v2zm0-5h18v-2H3v2zm0-7v2h18V6H3z") }
    val Add by lazy { filled("M19 13h-6v6h-2v-6H5v-2h6V5h2v6h6v2z") }
    val Remove by lazy { filled("M19 13H5v-2h14v2z") }
    val Check by lazy { filled("M9 16.17L4.83 12l-1.42 1.41L9 19 21 7l-1.41-1.41z") }
    val ExpandMore by lazy { filled("M16.59 8.59L12 13.17 7.41 8.59 6 10l6 6 6-6z") }
    val Calendar by lazy {
        filled("M20 3h-1V1h-2v2H7V1H5v2H4c-1.1 0-2 .9-2 2v16c0 1.1.9 2 2 2h16c1.1 0 2-.9 2-2V5c0-1.1-.9-2-2-2zm0 18H4V8h16v13z")
    }
    val Clock by lazy {
        filled(
            "M11.99 2C6.47 2 2 6.48 2 12s4.47 10 9.99 10C17.52 22 22 17.52 22 12S17.52 2 11.99 2zM12 20c-4.42 0-8-3.58-8-8s3.58-8 8-8 8 3.58 8 8-3.58 8-8 8z" +
                "m.5-13H11v6l5.25 3.15.75-1.23-4.5-2.67z",
        )
    }
    val Visibility by lazy {
        filled(
            "M12 4.5C7 4.5 2.73 7.61 1 12c1.73 4.39 6 7.5 11 7.5s9.27-3.11 11-7.5c-1.73-4.39-6-7.5-11-7.5zM12 17c-2.76 0-5-2.24-5-5s2.24-5 5-5 5 2.24 5 5-2.24 5-5 5z" +
                "m0-8c-1.66 0-3 1.34-3 3s1.34 3 3 3 3-1.34 3-3-1.34-3-3-3z",
        )
    }
    val VisibilityOff by lazy {
        filled(
            "M12 7c2.76 0 5 2.24 5 5 0 .65-.13 1.26-.36 1.83l2.92 2.92c1.51-1.26 2.7-2.89 3.43-4.75-1.73-4.39-6-7.5-11-7.5-1.4 0-2.74.25-3.98.7l2.16 2.16C10.74 7.13 11.35 7 12 7z" +
                "M2 4.27l2.28 2.28.46.46C3.08 8.3 1.78 10.02 1 12c1.73 4.39 6 7.5 11 7.5 1.55 0 3.03-.3 4.38-.84l.42.42L19.73 22 21 20.73 3.27 3 2 4.27z" +
                "M7.53 9.8l1.55 1.55c-.05.21-.08.43-.08.65 0 1.66 1.34 3 3 3 .22 0 .44-.03.65-.08l1.55 1.55c-.67.33-1.41.53-2.2.53-2.76 0-5-2.24-5-5 0-.79.2-1.53.53-2.2z" +
                "m4.31-.78l3.15 3.15.02-.16c0-1.66-1.34-3-3-3l-.17.01z",
        )
    }
    val Error by lazy { filled("M12 2C6.48 2 2 6.48 2 12s4.48 10 10 10 10-4.48 10-10S17.52 2 12 2zm1 15h-2v-2h2v2zm0-4h-2V7h2v6z") }
    val ArrowUp by lazy { filled("M4 12l1.41 1.41L11 7.83V20h2V7.83l5.58 5.59L20 12l-8-8-8 8z") }
    val ArrowDown by lazy { filled("M20 12l-1.41-1.41L13 16.17V4h-2v12.17l-5.58-5.59L4 12l8 8 8-8z") }
    val CloudOff by lazy {
        filled(
            "M19.35 10.04C18.67 6.59 15.64 4 12 4c-1.48 0-2.85.43-4.01 1.17l1.46 1.46C10.21 6.23 11.08 6 12 6c3.04 0 5.5 2.46 5.5 5.5v.5H19c1.66 0 3 1.34 3 3 0 1.13-.64 2.11-1.56 2.62l1.45 1.45C23.16 18.16 24 16.68 24 15c0-2.64-2.05-4.78-4.65-4.96z" +
                "M3 5.27l2.75 2.74C2.56 8.15 0 10.77 0 14c0 3.31 2.69 6 6 6h11.73l2 2L21 20.73 4.27 4 3 5.27zM7.73 10l8 8H6c-2.21 0-4-1.79-4-4s1.79-4 4-4h1.73z",
        )
    }
}

/**
 * Builds the vector of an icon: its `paths` stroked at a width of 2 with
 * round caps and joins, its `fills` filled, in a 24 by 24 box unless it names
 * another.
 */
internal fun vectorOf(icon: IconNode): ImageVector? {
    val legacy = IconShapes.read(icon.shapes)
    val strokes = icon.paths.map { IconPath(it) } + legacy
    val fills = icon.fills
    if (strokes.isEmpty() && fills.isEmpty()) return null
    val box = icon.viewBox?.trim()?.split(Regex("[\\s,]+"))?.mapNotNull { it.toFloatOrNull() }?.takeIf { it.size == 4 }
    val builder = ImageVector.Builder(
        defaultWidth = 24.dp,
        defaultHeight = 24.dp,
        viewportWidth = box?.get(2) ?: 24f,
        viewportHeight = box?.get(3) ?: 24f,
    )
    var drawn = 0
    fun add(data: String, fill: Boolean, stroke: Boolean) {
        val nodes = try {
            addPathNodes(data)
        } catch (_: RuntimeException) {
            // Path data that cannot be read is left out; the rest of the icon is still drawn.
            return
        }
        if (nodes.isEmpty()) return
        builder.addPath(
            pathData = nodes,
            fill = if (fill) SolidColor(Color.Black) else null,
            stroke = if (stroke) SolidColor(Color.Black) else null,
            strokeLineWidth = 2f,
            strokeLineCap = StrokeCap.Round,
            strokeLineJoin = StrokeJoin.Round,
        )
        drawn++
    }
    for (path in strokes) add(path.data, fill = path.fill, stroke = true)
    for (data in fills) add(data, fill = true, stroke = false)
    return if (drawn == 0) null else builder.build()
}

/** The vector of an icon, built once for as long as the icon stays the same. */
@Composable
internal fun rememberVector(icon: IconNode?): ImageVector? = remember(icon) { icon?.let(::vectorOf) }
