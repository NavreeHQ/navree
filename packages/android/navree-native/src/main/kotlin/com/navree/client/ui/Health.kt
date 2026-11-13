@file:OptIn(ExperimentalMaterial3Api::class, ExperimentalLayoutApi::class)

package com.navree.client.ui

import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.CubicBezierEasing
import androidx.compose.animation.core.tween
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.DatePicker
import androidx.compose.material3.DatePickerDialog
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.RichTooltip
import androidx.compose.material3.SelectableDates
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TooltipAnchorPosition
import androidx.compose.material3.TooltipBox
import androidx.compose.material3.TooltipDefaults
import androidx.compose.material3.rememberDatePickerState
import androidx.compose.material3.rememberTooltipState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.scale
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.drawscope.rotate
import androidx.compose.ui.graphics.luminance
import androidx.compose.ui.graphics.toArgb
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import com.navree.client.R
import com.navree.client.core.Events
import com.navree.client.design.ClockRules
import com.navree.client.design.PagerRules
import com.navree.client.design.RingRules
import com.navree.client.design.RingsRules
import com.navree.client.design.ShareRules
import com.navree.client.design.ZoneRules
import com.navree.client.theme.Hct
import com.navree.client.theme.Hues
import com.navree.client.theme.LocalReducedMotion
import com.navree.protocol.Navree
import kotlinx.coroutines.launch
import java.text.NumberFormat
import java.time.Instant
import java.time.LocalDate
import java.time.LocalTime
import java.time.ZoneOffset
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle
import kotlin.math.cos
import kotlin.math.floor
import kotlin.math.sin

/*
 * The health charts, each drawn the way Material draws data: marks in the
 * hues of the theme's own tonal palettes, tracks in the same hue's lightest
 * tone, words in the type scale. Every chart is one element to TalkBack that
 * says the page's own summary, and every size grows with the person's text.
 */

/** A hue in the theme: its mark, the track a mark runs along, a container and what sits on it. */
@Immutable
internal data class Ink(val mark: Color, val track: Color, val container: Color, val onContainer: Color)

/** The hues a series or a part takes, in order, when the page names none: the theme's own first. */
internal val SLOTS = listOf(
    Navree.Hue.ACCENT,
    Navree.Hue.ORANGE,
    Navree.Hue.GREEN,
    Navree.Hue.AMBER,
    Navree.Hue.ROSE,
    Navree.Hue.VIOLET,
    Navree.Hue.CYAN,
    Navree.Hue.RED,
)

@Composable
internal fun ink(hue: Navree.Hue): Ink {
    val scheme = MaterialTheme.colorScheme
    val dark = scheme.surface.luminance() < 0.5f
    val primary = scheme.primary
    return remember(hue, dark, primary, scheme.error, scheme.outline) {
        when (Hues.source(hue)) {
            Hues.Source.Primary -> Ink(scheme.primary, scheme.primaryContainer, scheme.primaryContainer, scheme.onPrimaryContainer)
            Hues.Source.Error -> Ink(scheme.error, scheme.errorContainer, scheme.errorContainer, scheme.onErrorContainer)
            Hues.Source.Outline -> Ink(scheme.outline, scheme.surfaceContainerHighest, scheme.surfaceContainerHighest, scheme.onSurfaceVariant)
            Hues.Source.Palette -> {
                val palette = Hues.palette(hue, Hct.hueChroma(primary.toArgb()).first)!!
                Ink(
                    mark = Color(palette.tone(Hues.markTone(dark))),
                    track = Color(palette.tone(Hues.trackTone(dark))),
                    container = Color(palette.tone(Hues.containerTone(dark))),
                    onContainer = Color(palette.tone(Hues.onContainerTone(dark))),
                )
            }
        }
    }
}

/** The ink of a page's colour, or of the slot's hue when it names none. */
@Composable
internal fun inkOf(colour: Navree.Colour?, fallback: Navree.Hue): Ink = ink(colour?.hue ?: fallback)

/** The person's text size, which every chart grows with. */
@Composable
internal fun textScale(): Float = LocalDensity.current.fontScale

private val Emphasized = CubicBezierEasing(0.05f, 0.7f, 0.1f, 1f)

/**
 * A value that grows to where it stands when it first shows and moves when
 * it changes, with Material's emphasised easing; at once when the person
 * turned animations off.
 */
@Composable
internal fun grown(target: Float): Float {
    if (LocalReducedMotion.current) return target
    val value = remember { Animatable(0f) }
    LaunchedEffect(target) { value.animateTo(target, tween(durationMillis = 700, easing = Emphasized)) }
    return value.value
}

