package com.navree.client.ui

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.PathEffect
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.lerp
import androidx.compose.ui.graphics.luminance
import androidx.compose.ui.graphics.toArgb
import androidx.compose.ui.platform.LocalLayoutDirection
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.drawText
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.LayoutDirection
import androidx.compose.ui.unit.dp
import com.navree.client.design.AxisRules
import com.navree.client.design.HeatRules
import com.navree.client.design.MatrixRules
import com.navree.client.theme.Hct
import com.navree.client.theme.Hues
import com.navree.client.theme.TonalPalette
import com.navree.protocol.Navree
import java.time.LocalDate
import java.time.format.TextStyle as DateWords
import java.time.temporal.WeekFields
import java.util.Locale

/*
 * Charts on two axes, a calendar of days, a matrix and a scatter plot: drawn
 * on Compose's canvas in the theme's hues, with the page's own words for the
 * axes and a key for every series, read to TalkBack as the page's summary.
 * Time runs from the start of the line, so it runs right to left in a
 * right-to-left language.
 */

private fun DrawScope.dashed(colour: Color, from: Offset, to: Offset) =
    drawLine(colour, from, to, strokeWidth = 1.5.dp.toPx(), pathEffect = PathEffect.dashPathEffect(floatArrayOf(6.dp.toPx(), 4.dp.toPx())))

/**
 * A chart: lines, areas and bars in the series' hues on a light grid, its
 * value axis named with its unit, zones as bands behind the marks and
 * references as dashed lines with their names, the stages of a chart with
 * `levels` named down its side. A sparkline is the line alone.
 */
