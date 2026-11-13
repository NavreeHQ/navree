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
import com.navree.client.design.Symbols
import com.navree.protocol.Navree
import java.util.concurrent.ConcurrentHashMap

/**
 * A Material Symbol as a vector: its path data sits in a 960 by 960 box whose
 * top is at -960, so the group moves it down into the box. Symbols that point
 * one way (back, forward) mirror in right-to-left layouts.
 */
internal fun symbolVector(symbol: String, filled: Boolean = false): ImageVector? {
    val key = if (filled) "$symbol:filled" else symbol
    built[key]?.let { return it }
    val data = Symbols.paths(symbol, filled) ?: return null
    val nodes = runCatching { addPathNodes(data) }.getOrNull() ?: return null
    val vector = ImageVector.Builder(
        name = key,
        defaultWidth = 24.dp,
        defaultHeight = 24.dp,
        viewportWidth = 960f,
        viewportHeight = 960f,
        autoMirror = symbol in MIRRORED,
    ).addGroup(translationY = 960f)
        .addPath(pathData = nodes, fill = SolidColor(Color.Black))
        .clearGroup()
        .build()
    built[key] = vector
    return vector
}

private val built = ConcurrentHashMap<String, ImageVector>()
private val MIRRORED = setOf("arrow_back", "arrow_forward", "chevron_right", "send", "undo", "logout", "open_in_new")

/** The client's own glyphs, all Material Symbols. */
internal object Glyphs {
    val Back get() = symbolVector("arrow_back")!!
    val Close get() = symbolVector("close")!!
    val More get() = symbolVector("more_vert")!!
    val Search get() = symbolVector("search")!!
    val Menu get() = symbolVector("menu")!!
    val Add get() = symbolVector("add")!!
    val Check get() = symbolVector("check")!!
    val ExpandMore get() = symbolVector("expand_more")!!
    val Calendar get() = symbolVector("calendar_today")!!
    val Clock get() = symbolVector("schedule")!!
    val Visibility get() = symbolVector("visibility")!!
    val VisibilityOff get() = symbolVector("visibility_off")!!
    val Error get() = symbolVector("error", filled = true)!!
    val ArrowUp get() = symbolVector("arrow_upward")!!
    val ArrowDown get() = symbolVector("arrow_downward")!!
    val CloudOff get() = symbolVector("cloud_off")!!
    val History get() = symbolVector("history")!!
    val DragHandle get() = symbolVector("drag_indicator")!!
    val CheckBox get() = symbolVector("check_box", filled = true)!!
    val CheckBoxOff get() = symbolVector("check_box_outline_blank")!!
    val RadioOn get() = symbolVector("radio_button_checked")!!
    val RadioOff get() = symbolVector("radio_button_unchecked")!!
    val DateRange get() = symbolVector("date_range")!!
    val Help get() = symbolVector("help")!!
    val Info get() = symbolVector("info")!!
    val ChevronRight get() = symbolVector("chevron_right")!!
    val Today get() = symbolVector("calendar_today")!!
}

/**
 * The page's own shape of an icon: its `paths` stroked at a width of 2 with
 * round caps and joins, its `fills` filled, in a 24 by 24 box unless it names
 * another.
 */
internal fun shapeVector(icon: Navree.Icon): ImageVector? {
    if (icon.paths.isEmpty() && icon.fills.isEmpty()) return null
    val box = icon.viewBox?.trim()?.split(Regex("[\\s,]+"))?.mapNotNull { it.toFloatOrNull() }?.takeIf { it.size == 4 }
    val builder = ImageVector.Builder(
        defaultWidth = 24.dp,
        defaultHeight = 24.dp,
        viewportWidth = box?.get(2) ?: 24f,
        viewportHeight = box?.get(3) ?: 24f,
    )
    var drawn = 0
    fun add(data: String, fill: Boolean, stroke: Boolean) {
        // Path data that cannot be read is left out; the rest of the icon is still drawn.
        val nodes = runCatching { addPathNodes(data) }.getOrNull() ?: return
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
    for (data in icon.paths) add(data, fill = false, stroke = true)
    for (data in icon.fills) add(data, fill = true, stroke = false)
    return if (drawn == 0) null else builder.build()
}

/**
 * The vector to draw for an icon: the system's symbol for a standard action;
 * then the Material Symbol the server named for the icon (the protocol's own
 * table); then the symbol the client's table maps the icon's name to; filled
 * when selected; and only then the page's own shape.
 */
internal fun iconVector(icon: Navree.Icon?, standard: Navree.Standard? = null, selected: Boolean = false): ImageVector? {
    standard?.let { return symbolVector(Symbols.of(it), selected) }
    if (icon == null) return null
    icon.symbol?.material?.takeIf(Symbols::has)?.let { symbol -> symbolVector(symbol, selected)?.let { return it } }
    Symbols.of(icon.name)?.takeIf(Symbols::has)?.let { symbol -> symbolVector(symbol, selected)?.let { return it } }
    return shapeVector(icon)
}

@Composable
internal fun rememberIcon(icon: Navree.Icon?, standard: Navree.Standard? = null, selected: Boolean = false): ImageVector? =
    remember(icon, standard, selected) { iconVector(icon, standard, selected) }