/** A number as the person's locale writes it, with no decimals it does not need. */
internal fun number(value: Double): String {
    val format = NumberFormat.getNumberInstance()
    format.maximumFractionDigits = if (value % 1.0 == 0.0) 0 else 1
    return format.format(value)
}

/**
 * The mark beside a heading that tells more on request: Material's rich
 * tooltip, opened by a tap, with the page's own words.
 */
@Composable
internal fun HintMark(hint: Navree.Hint, modifier: Modifier = Modifier) {
    val state = rememberTooltipState(isPersistent = true)
    val scope = rememberCoroutineScope()
    TooltipBox(
        positionProvider = TooltipDefaults.rememberTooltipPositionProvider(TooltipAnchorPosition.Above),
        tooltip = {
            RichTooltip(title = hint.title?.let { title -> { Text(title) } }) { Text(hint.text) }
        },
        state = state,
        modifier = modifier,
    ) {
        IconButton(onClick = { scope.launch { state.show() } }) {
            Icon(
                if (hint.mark == Navree.HintMark.INFO) Glyphs.Info else Glyphs.Help,
                contentDescription = stringResource(R.string.navree_about_hint, hint.about),
                tint = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.size(20.dp),
            )
        }
    }
}

/** A chart's heading: its title in the type scale's title, and its hint beside it. */
@Composable
internal fun ChartHeading(title: String?, hint: Navree.Hint?) {
    if (title == null && hint == null) return
    Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
        Text(
            title ?: "",
            style = MaterialTheme.typography.titleSmall,
            modifier = Modifier.weight(1f).semantics { heading() },
        )
        hint?.let { HintMark(it) }
    }
}

/** A key: a dot of each colour and what it stands for. */
@Composable
internal fun Key(items: List<Pair<Color, String>>, modifier: Modifier = Modifier) {
    if (items.isEmpty()) return
    FlowRow(modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(16.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
        for ((colour, name) in items) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Box(Modifier.size(10.dp).clip(CircleShape).background(colour))
                Text(name, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.padding(start = 6.dp))
            }
        }
    }
}

/** An arc of a ring: its track all round, then the value from the top, a second lap past the goal darker. */
private fun DrawScope.ringArc(progress: Float, ink: Ink, to: Color?, strokePx: Float, diameterPx: Float) {
    val arc = Size(diameterPx, diameterPx)
    val topLeft = Offset((size.width - diameterPx) / 2f, (size.height - diameterPx) / 2f)
    drawArc(ink.track, 0f, 360f, false, topLeft, arc, style = Stroke(strokePx))
    if (progress <= 0f) return
    val first = progress.coerceAtMost(1f) * 360f
    rotate(-90f) {
        val brush = if (to != null) {
            Brush.sweepGradient(0f to ink.mark, (first / 360f).coerceAtLeast(0.01f) to to, 1f to to, center = center)
        } else {
            SolidColor(ink.mark)
        }
        drawArc(brush, 0f, first, false, topLeft, arc, style = Stroke(strokePx, cap = StrokeCap.Round))
        if (progress > 1f) {
            val part = progress - floor(progress)
            val again = (if (part == 0f) 1f else part) * 360f
            drawArc(ink.onContainer.copy(alpha = 0.35f), 0f, again, false, topLeft, arc, style = Stroke(strokePx, cap = StrokeCap.Round))
        }
    }
}

/**
 * A ring: one value against its goal, as a circular arc in its hue on a track
 * of the same hue, the value and its unit in the middle, its label under it.
 * A value past the goal goes round again; no value is an empty ring and a dash.
 */
@Composable
internal fun RingView(node: Navree.Ring, modifier: Modifier = Modifier) {
    val scale = textScale()
    val diameter = RingRules.diameterDp(node.prominence, scale).dp
    val stroke = RingRules.strokeDp(node.prominence).dp
    val ink = inkOf(node.colour, Navree.Hue.ACCENT)
    val to = node.colour?.to?.let { ink(it).mark }
    val sweep = RingRules.sweep(node.value, node.max)
    val progress = grown(sweep?.let { it.laps + it.part } ?: 0f)
    val said = node.summary ?: listOfNotNull(node.label, node.shown?.let { listOfNotNull(it, node.unit).joinToString(" ") }).joinToString(": ")
    val high = node.prominence == Navree.Prominence.HIGH
    Column(modifier, horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Box(Modifier.size(diameter).clearAndSetSemantics { contentDescription = said }, contentAlignment = Alignment.Center) {
            Canvas(Modifier.fillMaxSize()) {
                val px = stroke.toPx()
                ringArc(progress, ink, to, px, size.minDimension - px)
            }
            if (RingRules.showsCentre(node.prominence, node.centre)) {
                when (node.centre) {
                    Navree.Centre.ICON -> node.icon?.let { IconView(it, Modifier.size(diameter * 0.32f), tint = if (node.value == null) MaterialTheme.colorScheme.onSurfaceVariant else ink.mark) }
                    else -> Column(horizontalAlignment = Alignment.CenterHorizontally) {
                        Text(
                            if (node.value == null) "–" else node.shown ?: number(node.value!!),
                            style = if (high) MaterialTheme.typography.displayMedium else MaterialTheme.typography.titleLarge,
                            maxLines = 1,
                        )
                        node.unit?.let { Text(it, style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 1) }
                    }
                }
            }
        }
        if (node.label != null || node.hint != null) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                node.label?.let { Text(it, style = if (high) MaterialTheme.typography.titleMedium else MaterialTheme.typography.labelLarge, textAlign = TextAlign.Center) }
                node.hint?.let { HintMark(it) }
            }
        }
        node.caption?.let { Text(it, style = MaterialTheme.typography.labelLarge, color = ink.mark) }
    }
}