@Composable
internal fun ChartView(node: Navree.Chart, modifier: Modifier = Modifier) {
    val scale = textScale()
    val scheme = MaterialTheme.colorScheme
    val inks = node.series.mapIndexed { index, series -> inkOf(series.colour, SLOTS[index % SLOTS.size]) }
    val zoneInks = node.zones.map { inkOf(it.colour, Navree.Hue.NEUTRAL) }
    val referenceInks = node.references.map { inkOf(it.colour, Navree.Hue.NEUTRAL) }
    val bounds = remember(node) { AxisRules.values(node) }
    val growth = grown(1f)
    val measurer = rememberTextMeasurer()
    val small = MaterialTheme.typography.labelSmall.copy(color = scheme.onSurfaceVariant)
    val rtl = LocalLayoutDirection.current == LayoutDirection.Rtl
    val spark = node.kind == Navree.ChartKind.SPARKLINE
    val said = node.summary ?: node.series.joinToString("; ") { series ->
        series.label + ": " + node.labels.indices.joinToString(", ") { at ->
            val shown = series.shown.getOrNull(at) ?: series.values.getOrNull(at)?.let(::amount) ?: "–"
            (node.labels.getOrNull(at)?.let { "$it $shown" } ?: shown)
        }
    }
    val unit = listOfNotNull(node.y?.name, node.y?.unit ?: node.series.firstOrNull()?.unit).joinToString(", ").ifEmpty { null }
    Column(modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(8.dp)) {
        if (!spark) ChartHeading(node.title, node.hint)
        if (!spark && unit != null && !node.horizontal) Text(unit, style = MaterialTheme.typography.labelMedium, color = scheme.onSurfaceVariant)
        val count = node.labels.size.coerceAtLeast(node.series.maxOfOrNull { it.values.size } ?: 0)
        val height = if (spark) 40.dp else if (node.horizontal) (28f * count.coerceAtLeast(1) * scale.coerceIn(1f, 1.5f)).dp else (184f * scale.coerceIn(1f, 1.4f)).dp
        Canvas(Modifier.fillMaxWidth().height(height).clearAndSetSemantics { contentDescription = said }) {
            if (count == 0) return@Canvas
            // Room for the value axis' numbers or the levels' names at the start, and the labels under.
            val axisWords = if (spark) emptyList() else if (node.levels.isNotEmpty()) node.levels else listOf(amount(bounds.high), amount(bounds.low))
            val gutter = if (spark || node.horizontal) 0f else (axisWords.maxOfOrNull { measurer.measure(it, small).size.width } ?: 0) + 8.dp.toPx()
            val labelsRoom = if (spark || node.horizontal) 0f else measurer.measure("0", small).size.height + 6.dp.toPx()
            val plot = Rect(if (rtl) 0f else gutter, 4.dp.toPx(), if (rtl) size.width - gutter else size.width, size.height - labelsRoom)
            fun x(at: Float): Float = if (rtl) plot.right - at * plot.width else plot.left + at * plot.width
            fun y(value: Double): Float = plot.bottom - bounds.at(value).coerceIn(-0.05f, 1.05f) * plot.height
            if (node.horizontal) {
                horizontalBars(node, inks.map { it.mark }, growth, measurer, small, rtl)
                return@Canvas
            }
            if (!spark) {
                // Bands of the zones, then the grid: the floor, the top, and halfway.
                node.zones.forEachIndexed { index, zone ->
                    val top = y(maxOf(zone.from, zone.to))
                    val bottom = y(minOf(zone.from, zone.to))
                    drawRect(zoneInks[index].track.copy(alpha = 0.6f), Offset(plot.left, top), Size(plot.width, bottom - top))
                }
                val rows = if (node.levels.isNotEmpty()) node.levels.indices.map { it.toDouble() } else listOf(bounds.low, (bounds.low + bounds.high) / 2, bounds.high)
                for (value in rows) drawLine(scheme.outlineVariant, Offset(plot.left, y(value)), Offset(plot.right, y(value)), strokeWidth = 1f)
                // The value axis: the levels' names, or its lowest and highest numbers.
                if (node.levels.isNotEmpty()) {
                    node.levels.forEachIndexed { index, word ->
                        val text = measurer.measure(word, small)
                        val left = if (rtl) plot.right + 8.dp.toPx() else plot.left - 8.dp.toPx() - text.size.width
                        drawText(text, topLeft = Offset(left, y(index.toDouble()) - text.size.height / 2f))
                    }
                } else {
                    for (value in listOf(bounds.low, bounds.high)) {
                        val text = measurer.measure(amount(value), small)
                        val left = if (rtl) plot.right + 8.dp.toPx() else plot.left - 8.dp.toPx() - text.size.width
                        drawText(text, topLeft = Offset(left, (y(value) - text.size.height / 2f).coerceIn(0f, size.height - text.size.height)))
                    }
                }
            }
            val bar = node.kind == Navree.ChartKind.BAR || node.kind == Navree.ChartKind.FUNNEL
            if (bar) {
                val group = plot.width / count
                val stacked = node.stacking != Navree.Stacking.APART
                val each = if (stacked) group * 0.56f else group * 0.7f / node.series.size.coerceAtLeast(1)
                val corner = CornerRadius(4.dp.toPx())
                for (at in 0 until count) {
                    if (stacked) {
                        AxisRules.stack(node, at).forEachIndexed { s, (from, to) ->
                            if (to <= from) return@forEachIndexed
                            val top = y(from + (to - from) * growth)
                            val bottom = y(from)
                            val left = x((at + 0.5f) / count) - each / 2f
                            drawRoundRect(inks[s].mark, Offset(left, top), Size(each, (bottom - top - 1f).coerceAtLeast(0f)), corner)
                        }
                    } else {
                        node.series.forEachIndexed { s, series ->
                            val value = series.values.getOrNull(at) ?: return@forEachIndexed
                            val top = y(bounds.low + (value - bounds.low) * growth)
                            val start = (at + 0.15f) / count + s * (0.7f / count / node.series.size.coerceAtLeast(1))
                            val left = if (rtl) x(start) - each else x(start)
                            drawRoundRect(inks[s].mark, Offset(left, top), Size(each * 0.9f, (y(bounds.low) - top).coerceAtLeast(0f)), corner)
                        }
                    }
                }
            } else {
                node.series.forEachIndexed { s, series ->
                    val points = series.values.mapIndexedNotNull { at, value ->
                        value?.let { Offset(x(if (count > 1) at.toFloat() / (count - 1) else 0.5f), y(bounds.low + (it - bounds.low) * growth)) }
                    }
                    if (points.isEmpty()) return@forEachIndexed
                    val line = linePath(points, node.curve)
                    if (node.kind == Navree.ChartKind.AREA) {
                        val area = Path().apply {
                            addPath(line)
                            lineTo(points.last().x, plot.bottom)
                            lineTo(points.first().x, plot.bottom)
                            close()
                        }
                        drawPath(area, inks[s].mark.copy(alpha = 0.18f))
                    }
                    drawPath(line, inks[s].mark, style = Stroke(width = (if (spark) 2.5f else 3f).dp.toPx(), cap = StrokeCap.Round, join = StrokeJoin.Round))
                    if (spark) drawCircle(inks[s].mark, 3.5.dp.toPx(), points.last())
                }
            }
            if (!spark) {
                node.references.forEachIndexed { index, reference ->
                    val value = reference.value ?: return@forEachIndexed
                    val at = y(value)
                    dashed(referenceInks[index].mark, Offset(plot.left, at), Offset(plot.right, at))
                    reference.label?.let { word ->
                        val text = measurer.measure(word, small.copy(color = referenceInks[index].mark))
                        val left = if (rtl) plot.left + 4.dp.toPx() else plot.right - text.size.width - 4.dp.toPx()
                        drawText(text, topLeft = Offset(left, at - text.size.height - 2.dp.toPx()))
                    }
                }
                // The labels under: every one when there are few, else the first, middle and last.
                val shown = if (count <= 8) node.labels.indices.toList() else listOf(0, node.labels.size / 2, node.labels.lastIndex).distinct()
                for (at in shown) {
                    val word = node.labels.getOrNull(at) ?: continue
                    val text = measurer.measure(word, small)
                    val centre = if (bar) x((at + 0.5f) / count) else x(if (count > 1) at.toFloat() / (count - 1) else 0.5f)
                    val left = (centre - text.size.width / 2f).coerceIn(plot.left - (if (rtl) 0f else gutter), size.width - text.size.width)
                    drawText(text, topLeft = Offset(left, plot.bottom + 6.dp.toPx()))
                }
            }
        }
        if (!spark && node.series.size > 1) Key(node.series.mapIndexed { index, series -> inks[index].mark to series.label })
    }
}

