package com.navree.client.design

import com.navree.protocol.Navree
import java.time.DayOfWeek
import java.time.LocalDate
import java.time.temporal.ChronoUnit
import java.time.temporal.TemporalAdjusters
import java.text.DateFormatSymbols
import java.util.Locale
import kotlin.math.max
import kotlin.math.min

/*
 * The placement and measuring rules of the health charts, as pure functions
 * of the tree, the window and the person's text size, so each can be tested
 * without a device. Drawing them is `ui/Health.kt`.
 */

/** Rings: one value against its goal, a second lap past it. */
object RingRules {
    /** The ring's width in dp: the hero ring, one of several, a small one beside words. Rings with words inside grow with the text. */
    fun diameterDp(prominence: Navree.Prominence, fontScale: Float): Float {
        val base = when (prominence) {
            Navree.Prominence.HIGH -> 184f
            Navree.Prominence.MEDIUM -> 88f
            Navree.Prominence.LOW -> 36f
        }
        return if (prominence == Navree.Prominence.LOW) base else base * fontScale.coerceIn(1f, 1.6f)
    }

    /** The width of the arc in dp. */
    fun strokeDp(prominence: Navree.Prominence): Float = when (prominence) {
        Navree.Prominence.HIGH -> 18f
        Navree.Prominence.MEDIUM -> 9f
        Navree.Prominence.LOW -> 5f
    }

    /** Whether words fit inside: a small ring says its value to TalkBack only. */
    fun showsCentre(prominence: Navree.Prominence, centre: Navree.Centre): Boolean =
        prominence != Navree.Prominence.LOW && centre != Navree.Centre.NOTHING

    /** How far round a value goes: whole laps past the goal, and the part of the lap it is on. Null for no value. */
    fun sweep(value: Double?, max: Double): Sweep? {
        if (value == null) return null
        if (max <= 0.0) return Sweep(0, if (value > 0) 1f else 0f)
        val share = (value / max).coerceAtLeast(0.0)
        val laps = share.toInt()
        val part = (share - laps).toFloat()
        // Exactly at the goal is one full lap, not a second one starting.
        return if (laps >= 1 && part == 0f) Sweep(laps - 1, 1f) else Sweep(laps, part)
    }

    data class Sweep(val laps: Int, val part: Float) {
        /** Whether the value is past the goal: the arc goes round a second time. */
        val past: Boolean get() = laps >= 1
    }
}

/** Concentric rings, the first outside. */
object RingsRules {
    /** At most six are drawn, as the protocol says. */
    const val MOST = 6

    /** The width of the whole in dp. */
    fun diameterDp(prominence: Navree.Prominence, count: Int, fontScale: Float): Float = when (prominence) {
        Navree.Prominence.LOW -> 36f
        Navree.Prominence.MEDIUM -> 148f + 8f * (count - 3).coerceAtLeast(0)
        Navree.Prominence.HIGH -> 184f
    } * if (prominence == Navree.Prominence.LOW) 1f else fontScale.coerceIn(1f, 1.4f)

    /** The arc width and the gap between rings, in dp, for a whole this wide. */
    fun strokeDp(diameterDp: Float, count: Int): Float {
        val n = count.coerceIn(1, MOST)
        // Rings and their gaps take a little under half the radius together, so the middle keeps room.
        return (diameterDp * 0.46f / 2f) / (n + (n - 1) * 0.25f)
    }

    fun gapDp(strokeDp: Float): Float = strokeDp * 0.25f

    /** The diameter of ring `index` (0 the outside one), at the middle of its arc. */
    fun ringDiameterDp(diameterDp: Float, index: Int, strokeDp: Float): Float =
        diameterDp - strokeDp - index * 2f * (strokeDp + gapDp(strokeDp))
}

/** Zones along a scale: a gauge's, a range's, a chart's. */
object ZoneRules {
    /** The zone a value is in: the last one that holds it, as the protocol says. */
    fun zoneOf(value: Double?, zones: List<Navree.Zone>): Navree.Zone? =
        if (value == null) null else zones.lastOrNull { value >= min(it.from, it.to) && value <= max(it.from, it.to) }