/**
 * Concentric rings, the first outside, as Android's activity rings are drawn:
 * each in its hue on its own track, the values over their goals in the middle
 * when the page asks, and a key that names each ring with its value and share.
 */
@Composable
internal fun RingsView(node: Navree.Rings, modifier: Modifier = Modifier) {
    val rings = node.rings.take(RingsRules.MOST)
    val scale = textScale()
    val diameter = RingsRules.diameterDp(node.prominence, rings.size, scale)
    val stroke = RingsRules.strokeDp(diameter, rings.size)
    val inks = rings.mapIndexed { index, ring -> inkOf(ring.colour, SLOTS[(index + 1) % SLOTS.size]) }
    val progress = rings.map { ring -> RingRules.sweep(ring.value, ring.max)?.let { it.laps + it.part } ?: 0f }
    val grownAll = progress.map { grown(it) }
    val said = node.summary ?: rings.joinToString(". ") { "${it.name} ${it.shown ?: it.value?.let(::number) ?: "–"}" }
    val low = node.prominence == Navree.Prominence.LOW
    Column(modifier, verticalArrangement = Arrangement.spacedBy(12.dp)) {
        if (!low) ChartHeading(node.title, node.hint)
        val dial: @Composable () -> Unit = {
            Box(Modifier.size(diameter.dp).clearAndSetSemantics { contentDescription = said }, contentAlignment = Alignment.Center) {
                Canvas(Modifier.fillMaxSize()) {
                    val px = stroke.dp.toPx()
                    rings.indices.forEach { index ->
                        ringArc(grownAll[index], inks[index], null, px, RingsRules.ringDiameterDp(diameter, index, stroke).dp.toPx())
                    }
                }
                if (!low) {
                    when (node.centre) {
                        Navree.RingsCentre.VALUES -> Column(horizontalAlignment = Alignment.CenterHorizontally) {
                            rings.forEachIndexed { index, ring ->
                                Text(
                                    "${ring.shown ?: ring.value?.let(::number) ?: "–"}/${number(ring.max)}",
                                    style = MaterialTheme.typography.labelLarge,
                                    color = inks[index].mark,
                                    maxLines = 1,
                                )
                            }
                        }
                        Navree.RingsCentre.ICON -> node.icon?.let { IconView(it, Modifier.size((diameter * 0.2f).dp)) }
                        Navree.RingsCentre.NOTHING -> Unit
                    }
                }
            }
        }
        if (node.legend && !low) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(24.dp)) {
                dial()
                Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(12.dp)) {
                    rings.forEachIndexed { index, ring ->
                        Column {
                            Text(ring.name, style = MaterialTheme.typography.labelLarge, color = inks[index].mark)
                            Text(
                                listOfNotNull("${ring.shown ?: ring.value?.let(::number) ?: "–"}/${number(ring.max)}", ring.unit).joinToString(" "),
                                style = MaterialTheme.typography.titleMedium,
                            )
                            ring.value?.let { value ->
                                Text(
                                    "${ShareRules.percent((value / ring.max).toFloat())}%",
                                    style = MaterialTheme.typography.bodySmall,
                                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                                )
                            }
                        }
                    }
                }
            }
        } else {
            dial()
        }
    }
}

/**
 * A gauge: a value on an open arc of 240 degrees, its zones laid along the
 * track in their hues, the arc up to the value in the hue of the zone it is
 * in, the value and the zone's name in the middle.
 */