/** A line through its points: straight, smooth (a curve that does not overshoot its points), or in steps. */
private fun linePath(points: List<Offset>, curve: Navree.Curve): Path = Path().apply {
    moveTo(points[0].x, points[0].y)
    for (index in 1 until points.size) {
        val from = points[index - 1]
        val to = points[index]
        when (curve) {
            Navree.Curve.STEP -> {
                lineTo(to.x, from.y)
                lineTo(to.x, to.y)
            }
            Navree.Curve.SMOOTH -> {
                val mid = (from.x + to.x) / 2f
                cubicTo(mid, from.y, mid, to.y, to.x, to.y)
            }
            Navree.Curve.LINEAR -> lineTo(to.x, to.y)
        }
    }
}

/** Bars that lie along the line: a row a label, its name at the start, the parts stacked as the page says. */
private fun DrawScope.horizontalBars(
    node: Navree.Chart,
    colours: List<Color>,
    growth: Float,
    measurer: androidx.compose.ui.text.TextMeasurer,
    style: TextStyle,
    rtl: Boolean,
) {
    val count = node.labels.size.coerceAtLeast(1)
    val names = node.labels.map { measurer.measure(it, style) }
    val gutter = (names.maxOfOrNull { it.size.width } ?: 0) + 8.dp.toPx()
    val width = size.width - gutter
    val row = size.height / count
    val most = if (node.stacking == Navree.Stacking.SHARES) 1.0 else (0 until count).maxOf { at -> AxisRules.stack(node, at).lastOrNull()?.second ?: 0.0 }.coerceAtLeast(1e-9)
    for (at in 0 until count) {
        val name = names.getOrNull(at)
        val top = row * at
        if (name != null) {
            val left = if (rtl) size.width - name.size.width else 0f
            drawText(name, topLeft = Offset(left, top + (row - name.size.height) / 2f))
        }
        AxisRules.stack(node, at).forEachIndexed { s, (from, to) ->
            if (to <= from) return@forEachIndexed
            val start = (from / most * width * growth).toFloat()
            val end = (to / most * width * growth).toFloat()
            val left = if (rtl) width - end else gutter + start
            drawRoundRect(colours[s], Offset(left, top + row * 0.2f), Size((end - start - 1f).coerceAtLeast(0f), row * 0.6f), CornerRadius(3.dp.toPx()))
        }
    }
}