    /** Where a value sits between two ends, 0 to 1; outside it sits at the nearer end. */
    fun position(value: Double, low: Double, high: Double): Float =
        if (high <= low) 0f else ((value - low) / (high - low)).toFloat().coerceIn(0f, 1f)
}

/** The day clock: 24 hours round a dial, midnight at the top. */
object ClockRules {
    /** The angle in degrees (0 is to the right, as a canvas counts) of a time of day in hours. */
    fun angle(hours: Double): Float = (-90.0 + hours * 15.0).toFloat()

    /** How far out an hour's mark reaches, 0 to 1: its level, else its value against the most, else nothing. */
    fun reach(hour: Navree.Hour, most: Double?): Float? {
        hour.level?.let { return it.toFloat().coerceIn(0f, 1f) }
        val value = hour.value ?: return null
        val top = most ?: return null
        return if (top <= 0) 0f else (value / top).toFloat().coerceIn(0f, 1f)
    }

    /** The most an hour holds: the clock's own `max`, else the busiest hour. */
    fun most(clock: Navree.DayClock): Double? = clock.max ?: clock.hours.mapNotNull { it.value }.maxOrNull()

    /** The hours that are named round the dial: the four quarters, midnight at the top. */
    val QUARTERS = listOf(0, 6, 12, 18)

    /**
     * An hour as the person's clock writes it round the dial: "12 AM", "6 PM"
     * on a 12 hour clock (the locale's own words for the halves of the day),
     * "00" and "18" on a 24 hour one.
     */
    fun quarterLabel(hour: Int, is24: Boolean, locale: Locale): String {
        if (is24) return "%02d".format(locale, hour % 24)
        val halves = DateFormatSymbols.getInstance(locale).amPmStrings
        val twelve = if (hour % 12 == 0) 12 else hour % 12
        return "%d %s".format(locale, twelve, halves[if (hour % 24 < 12) 0 else 1])
    }

    /**
     * A time of day in hours past midnight as the person's clock writes it:
     * "7:07 AM" on a 12 hour clock, "07:07" on a 24 hour one.
     */
    fun timeLabel(at: Double, is24: Boolean, locale: Locale): String {
        val minutes = Math.round(((at % 24.0) + 24.0) % 24.0 * 60.0).toInt() % (24 * 60)
        val hour = minutes / 60
        val minute = minutes % 60
        if (is24) return "%02d:%02d".format(locale, hour, minute)
        val halves = DateFormatSymbols.getInstance(locale).amPmStrings
        val twelve = if (hour % 12 == 0) 12 else hour % 12
        return "%d:%02d %s".format(locale, twelve, minute, halves[if (hour < 12) 0 else 1])
    }

    /**
     * What the key says of a moment of the day: its name and its time, the
     * time written in the person's clock. A name that already holds a time is
     * said as it is.
     */
    fun momentEntry(name: String, at: Double, is24: Boolean, locale: Locale): String =
        if (Regex("\\d:\\d\\d").containsMatchIn(name)) name else name + ", " + timeLabel(at, is24, locale)

    /** The radii of the dial, in dp, for one that is `halfDp` from its middle to its edge. */
    data class Dial(
        /** Where the hours' bars end at their longest. */
        val outer: Float,
        /** Where they start. */
        val inner: Float,
        /** The line the day's moments stand on, outside the bars. */
        val moments: Float,
        /** Where the names of the quarters begin: their edge nearest the dial. */
        val labelEdge: Float,
    )

    /**
     * The radii of a dial: the hours' bars inside, the one line of the day's
     * moments round them, the quarters' names outside that, in a band that
     * grows with the text (to a limit; the names are a help, the summary says it all).
     */
    fun dial(halfDp: Float, fontScale: Float): Dial {
        val s = fontScale.coerceIn(1f, LABEL_CAP)
        val moments = (halfDp - (10f + 34f * s)).coerceAtLeast(48f)
        val outer = moments - 10f
        return Dial(outer = outer, inner = outer * 0.58f, moments = moments, labelEdge = moments + 9f)
    }