@Composable
internal fun HealthGaugeView(node: Navree.Gauge, modifier: Modifier = Modifier) {
    val scale = textScale()
    val zone = ZoneRules.zoneOf(node.value, node.zones)
    val fallback = when (node.tone) {
        Navree.Tone.SUCCESS -> Navree.Hue.SUCCESS
        Navree.Tone.WARNING -> Navree.Hue.WARNING
        Navree.Tone.DANGER -> Navree.Hue.DANGER
        Navree.Tone.INFO -> Navree.Hue.INFO
        else -> Navree.Hue.ACCENT
    }
    val ink = inkOf(node.colour ?: zone?.colour, fallback)
    val zoneInks = node.zones.map { inkOf(it.colour, Navree.Hue.NEUTRAL) }
    val at = grown(ZoneRules.position(node.value, node.min, node.max))
    val said = node.summary ?: listOfNotNull(node.label, node.shown ?: number(node.value), zone?.label ?: node.caption).joinToString(", ")
    val diameter = (176f * scale.coerceIn(1f, 1.5f)).dp
    val halo = MaterialTheme.colorScheme.surface
    Column(modifier.fillMaxWidth(), horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(8.dp)) {
        if (node.hint != null) ChartHeading(node.label, node.hint)
        Box(Modifier.size(diameter).clearAndSetSemantics { contentDescription = said }, contentAlignment = Alignment.Center) {
            Canvas(Modifier.fillMaxSize()) {
                val px = 14.dp.toPx()
                val d = size.minDimension - px
                val arc = Size(d, d)
                val topLeft = Offset((size.width - d) / 2f, (size.height - d) / 2f)
                val start = 150f
                val sweep = 240f
                if (node.zones.isEmpty()) {
                    drawArc(ink.track, start, sweep, false, topLeft, arc, style = Stroke(px, cap = StrokeCap.Round))
                } else {
                    node.zones.forEachIndexed { index, z ->
                        val from = ZoneRules.position(z.from, node.min, node.max)
                        val to = ZoneRules.position(z.to, node.min, node.max)
                        // A hair between zones, so each reads as its own.
                        drawArc(zoneInks[index].track, start + from * sweep + 1f, ((to - from) * sweep - 2f).coerceAtLeast(0.5f), false, topLeft, arc, style = Stroke(px))
                    }
                }
                drawArc(ink.mark, start, (at * sweep).coerceAtLeast(0.5f), false, topLeft, arc, style = Stroke(px, cap = StrokeCap.Round))
                // The marker: a dot on the arc where the value is.
                val angle = Math.toRadians((start + at * sweep).toDouble())
                val r = d / 2f
                val c = Offset(size.width / 2f + (r * cos(angle)).toFloat(), size.height / 2f + (r * sin(angle)).toFloat())
                drawCircle(halo, px * 0.42f, c)
                drawCircle(ink.mark, px * 0.28f, c)
            }
            Column(horizontalAlignment = Alignment.CenterHorizontally) {
                Text(node.shown ?: number(node.value), style = MaterialTheme.typography.headlineMedium, maxLines = 1, textAlign = TextAlign.Center)
                (zone?.label ?: node.caption)?.let { Text(it, style = MaterialTheme.typography.labelLarge, color = ink.mark) }
            }
        }
        if (node.hint == null) node.label?.let { Text(it, style = MaterialTheme.typography.labelLarge, color = MaterialTheme.colorScheme.onSurfaceVariant, textAlign = TextAlign.Center) }
        if (node.zones.isNotEmpty()) {
            Key(node.zones.mapIndexedNotNull { index, z -> z.label?.let { zoneInks[index].mark to it } })
        }
    }
}

/**
 * The day clock: 24 hours round a dial, midnight at the top, each hour a
 * bar outwards as long as how much happened in it, in its category's hue;
 * the hour that is now outlined, the moments of the day marked outside, the
 * day's total in the middle.
 */