/**
 * The calendar of days: a week a column, the person's first weekday at the
 * top, the month's name over the week it starts in, each day a square in one
 * of four shades of its hue, a day with no data left pale.
 */
@Composable
internal fun CalendarHeatmapView(node: Navree.CalendarHeatmap, modifier: Modifier = Modifier) {
    val scheme = MaterialTheme.colorScheme
    val dark = scheme.surface.luminance() < 0.5f
    val hue = node.colour?.hue ?: Navree.Hue.GREEN
    val shades = shadeColours(hue, dark)
    val empty = scheme.surfaceContainerHighest
    val days = node.days.mapNotNull { day -> runCatching { LocalDate.parse(day.date) }.getOrNull()?.let { it to day.value } }
    val first = node.first?.let { runCatching { LocalDate.parse(it) }.getOrNull() } ?: days.minOfOrNull { it.first }
    val last = node.last?.let { runCatching { LocalDate.parse(it) }.getOrNull() } ?: days.maxOfOrNull { it.first }
    val locale = Locale.getDefault()
    val weekStart = WeekFields.of(locale).firstDayOfWeek
    val highest = days.mapNotNull { it.second }.maxOrNull() ?: 0.0
    val said = node.summary ?: node.title ?: ""
    Column(modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(8.dp)) {
        ChartHeading(node.title, node.hint)
        if (first == null || last == null) return@Column
        val start = HeatRules.start(first, weekStart)
        val weeks = HeatRules.weeks(first, last, weekStart)
        val values = days.toMap()
        BoxWithConstraints(Modifier.fillMaxWidth().clearAndSetSemantics { contentDescription = said }) {
            val gap = 3.dp
            val side = ((maxWidth - 28.dp - gap * (weeks - 1)) / weeks).coerceIn(8.dp, 22.dp)
            Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
                // The months over the weeks they start in.
                Row(Modifier.padding(start = 28.dp)) {
                    var shownMonth = -1
                    for (week in 0 until weeks) {
                        val day = start.plusDays(week * 7L).let { if (it.isBefore(first)) first else it }
                        val month = day.monthValue
                        Box(Modifier.width(side + gap)) {
                            if (month != shownMonth && day.dayOfMonth <= 7 || week == 0) {
                                Text(day.month.getDisplayName(DateWords.SHORT, locale), style = MaterialTheme.typography.labelSmall, color = scheme.onSurfaceVariant, maxLines = 1, softWrap = false)
                            }
                        }
                        shownMonth = month
                    }
                }
                Row {
                    Column(Modifier.width(28.dp), verticalArrangement = Arrangement.spacedBy(gap)) {
                        for (row in 0 until 7) {
                            Box(Modifier.height(side), contentAlignment = Alignment.CenterStart) {
                                if (row % 2 == 0) Text(weekStart.plus(row.toLong()).getDisplayName(DateWords.NARROW, locale), style = MaterialTheme.typography.labelSmall, color = scheme.onSurfaceVariant)
                            }
                        }
                    }
                    Row(horizontalArrangement = Arrangement.spacedBy(gap)) {
                        for (week in 0 until weeks) {
                            Column(verticalArrangement = Arrangement.spacedBy(gap)) {
                                for (row in 0 until 7) {
                                    val day = start.plusDays(week * 7L + row)
                                    val inside = !day.isBefore(first) && !day.isAfter(last)
                                    val shade = HeatRules.shade(values[day], node.steps, highest)
                                    val colour = when {
                                        !inside -> Color.Transparent
                                        shade == null -> empty
                                        else -> shades[shade]
                                    }
                                    Box(Modifier.size(side).clip(RoundedCornerShape(3.dp)).background(colour))
                                }
                            }
                        }
                    }
                }
            }
        }
        // The shades from least to most, with the page's own name of the measure.
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(3.dp)) {
            (node.label ?: node.unit)?.let { Text(it, style = MaterialTheme.typography.labelSmall, color = scheme.onSurfaceVariant, modifier = Modifier.padding(end = 6.dp)) }
            Box(Modifier.size(12.dp).clip(RoundedCornerShape(3.dp)).background(empty))
            for (colour in shades) Box(Modifier.size(12.dp).clip(RoundedCornerShape(3.dp)).background(colour))
        }
    }
}