    /** How far the names round a dial grow with the person's text. */
    const val LABEL_CAP = 1.3f

    /**
     * A size in sp that, once the person's text size is applied, is as large
     * as `baseSp` grown by the text size up to `cap` and no further.
     */
    fun cappedSp(baseSp: Float, fontScale: Float, cap: Float = LABEL_CAP): Float =
        baseSp * minOf(fontScale.coerceAtLeast(1f), cap) / fontScale.coerceAtLeast(1f)

    /**
     * The size in sp for words in the middle of a dial: as large as `wantSp`
     * grown by the text size, but never wider than the hole they stand in,
     * which is `holeDp` across, for a text of `chars` characters.
     */
    fun centreSp(holeDp: Float, chars: Int, wantSp: Float, fontScale: Float): Float {
        val scale = fontScale.coerceAtLeast(1f)
        val fits = holeDp * 0.9f / (chars.coerceAtLeast(1) * 0.56f)
        return minOf(wantSp * scale, fits) / scale
    }

    /** The width of one hour's bar at the dial's start, in dp: most of the gap between neighbours. */
    fun barDp(innerDp: Float): Float = (2f * Math.PI.toFloat() * innerDp / 24f) * 0.62f
}

/** The key of a chart: each kind's name and what the page says of it. */
object KeyRules {
    /** An entry of a key: the name and, when the page says it, its share or total beside it. */
    fun entry(name: String, detail: String?): String = if (detail.isNullOrBlank()) name else "$name \u00b7 $detail"
}

/** What a progress bar's fill takes its colour from. */
object ProgressRules {
    enum class Fill { Measure, Tone, Accent }

    /** The colour of the measure the bar belongs to wins over the colour of what it means; with neither, the accent. */
    fun fill(progress: Navree.Progress): Fill = when {
        progress.colour != null -> Fill.Measure
        progress.tone != null -> Fill.Tone
        else -> Fill.Accent
    }
}

/** A ring's trend, drawn with the system's own arrow. */
object TrendRules {
    /** The Material Symbol that says which way a trend goes. */
    fun symbol(direction: Navree.Direction): String = when (direction) {
        Navree.Direction.UP -> "trending_up"
        Navree.Direction.DOWN -> "trending_down"
        Navree.Direction.FLAT -> "trending_flat"
    }

    /** Whether a ring says its status and trend: one with room, not a small one beside words. */
    fun saysStatus(prominence: Navree.Prominence): Boolean = prominence != Navree.Prominence.LOW
}

/** Numbers and their units in the right to left languages. */
object BidiRules {
    private const val ISOLATE_LTR = '\u2066'
    private const val POP_ISOLATE = '\u2069'

    /**
     * Text that is read as one run left to right, whatever the language
     * around it: an amount with its unit ("540/600 kcal"), which would
     * otherwise read "kcal 540/600" in a right to left line.
     */
    fun amount(text: String): String = "$ISOLATE_LTR$text$POP_ISOLATE"
}

/** Parts of a whole: a meter's segments, a column's parts, a strip's cells. */
object ShareRules {
    /** Each amount's share of their total; amounts left out count as none. All zero: all none. */
    fun shares(amounts: List<Double?>): List<Float> {
        val total = amounts.sumOf { (it ?: 0.0).coerceAtLeast(0.0) }
        return amounts.map { if (total <= 0) 0f else ((it ?: 0.0).coerceAtLeast(0.0) / total).toFloat() }
    }

    /** Whether a share's percentage fits inside its part of a bar this wide. */
    fun saysInside(share: Float, barDp: Float, fontScale: Float): Boolean = share * barDp >= 40f * fontScale.coerceAtLeast(1f)

    /** A share as a whole percentage, never 0 for a part that has some. */
    fun percent(share: Float): Int = if (share <= 0f) 0 else max(1, Math.round(share * 100))