@Composable
internal fun DayClockView(node: Navree.DayClock, modifier: Modifier = Modifier) {
    val scale = textScale()
    val most = ClockRules.most(node)
    val base = inkOf(node.colour, Navree.Hue.ACCENT)
    val keyInks = node.key.mapIndexed { index, category -> inkOf(category.colour, SLOTS[index % SLOTS.size]) }
    val momentInks = node.moments.map { inkOf(it.colour, Navree.Hue.NEUTRAL) }
    val quiet = MaterialTheme.colorScheme.outlineVariant
    val ring = MaterialTheme.colorScheme.surfaceContainerHighest
    val outline = MaterialTheme.colorScheme.onSurface
    val growth = grown(1f)
    val said = node.summary ?: listOfNotNull(node.title, node.shown?.let { listOfNotNull(it, node.unit).joinToString(" ") }).joinToString(": ")
    val times = remember { DateTimeFormatter.ofLocalizedTime(FormatStyle.SHORT) }
    Column(modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(12.dp), horizontalAlignment = Alignment.CenterHorizontally) {
        ChartHeading(node.title, node.hint)
        Box(Modifier.size((232f * scale.coerceIn(1f, 1.4f)).dp).clearAndSetSemantics { contentDescription = said }, contentAlignment = Alignment.Center) {
            Canvas(Modifier.fillMaxSize()) {
                val outer = size.minDimension / 2f - 10.dp.toPx()
                val inner = outer * 0.58f
                val c = center
                val width = (2f * Math.PI.toFloat() * inner / 24f) * 0.62f
                drawCircle(ring, inner - width * 0.9f, c, style = Stroke(1.dp.toPx()))
                node.hours.take(24).forEachIndexed { hour, h ->
                    val angle = Math.toRadians(ClockRules.angle(hour + 0.5).toDouble())
                    val dir = Offset(cos(angle).toFloat(), sin(angle).toFloat())
                    val reach = ClockRules.reach(h, most)
                    val colour = h.category?.toInt()?.let { keyInks.getOrNull(it)?.mark } ?: base.mark
                    if (reach == null || h.value == null && h.level == null) {
                        drawCircle(quiet, width * 0.22f, c + dir * inner)
                    } else {
                        val end = inner + (outer - inner) * (reach * growth).coerceAtLeast(0.04f)
                        drawLine(if (reach == 0f) quiet else colour, c + dir * inner, c + dir * end, strokeWidth = width, cap = StrokeCap.Round)
                    }
                    if (node.now == hour) {
                        drawLine(outline, c + dir * (inner - width), c + dir * (outer + width * 0.2f), strokeWidth = width + 3.dp.toPx(), cap = StrokeCap.Round, alpha = 0.18f)
                    }
                }
                node.moments.forEachIndexed { index, moment ->
                    val angle = Math.toRadians(ClockRules.angle(moment.at).toDouble())
                    val dir = Offset(cos(angle).toFloat(), sin(angle).toFloat())
                    drawCircle(momentInks[index].mark, 4.dp.toPx(), c + dir * (outer + 6.dp.toPx()))
                }
            }
            Column(horizontalAlignment = Alignment.CenterHorizontally) {
                node.shown?.let { Text(it, style = MaterialTheme.typography.headlineMedium, maxLines = 1) }
                (node.unit ?: node.label)?.let { Text(it, style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant) }
            }
        }
        Key(node.key.mapIndexed { index, category -> keyInks[index].mark to category.name })
        if (node.moments.isNotEmpty()) {
            Key(
                node.moments.mapIndexed { index, moment ->
                    val minutes = (moment.at * 60).toInt().coerceIn(0, 24 * 60 - 1)
                    momentInks[index].mark to "${moment.name} ${LocalTime.of(minutes / 60, minutes % 60).format(times)}"
                },
            )
        }
    }
}

/**
 * A meter: one bar of segments that add up to the whole, each in its hue
 * with a hair between them, its share inside where it fits, and a key that
 * says each segment's amount in the page's words.
 */
@Composable
internal fun MeterView(node: Navree.Meter, modifier: Modifier = Modifier) {
    val scale = textScale()
    val shares = ShareRules.shares(node.segments.map { it.value })
    val inks = node.segments.mapIndexed { index, segment -> inkOf(segment.colour, SLOTS[index % SLOTS.size]) }
    val grownShare = grown(1f)
    val said = node.summary ?: node.segments.joinToString(", ") { listOfNotNull(it.name, it.shown).joinToString(" ") }
    Column(modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(12.dp)) {
        ChartHeading(node.title, node.hint)
        BoxWithConstraints(Modifier.fillMaxWidth().clearAndSetSemantics { contentDescription = said }) {
            val width = maxWidth.value
            Row(Modifier.fillMaxWidth().height((28f * scale.coerceIn(1f, 1.5f)).dp).clip(RoundedCornerShape(8.dp)), horizontalArrangement = Arrangement.spacedBy(2.dp)) {
                node.segments.forEachIndexed { index, _ ->
                    val share = shares[index] * grownShare
                    if (share > 0f) {
                        Box(Modifier.weight(share).fillMaxSize().background(inks[index].mark), contentAlignment = Alignment.Center) {
                            if (ShareRules.saysInside(shares[index], width, scale)) {
                                Text("${ShareRules.percent(shares[index])}%", style = MaterialTheme.typography.labelMedium, color = onMark(inks[index].mark), maxLines = 1)
                            }
                        }
                    }
                }
            }
        }
        Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
            node.segments.forEachIndexed { index, segment ->
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Box(Modifier.size(10.dp).clip(CircleShape).background(inks[index].mark))
                    Text(segment.name, style = MaterialTheme.typography.bodyMedium, modifier = Modifier.padding(start = 8.dp).weight(1f))
                    segment.shown?.let { Text(it, style = MaterialTheme.typography.labelLarge) }
                }
                segment.detail?.let { Text(it, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.padding(start = 18.dp)) }
            }
        }
    }
}