/** The four shades of a heat map in a hue, faint to strong, in the theme. */
@Composable
private fun shadeColours(hue: Navree.Hue, dark: Boolean): List<Color> {
    val scheme = MaterialTheme.colorScheme
    val primary = scheme.primary
    return remember(hue, dark, primary) {
        val (primaryHue, primaryChroma) = Hct.hueChroma(primary.toArgb())
        // A hue Material has a role for (the accent) is shaded from the theme's own primary.
        val palette = Hues.palette(hue, primaryHue) ?: TonalPalette(primaryHue, primaryChroma.coerceAtLeast(36.0))
        Hues.shadeTones(dark).map { Color(palette.tone(it)) }
    }
}

/**
 * Every measure against every other: under the diagonal when the page says
 * the pair is the same either way round. The sign is the hue, the depth the
 * strength; a pair that is chance is neutral, one never tested is left pale.
 */
@Composable
internal fun MatrixHeatmapView(node: Navree.MatrixHeatmap, modifier: Modifier = Modifier) {
    val scheme = MaterialTheme.colorScheme
    val positive = inkOf(node.positive, Navree.Hue.BLUE)
    val negative = inkOf(node.negative, Navree.Hue.ORANGE)
    val pale = scheme.surfaceContainerHigh
    val n = node.measures.size
    val said = node.summary ?: node.title ?: ""
    Column(modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(8.dp)) {
        ChartHeading(node.title, node.hint)
        if (n < 2) return@Column
        val rows = if (node.lower) (1 until n) else (0 until n)
        val columns = if (node.lower) (0 until n - 1) else (0 until n)
        BoxWithConstraints(Modifier.fillMaxWidth().clearAndSetSemantics { contentDescription = said }) {
            val names = 96.dp
            val side = ((maxWidth - names) / columns.count()).coerceIn(28.dp, 72.dp)
            Column(verticalArrangement = Arrangement.spacedBy(2.dp)) {
                for (row in rows) {
                    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(2.dp)) {
                        Text(node.measures[row], style = MaterialTheme.typography.labelSmall, color = scheme.onSurfaceVariant, maxLines = 2, overflow = TextOverflow.Ellipsis, modifier = Modifier.width(names))
                        for (column in columns) {
                            if (!MatrixRules.drawn(row, column, node.lower)) {
                                Box(Modifier.size(side))
                                continue
                            }
                            val relation = MatrixRules.relation(node.relations, row, column)
                            val strength = MatrixRules.strength(relation, node.peak)
                            val value = relation?.value
                            val ink = if ((value ?: 0.0) < 0) negative else positive
                            val colour = when {
                                value == null -> pale.copy(alpha = 0.5f)
                                relation?.significant == false -> scheme.surfaceContainerHighest
                                else -> lerp(ink.track, ink.mark, strength)
                            }
                            Box(Modifier.size(side).clip(RoundedCornerShape(4.dp)).background(colour), contentAlignment = Alignment.Center) {
                                if (value != null && side >= 36.dp) {
                                    Text(
                                        String.format(Locale.getDefault(), "%.2f", value),
                                        style = MaterialTheme.typography.labelSmall,
                                        color = if (strength > 0.55f) onMark(colour) else scheme.onSurface,
                                        maxLines = 1,
                                    )
                                }
                            }
                        }
                    }
                }
                Row(horizontalArrangement = Arrangement.spacedBy(2.dp)) {
                    Box(Modifier.width(names))
                    for (column in columns) {
                        Text(node.measures[column], style = MaterialTheme.typography.labelSmall, color = scheme.onSurfaceVariant, maxLines = 2, overflow = TextOverflow.Ellipsis, modifier = Modifier.width(side))
                    }
                }
            }
        }
        node.label?.let { Text(it, style = MaterialTheme.typography.labelSmall, color = scheme.onSurfaceVariant) }
    }
}

/**
 * A scatter plot: clouds of translucent points in their series' hues on two
 * numeric axes named with their units; with equal axes the plot is square
 * and a diagonal reference is the line of no change.
 */