    /** How many cells of a strip are of each category of its key, as shares of the cells that have one. */
    fun categoryShares(cells: List<Navree.StripCell>, keySize: Int): List<Float> {
        val counts = IntArray(keySize)
        for (cell in cells) cell.category?.toInt()?.takeIf { it in 0 until keySize }?.let { counts[it]++ }
        val total = counts.sum()
        return counts.map { if (total == 0) 0f else it.toFloat() / total }
    }
}

/** The calendar of days: a week a column, a day a cell. */
object HeatRules {
    /**
     * The shade of a day, 0 to 3, or null for a day with no value (no data,
     * not zero). `steps` are where the second, third and fourth shade begin;
     * without them, four equal parts from 0 to the highest value.
     */
    fun shade(value: Double?, steps: List<Double>, highest: Double): Int? {
        if (value == null) return null
        val bounds = if (steps.size >= 3) steps.take(3) else (1..3).map { highest * it / 4.0 }
        return bounds.count { value >= it }
    }

    /** The first day of the first week shown: the week of `first`, starting on the person's first weekday. */
    fun start(first: LocalDate, weekStart: DayOfWeek): LocalDate = first.with(TemporalAdjusters.previousOrSame(weekStart))

    /** The column (week) and the row (day of the week) of a date. */
    fun cell(date: LocalDate, start: LocalDate): Pair<Int, Int> {
        val days = ChronoUnit.DAYS.between(start, date).toInt()
        return Math.floorDiv(days, 7) to Math.floorMod(days, 7)
    }

    /** How many weeks the calendar spans. */
    fun weeks(first: LocalDate, last: LocalDate, weekStart: DayOfWeek): Int = cell(last, start(first, weekStart)).first + 1
}

/** Every measure against every other. */
object MatrixRules {
    /** The relation of two measures, either way round. */
    fun relation(relations: List<Navree.Relation>, a: Int, b: Int): Navree.Relation? =
        relations.firstOrNull { (it.a.toInt() == a && it.b.toInt() == b) || (it.a.toInt() == b && it.b.toInt() == a) }

    /** Whether a cell is drawn: with `lower`, only those under the diagonal. */
    fun drawn(row: Int, column: Int, lower: Boolean): Boolean = if (lower) column < row else row != column

    /** How strong a relation is, 0 to 1 against `peak` (1 when none is given); not significant or untested is none. */
    fun strength(relation: Navree.Relation?, peak: Double?): Float {
        val value = relation?.value ?: return 0f
        if (!relation.significant) return 0f
        val top = peak?.takeIf { it > 0 } ?: 1.0
        return (kotlin.math.abs(value) / top).toFloat().coerceIn(0f, 1f)
    }
}

/** The bounds of a chart's axes. */
object AxisRules {
    data class Bounds(val low: Double, val high: Double) {
        val span: Double get() = (high - low).takeIf { it > 0 } ?: 1.0

        fun at(value: Double): Float = ((value - low) / span).toFloat()
    }

    /**
     * The value axis of a chart: the measure's own bounds where it gives
     * them; else from zero (bars always stand on zero) to the highest value,
     * reference or zone, a little beyond so a mark never touches the top.
     */
    fun values(chart: Navree.Chart): Bounds {
        val stacked = chart.stacking == Navree.Stacking.VALUES
        val shares = chart.stacking == Navree.Stacking.SHARES
        if (shares) return Bounds(0.0, 1.0)
        if (chart.levels.isNotEmpty()) return Bounds(0.0, (chart.levels.size - 1).toDouble().coerceAtLeast(1.0))
        val count = chart.series.maxOfOrNull { it.values.size } ?: 0
        val tops = if (stacked) {
            (0 until count).map { at -> chart.series.sumOf { (it.values.getOrNull(at) ?: 0.0).coerceAtLeast(0.0) } }
        } else {
            chart.series.flatMap { it.values.filterNotNull() }
        }
        val all = tops + chart.references.mapNotNull { it.value } + chart.zones.flatMap { listOf(it.from, it.to) }
        val lowest = all.minOrNull() ?: 0.0
        val highest = all.maxOrNull() ?: 1.0
        val bars = chart.kind == Navree.ChartKind.BAR
        val low = chart.y?.min ?: if (bars || lowest >= 0 && lowest < highest * 0.5) 0.0 else lowest - (highest - lowest) * 0.1
        val high = chart.y?.max ?: (highest + (highest - low) * 0.08)
        // Ends the page did not give are rounded out to numbers a person reads at a glance.
        val round = nice(low, if (high > low) high else low + 1.0)
        return Bounds(chart.y?.min ?: round.low, chart.y?.max ?: round.high)
    }