/** The colour of words on a mark: white on a deep one, near black on a light one. */
internal fun onMark(mark: Color): Color = if (mark.luminance() > 0.4f) Color(0xFF1B1B1F) else Color.White

/**
 * Columns: a column an item, each split into the same parts and drawn as a
 * whole, so they compare what each is made of. The column that stands out is
 * drawn in full and the others quieter.
 */
@Composable
internal fun MiniColumnsView(node: Navree.MiniColumns, modifier: Modifier = Modifier) {
    val scale = textScale()
    val inks = node.parts.mapIndexed { index, part -> inkOf(part.colour, SLOTS[index % SLOTS.size]) }
    val growth = grown(1f)
    val said = node.summary ?: node.columns.joinToString(", ") { listOfNotNull(it.label, it.shown).joinToString(" ") }
    Column(modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(12.dp)) {
        ChartHeading(node.title, node.hint)
        Row(
            Modifier.fillMaxWidth().clearAndSetSemantics { contentDescription = said },
            horizontalArrangement = Arrangement.spacedBy(12.dp),
            verticalAlignment = Alignment.Bottom,
        ) {
            node.columns.forEachIndexed { at, column ->
                val quiet = node.highlight != null && node.highlight!!.toInt() != at
                val shares = ShareRules.shares(column.values)
                Column(Modifier.weight(1f), horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(6.dp)) {
                    column.shown?.let { Text(it, style = MaterialTheme.typography.labelMedium, maxLines = 2, textAlign = TextAlign.Center) }
                    Column(
                        Modifier.fillMaxWidth().height((120f * scale.coerceIn(1f, 1.4f) * growth).dp.coerceAtLeast(1.dp)).clip(RoundedCornerShape(8.dp)),
                        verticalArrangement = Arrangement.spacedBy(2.dp),
                    ) {
                        // The first part at the bottom, as a stacked column is read.
                        for (index in shares.indices.reversed()) {
                            if (shares[index] > 0f) {
                                Box(Modifier.weight(shares[index]).fillMaxWidth().background(inks.getOrNull(index)?.mark?.copy(alpha = if (quiet) 0.45f else 1f) ?: Color.Gray))
                            }
                        }
                    }
                    Text(column.label, style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 1, overflow = TextOverflow.Ellipsis)
                }
            }
        }
        Key(node.parts.mapIndexed { index, part -> inks[index].mark to part.name })
    }
}

/**
 * A strip: equal cells, each in the hue of its category, a quiet cell where
 * there is no reading; the first and last cells' words under its ends, and a
 * key that gives each category's share when the page asks.
 */