@Composable
internal fun ScatterView(node: Navree.Scatter, modifier: Modifier = Modifier) {
    val scheme = MaterialTheme.colorScheme
    val inks = node.series.mapIndexed { index, series -> inkOf(series.colour, SLOTS[index % SLOTS.size]) }
    val referenceInks = node.references.map { inkOf(it.colour, Navree.Hue.NEUTRAL) }
    val (xs, ys) = remember(node) { AxisRules.scatter(node) }
    val growth = grown(1f)
    val measurer = rememberTextMeasurer()
    val small = MaterialTheme.typography.labelSmall.copy(color = scheme.onSurfaceVariant)
    val rtl = LocalLayoutDirection.current == LayoutDirection.Rtl
    val said = node.summary ?: node.title ?: ""
    Column(modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(8.dp)) {
        ChartHeading(node.title, node.hint)
        node.y?.let { measure -> Text(listOfNotNull(measure.name, measure.unit).joinToString(", "), style = MaterialTheme.typography.labelMedium, color = scheme.onSurfaceVariant) }
        val shape = if (node.equalAxes) Modifier.fillMaxWidth(0.86f).aspectRatio(1f) else Modifier.fillMaxWidth().aspectRatio(1.5f)
        Box(Modifier.fillMaxWidth(), contentAlignment = Alignment.Center) {
            Canvas(shape.clearAndSetSemantics { contentDescription = said }) {
                val words = listOf(amount(ys.low), amount(ys.high))
                val gutter = (words.maxOf { measurer.measure(it, small).size.width }) + 6.dp.toPx()
                val under = measurer.measure("0", small).size.height + 4.dp.toPx()
                val plot = Rect(if (rtl) 0f else gutter, 0f, if (rtl) size.width - gutter else size.width, size.height - under)
                fun px(value: Double) = if (rtl) plot.right - xs.at(value) * plot.width else plot.left + xs.at(value) * plot.width
                fun py(value: Double) = plot.bottom - ys.at(value) * plot.height
                drawRect(scheme.outlineVariant, plot.topLeft, plot.size, style = Stroke(1f))
                node.references.forEachIndexed { index, reference ->
                    when (reference.kind) {
                        Navree.ReferenceKind.DIAGONAL -> {
                            val low = maxOf(xs.low, ys.low)
                            val high = minOf(xs.high, ys.high)
                            dashed(referenceInks[index].mark, Offset(px(low), py(low)), Offset(px(high), py(high)))
                        }
                        Navree.ReferenceKind.LEVEL -> reference.value?.let { value -> dashed(referenceInks[index].mark, Offset(plot.left, py(value)), Offset(plot.right, py(value))) }
                    }
                }
                node.series.forEachIndexed { s, series ->
                    for (point in series.points) {
                        drawCircle(inks[s].mark.copy(alpha = 0.55f), 4.5.dp.toPx() * growth, Offset(px(point.x), py(point.y)))
                    }
                }
                for ((value, word) in listOf(ys.low to words[0], ys.high to words[1])) {
                    val text = measurer.measure(word, small)
                    val left = if (rtl) plot.right + 6.dp.toPx() else plot.left - 6.dp.toPx() - text.size.width
                    drawText(text, topLeft = Offset(left, (py(value) - text.size.height / 2f).coerceIn(0f, size.height - text.size.height)))
                }
                for (value in listOf(xs.low, xs.high)) {
                    val text = measurer.measure(amount(value), small)
                    val left = (px(value) - text.size.width / 2f).coerceIn(0f, size.width - text.size.width)
                    drawText(text, topLeft = Offset(left, plot.bottom + 4.dp.toPx()))
                }
            }
        }
        node.x?.let { measure -> Text(listOfNotNull(measure.name, measure.unit).joinToString(", "), style = MaterialTheme.typography.labelMedium, color = scheme.onSurfaceVariant, modifier = Modifier.align(Alignment.End)) }
        Key(node.series.mapIndexed { index, series -> inks[index].mark to series.name } + node.references.mapIndexedNotNull { index, reference -> reference.label?.let { referenceInks[index].mark to it } })
    }
}
