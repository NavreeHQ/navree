package com.navree.client.ui

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.navree.protocol.Navree

/**
 * A chart drawn with the theme's own colours: primary for the first series,
 * then tertiary and secondary, never arbitrary hues. Bars start at zero, a
 * line runs through its points, a donut shows the parts of one whole. Every
 * series is named in a legend under the chart, and TalkBack reads the numbers
 * as the server wrote them, so nothing rests on colour alone.
 */
@Composable
internal fun ChartView(node: Navree.Chart, modifier: Modifier = Modifier) {
    val scheme = MaterialTheme.colorScheme
    val colours = listOf(scheme.primary, scheme.tertiary, scheme.secondary, scheme.error, scheme.outline)
    val grid = scheme.outlineVariant
    val said = node.series.joinToString("; ") { series ->
        series.label + ": " + node.labels.indices.joinToString(", ") { at ->
            val shown = series.shown.getOrNull(at) ?: series.values.getOrNull(at)?.toString() ?: "—"
            (node.labels.getOrNull(at)?.let { "$it $shown" } ?: shown)
        }
    }
    Column(
        modifier.fillMaxWidth().semantics { contentDescription = listOfNotNull(node.title, said).joinToString(". ") },
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        node.title?.let { Text(it, style = MaterialTheme.typography.titleMedium, modifier = Modifier.semantics { heading() }) }
        val height = if (node.kind == Navree.ChartKind.SPARKLINE) 48.dp else 180.dp
        Box(Modifier.fillMaxWidth().height(height).clearAndSetSemantics {}) {
            Canvas(Modifier.fillMaxWidth().height(height)) {
                val values = node.series.map { series -> series.values.map { it ?: 0.0 } }
                val most = (values.flatten().maxOrNull() ?: 0.0).coerceAtLeast(1e-9)
                val count = node.labels.size.coerceAtLeast(values.maxOfOrNull { it.size } ?: 0)
                if (count == 0) return@Canvas
                // Three light grid lines, the floor the bars stand on among them.
                if (node.kind != Navree.ChartKind.SPARKLINE && node.kind != Navree.ChartKind.DONUT) {
                    for (step in 0..2) {
                        val y = size.height * step / 2f
                        drawLine(grid, Offset(0f, y), Offset(size.width, y), strokeWidth = 1f)
                    }
                }
                when (node.kind) {
                    Navree.ChartKind.BAR, Navree.ChartKind.FUNNEL -> {
                        val group = size.width / count
                        val each = (group * 0.7f) / values.size.coerceAtLeast(1)
                        values.forEachIndexed { s, series ->
                            series.forEachIndexed { at, value ->
                                val h = (value / most).toFloat() * size.height
                                val x = group * at + group * 0.15f + each * s
                                drawRoundRect(colours[s % colours.size], Offset(x, size.height - h), Size(each * 0.9f, h), CornerRadius(4.dp.toPx()))
                            }
                        }
                    }
                    Navree.ChartKind.DONUT -> {
                        val parts = values.firstOrNull().orEmpty()
                        val whole = parts.sum().coerceAtLeast(1e-9)
                        val side = minOf(size.width, size.height)
                        val stroke = side * 0.18f
                        val topLeft = Offset((size.width - side) / 2 + stroke / 2, stroke / 2)
                        var start = -90f
                        parts.forEachIndexed { at, part ->
                            val sweep = (part / whole * 360).toFloat()
                            drawArc(colours[at % colours.size], start, (sweep - 2f).coerceAtLeast(0f), false, topLeft, Size(side - stroke, side - stroke), style = Stroke(stroke))
                            start += sweep
                        }
                    }
                    else -> values.forEachIndexed { s, series ->
                        if (series.isEmpty()) return@forEachIndexed
                        val stepX = if (count > 1) size.width / (count - 1) else size.width
                        val path = Path()
                        series.forEachIndexed { at, value ->
                            val x = stepX * at
                            val y = size.height - (value / most).toFloat() * size.height
                            if (at == 0) path.moveTo(x, y) else path.lineTo(x, y)
                        }
                        val colour = colours[s % colours.size]
                        if (node.kind == Navree.ChartKind.AREA) {
                            val area = Path().apply {
                                addPath(path)
                                lineTo(stepX * (series.size - 1), size.height)
                                lineTo(0f, size.height)
                                close()
                            }
                            drawPath(area, colour.copy(alpha = 0.16f))
                        }
                        drawPath(path, colour, style = Stroke(width = 3.dp.toPx(), cap = StrokeCap.Round, join = StrokeJoin.Round))
                    }
                }
            }
        }
        if (node.kind != Navree.ChartKind.SPARKLINE && node.labels.isNotEmpty() && node.kind != Navree.ChartKind.DONUT) {
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
                Text(node.labels.first(), style = MaterialTheme.typography.labelSmall, color = scheme.onSurfaceVariant)
                if (node.labels.size > 1) Text(node.labels.last(), style = MaterialTheme.typography.labelSmall, color = scheme.onSurfaceVariant)
            }
        }
        val named = if (node.kind == Navree.ChartKind.DONUT) node.labels else node.series.map { it.label }
        if (node.kind != Navree.ChartKind.SPARKLINE && named.size > 1) {
            Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
                named.forEachIndexed { at, name ->
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Box(Modifier.size(10.dp).clip(CircleShape).background(colours[at % colours.size]))
                        Text(name, style = MaterialTheme.typography.bodySmall, maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.padding(start = 8.dp))
                    }
                }
            }
        }
    }
}