@Composable
internal fun StripView(node: Navree.ColorStrip, modifier: Modifier = Modifier) {
    val scale = textScale()
    val inks = node.key.mapIndexed { index, category -> inkOf(category.colour, SLOTS[index % SLOTS.size]) }
    val quiet = MaterialTheme.colorScheme.surfaceContainerHighest
    val shares = ShareRules.categoryShares(node.cells, node.key.size)
    val growth = grown(1f)
    val said = node.summary ?: node.title ?: ""
    Column(modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(8.dp)) {
        ChartHeading(node.title, node.hint)
        Row(
            Modifier.fillMaxWidth().height((32f * scale.coerceIn(1f, 1.5f)).dp).clip(RoundedCornerShape(8.dp)).clearAndSetSemantics { contentDescription = said },
            horizontalArrangement = Arrangement.spacedBy(2.dp),
        ) {
            for (cell in node.cells) {
                val colour = cell.category?.toInt()?.let { inks.getOrNull(it)?.mark } ?: quiet
                Box(Modifier.weight(1f).fillMaxSize().background(colour.copy(alpha = colour.alpha * growth)))
            }
        }
        if (node.cells.isNotEmpty()) {
            Row(Modifier.fillMaxWidth()) {
                Text(node.cells.first().label, style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.weight(1f))
                if (node.cells.size > 1) Text(node.cells.last().label, style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
        }
        Key(node.key.mapIndexed { index, category -> inks[index].mark to (if (node.shares) "${category.name} ${ShareRules.percent(shares[index])}%" else category.name) })
    }
}

/**
 * A range: where one value sits between two ends, as a line of its zones in
 * their hues with a marker at the value, the value and its zone above it,
 * the page's words (or the ends' numbers) under the ends.
 */
@Composable
internal fun RangeView(node: Navree.RangeScale, modifier: Modifier = Modifier) {
    val zone = ZoneRules.zoneOf(node.value, node.zones)
    val ink = inkOf(zone?.colour, Navree.Hue.ACCENT)
    val zoneInks = node.zones.map { inkOf(it.colour, Navree.Hue.NEUTRAL) }
    val track = MaterialTheme.colorScheme.surfaceContainerHighest
    val surface = MaterialTheme.colorScheme.surface
    val at = grown(node.value?.let { ZoneRules.position(it, node.min, node.max) } ?: 0f)
    val said = node.summary ?: listOfNotNull(node.title, node.shown?.let { listOfNotNull(it, node.unit).joinToString(" ") }, zone?.label).joinToString(", ")
    Column(modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(8.dp)) {
        ChartHeading(node.title, node.hint)
        Column(Modifier.clearAndSetSemantics { contentDescription = said }, verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Row(verticalAlignment = Alignment.Bottom) {
                Text(if (node.value == null) "–" else node.shown ?: number(node.value!!), style = MaterialTheme.typography.headlineMedium)
                node.unit?.let { Text(it, style = MaterialTheme.typography.titleSmall, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.padding(start = 4.dp, bottom = 4.dp)) }
                Spacer(Modifier.weight(1f))
                zone?.label?.let { Text(it, style = MaterialTheme.typography.labelLarge, color = ink.mark, modifier = Modifier.padding(bottom = 4.dp)) }
            }
            val rtl = androidx.compose.ui.platform.LocalLayoutDirection.current == androidx.compose.ui.unit.LayoutDirection.Rtl
            Canvas(Modifier.fillMaxWidth().height(24.dp)) {
                val h = 8.dp.toPx()
                val y = size.height / 2f
                fun x(position: Float) = if (rtl) size.width * (1f - position) else size.width * position
                drawLine(track, Offset(0f, y), Offset(size.width, y), strokeWidth = h, cap = StrokeCap.Round)
                node.zones.forEachIndexed { index, z ->
                    val from = x(ZoneRules.position(z.from, node.min, node.max))
                    val to = x(ZoneRules.position(z.to, node.min, node.max))
                    drawLine(zoneInks[index].track, Offset(minOf(from, to) + 1f, y), Offset(maxOf(from, to) - 1f, y), strokeWidth = h)
                }
                if (node.value != null) {
                    val c = Offset(x(at).coerceIn(h, size.width - h), y)
                    drawCircle(surface, h * 1.3f, c)
                    drawCircle(ink.mark, h * 0.9f, c)
                }
            }
            Row(Modifier.fillMaxWidth()) {
                Text(node.low ?: number(node.min), style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.weight(1f))
                Text(node.high ?: number(node.max), style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
        }
        if (node.zones.any { it.label != null }) Key(node.zones.mapIndexedNotNull { index, z -> z.label?.let { zoneInks[index].mark to it } })
    }
}

/**
 * A figure: one number that matters on a tonal card, its label with an icon
 * in its hue, the number in the type scale's headline with its unit beside
 * it, its status, trend and caption under it, a small ring at its end and a
 * small chart under it. Tapping it opens what it is about.
 */
@Composable
internal fun HealthFigureView(node: Navree.Figure, place: Place, modifier: Modifier = Modifier) {
    val ui = LocalUi.current
    val ink = inkOf(node.colour, Navree.Hue.ACCENT)
    val action = node.action
    Card(
        modifier = modifier
            .fillMaxWidth()
            .clip(CardDefaults.shape)
            .then(if (action != null) Modifier.clickable(role = Role.Button) { ui.act(place, action) } else Modifier)
            .semantics(mergeDescendants = true) {},
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceContainerHigh),
    ) {
        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
            Row(verticalAlignment = Alignment.Top) {
                Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        node.icon?.let { IconView(it, Modifier.size(18.dp).padding(end = 0.dp), tint = ink.mark) }
                        if (node.icon != null) Spacer(Modifier.width(6.dp))
                        Text(node.label, style = MaterialTheme.typography.labelLarge, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.weight(1f, fill = false))
                        node.hint?.let { HintMark(it) }
                    }
                    Row(verticalAlignment = Alignment.Bottom) {
                        Text(node.value, style = MaterialTheme.typography.headlineMedium, color = if (node.tone == null) MaterialTheme.colorScheme.onSurface else textColour(node.tone, Navree.TextStyle.TITLE))
                        node.unit?.let { Text(it, style = MaterialTheme.typography.titleSmall, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.padding(start = 4.dp, bottom = 4.dp)) }
                    }
                }
                node.aside?.let { aside -> NodeView(aside, place) }
            }
            node.status?.let { StatusView(it) }
            node.trend?.let { trend ->
                val symbol = when (trend.direction) {
                    Navree.Direction.UP -> "trending_up"
                    Navree.Direction.DOWN -> "trending_down"
                    Navree.Direction.FLAT -> "trending_flat"
                }
                val tint = markColour(trend.tone)
                Row(verticalAlignment = Alignment.CenterVertically) {
                    symbolVector(symbol)?.let { Icon(it, contentDescription = null, tint = tint, modifier = Modifier.size(18.dp)) }
                    trend.text?.let { Text(it, style = MaterialTheme.typography.bodyMedium, color = tint, modifier = Modifier.padding(start = 4.dp)) }
                }
            }
            node.detail?.let { Text(it, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant) }
            node.chart?.let { chart -> NodeView(chart, place) }
        }
    }
}