    /** Two ends rounded out to a step of 1, 2, 2.5 or 5 times a power of ten, about four steps apart. */
    fun nice(low: Double, high: Double): Bounds {
        val span = high - low
        if (span <= 0) return Bounds(low, low + 1.0)
        val raw = span / 4.0
        val power = Math.pow(10.0, Math.floor(Math.log10(raw)))
        val step = when (val n = raw / power) {
            in 0.0..1.0 -> 1.0
            in 1.0..2.0 -> 2.0
            in 2.0..2.5 -> 2.5
            in 2.5..5.0 -> 5.0
            else -> 10.0
        } * power
        return Bounds(Math.floor(low / step) * step, Math.ceil(high / step) * step)
    }

    /** A scatter's two axes; with equal axes, one range for both and a square plot. */
    fun scatter(plot: Navree.Scatter): Pair<Bounds, Bounds> {
        val points = plot.series.flatMap { it.points }
        fun padded(values: List<Double>, measure: Navree.Measure?): Bounds {
            val low = values.minOrNull() ?: 0.0
            val high = values.maxOrNull() ?: 1.0
            val pad = (high - low).takeIf { it > 0 }?.times(0.08) ?: 1.0
            val round = nice(low - pad, high + pad)
            return Bounds(measure?.min ?: round.low, measure?.max ?: round.high)
        }
        val x = padded(points.map { it.x }, plot.x)
        val y = padded(points.map { it.y }, plot.y)
        if (!plot.equalAxes) return x to y
        val both = Bounds(min(x.low, y.low), max(x.high, y.high))
        return both to both
    }

    /** The values of a stacked chart at one place, each as where its part starts and ends. */
    fun stack(chart: Navree.Chart, at: Int): List<Pair<Double, Double>> {
        val values = chart.series.map { (it.values.getOrNull(at) ?: 0.0).coerceAtLeast(0.0) }
        val total = values.sum()
        var start = 0.0
        return values.map { value ->
            val part = if (chart.stacking == Navree.Stacking.SHARES) (if (total > 0) value / total else 0.0) else value
            val span = start to start + part
            start += part
            span
        }
    }
}

/** The day pager: the day a screen is about, stepped in place. */
object PagerRules {
    /**
     * The day control in the app bar, as a date is in Material's own day
     * views: the screen's own pager, or a pager that leads its body.
     */
    fun inBar(screen: Navree.Screen): Navree.DayPager? {
        screen.pager?.let { return it }
        val body = screen.body
        if (body is Navree.DayPager) return body
        val first = (body as? Navree.Stack)?.children?.firstOrNull { !(it is Navree.Stack && it.hidden) }
        return first as? Navree.DayPager
    }

    /** Whether a day may be chosen in the date picker: inside `first` and `last`, where they are given. */
    fun choosable(day: LocalDate, first: String?, last: String?): Boolean {
        val low = first?.let { runCatching { LocalDate.parse(it) }.getOrNull() }
        val high = last?.let { runCatching { LocalDate.parse(it) }.getOrNull() }
        return (low == null || !day.isBefore(low)) && (high == null || !day.isAfter(high))
    }

    /** The day as the core wants it in `text`. */
    fun text(day: LocalDate): String = day.toString()
}