/**
 * The day pager: the day in words between the steps to the day before and
 * after, the day itself opening Material's date picker bounded by the days
 * there are, "today" when another day is shown, and the day's small rings
 * before it. A step replaces the day in place; the core keeps the screen.
 */
@Composable
internal fun DayPagerView(node: Navree.DayPager, place: Place, modifier: Modifier = Modifier, inBar: Boolean = false) {
    val ui = LocalUi.current
    var choosing by remember { mutableStateOf(false) }
    val previous = node.words?.previous ?: stringResource(R.string.navree_previous_day)
    val next = node.words?.next ?: stringResource(R.string.navree_next_day)
    val choose = node.words?.choose ?: stringResource(R.string.navree_choose_day)
    fun go(href: String?) {
        href ?: return
        ui.act(place, Navree.Action.Navigate(href))
    }
    Row(modifier, verticalAlignment = Alignment.CenterVertically) {
        node.scores?.let { scores -> Box(Modifier.padding(end = 4.dp)) { NodeView(scores, place) } }
        IconButton(onClick = { go(node.previous) }, enabled = node.previous != null) {
            // The step back points to the start of the line: the forward chevron, turned.
            Icon(Glyphs.ChevronRight, contentDescription = previous, modifier = Modifier.scale(-1f, 1f))
        }
        TextButton(onClick = { if (node.pick != null) choosing = true }, enabled = node.pick != null, modifier = Modifier.semantics { contentDescription = "${node.day}. $choose" }) {
            Text(node.day, style = if (inBar) MaterialTheme.typography.titleLarge else MaterialTheme.typography.titleMedium, color = MaterialTheme.colorScheme.onSurface, maxLines = 1, overflow = TextOverflow.Ellipsis)
        }
        IconButton(onClick = { go(node.next) }, enabled = node.next != null) {
            Icon(Glyphs.ChevronRight, contentDescription = next)
        }
        node.today?.let { today ->
            TextButton(onClick = { go(today) }) { Text(stringResource(R.string.navree_today)) }
        }
    }
    val pick = node.pick
    if (choosing && pick != null) {
        val shown = runCatching { LocalDate.parse(node.date) }.getOrNull()
        val state = rememberDatePickerState(
            initialSelectedDateMillis = shown?.atStartOfDay(ZoneOffset.UTC)?.toInstant()?.toEpochMilli(),
            selectableDates = object : SelectableDates {
                override fun isSelectableDate(utcTimeMillis: Long): Boolean =
                    PagerRules.choosable(Instant.ofEpochMilli(utcTimeMillis).atZone(ZoneOffset.UTC).toLocalDate(), node.first, node.last)

                override fun isSelectableYear(year: Int): Boolean {
                    val low = node.first?.take(4)?.toIntOrNull()
                    val high = node.last?.take(4)?.toIntOrNull()
                    return (low == null || year >= low) && (high == null || year <= high)
                }
            },
        )
        DatePickerDialog(
            onDismissRequest = { choosing = false },
            confirmButton = {
                TextButton(onClick = {
                    choosing = false
                    state.selectedDateMillis?.let { millis ->
                        val day = Instant.ofEpochMilli(millis).atZone(ZoneOffset.UTC).toLocalDate()
                        ui.send(Events.act(place.id, Navree.Action.Navigate(pick.href), place.form, place.overlay, text = PagerRules.text(day)))
                    }
                }) { Text(stringResource(android.R.string.ok)) }
            },
            dismissButton = { TextButton(onClick = { choosing = false }) { Text(stringResource(android.R.string.cancel)) } },
        ) {
            DatePicker(state = state, title = { Text(choose, modifier = Modifier.padding(start = 24.dp, top = 16.dp)) })
        }
    }
}

/** A dp size scaled by the person's text size, within bounds. */
internal fun Dp.withText(scale: Float, most: Float = 1.5f): Dp = this * scale.coerceIn(1f, most)
