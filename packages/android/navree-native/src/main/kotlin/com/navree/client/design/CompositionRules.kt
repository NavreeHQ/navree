package com.navree.client.design

import com.navree.protocol.Navree

/**
 * The rules of free composition (cards, layers, shapes, carousels, a part for
 * each room), as pure functions of the tree and the room it is drawn in.
 */
object CompositionRules {
    /** A step of the spacing scale, in dp: the same step `stack.gap` always used. */
    const val STEP_DP = 4

    /** A room at least this wide is wide: the place the content is drawn in, not the window. */
    const val WIDE_DP = 600

    fun wide(roomDp: Float): Boolean = roomDp >= WIDE_DP

    /** A card's padding: the page's steps, or the system's own 16 dp. */
    fun cardPadDp(pad: Int?): Int = pad?.let { it * STEP_DP } ?: 16

    /** The axis a stack lays its parts along: its wide axis in a wide room, else its own. */
    fun axis(stack: Navree.Stack, roomDp: Float): Navree.Axis = if (wide(roomDp)) stack.wide ?: stack.axis else stack.axis

    /**
     * How many parts of a carousel show at once: as many as fit at their least
     * width with the gaps between, at least one. With no least width each part
     * keeps its own width (null).
     */
    fun carouselCount(roomDp: Float, minDp: Int?, gapSteps: Int): Int? {
        val least = minDp ?: return null
        val gap = gapSteps * STEP_DP
        return ((roomDp + gap) / (least + gap)).toInt().coerceAtLeast(1)
    }

    /** The width of each part of a carousel that shows `count` at once. */
    fun carouselItemDp(roomDp: Float, count: Int, gapSteps: Int): Float = (roomDp - gapSteps * STEP_DP * (count - 1)) / count

    /**
     * Whether a part takes the room left in a horizontal stack: a stack or
     * layers that say so, and a grid, a carousel or a chart, which have no
     * width of their own. A ring, a shape or an icon keeps its size.
     */
    fun grows(node: Navree.Node): Boolean = when (node) {
        is Navree.Stack -> node.grow
        is Navree.Layers -> node.grow
        is Navree.Grid, is Navree.Carousel, is Navree.Chart -> true
        else -> false
    }

    /** The room a ring's legend needs beside its dial, and the space between the two, in dp. */
    private const val LEGEND_DP = 96f
    private const val LEGEND_GAP_DP = 24f

    /** The least a chart keeps beside other parts of a line, in dp. */
    private const val CHART_LEAST_DP = 160f

    /**
     * The least width, in dp, a part keeps when it stands in a line with
     * others: it is never squeezed below it. A ring keeps its diameter, a
     * ring set its diameter and its legend, a grid one column, a carousel
     * its least part, a chart a width it can be read in. Other parts take
     * what they need and give way, so their least is none.
     */
    fun leastAcrossDp(node: Navree.Node, fontScale: Float): Float = when (node) {
        is Navree.Ring -> RingRules.diameterDp(node.prominence, fontScale)
        is Navree.Rings -> {
            val dial = RingsRules.diameterDp(node.prominence, node.rings.size.coerceAtMost(RingsRules.MOST), fontScale)
            if (node.legend && node.prominence != Navree.Prominence.LOW) dial + LEGEND_GAP_DP + LEGEND_DP else dial
        }
        // As the grid's own columns count it: one column at its least width, grown with the text.
        is Navree.Grid -> ((node.min ?: 160) * fontScale.coerceAtLeast(1f)).toInt().coerceAtLeast(48).toFloat()
        is Navree.Carousel -> (node.min ?: 0).toFloat()
        is Navree.Chart -> CHART_LEAST_DP
        else -> 0f
    }

    /** Whether a stack may lay its parts along another axis than it was given, because of its width or because they would not fit. */
    fun turns(stack: Navree.Stack, fontScale: Float = 1f): Boolean =
        (stack.wide != null && stack.wide != stack.axis) ||
            (stack.axis == Navree.Axis.HORIZONTAL && !stack.wrap && stack.children.any { leastAcrossDp(it, fontScale) > 0f })

    /** Whether parts with these least widths fit in a line this wide, the gaps between them counted. */
    fun fitsAcross(roomDp: Float, leasts: List<Float>, gapDp: Float): Boolean =
        leasts.sum() + gapDp * (leasts.size - 1).coerceAtLeast(0) <= roomDp

    /**
     * The axis a stack is laid along: the one its axis and its wide axis say
     * for the room, but a line that would squeeze a part below its least
     * width is a column instead, as a wrapping line of parts would be.
     */
    fun laidAxis(stack: Navree.Stack, shown: List<Navree.Node>, roomDp: Float, gapDp: Float, fontScale: Float): Navree.Axis {
        val wanted = axis(stack, roomDp)
        if (wanted != Navree.Axis.HORIZONTAL || stack.wrap) return wanted
        val leasts = shown.map { leastAcrossDp(it, fontScale) }
        return if (fitsAcross(roomDp, leasts, gapDp)) Navree.Axis.HORIZONTAL else Navree.Axis.VERTICAL
    }

    /**
     * The room a part that grows has in a line: what is left of the line
     * after the parts that keep their size and the gaps between. At least 1.
     */
    fun roomLeftDp(roomDp: Float, shown: List<Navree.Node>, gapDp: Float, fontScale: Float): Float {
        val kept = shown.filter { !grows(it) }.sumOf { leastAcrossDp(it, fontScale).toDouble() }.toFloat()
        return (roomDp - kept - gapDp * (shown.size - 1).coerceAtLeast(0)).coerceAtLeast(1f)
    }

    /** An icon as large as a text style, in dp. Icons do not grow with the text, as Google's guidance has it. */
    fun iconDp(scale: Navree.TextStyle?): Int = when (scale) {
        Navree.TextStyle.CAPTION -> 16
        Navree.TextStyle.TITLE2 -> 28
        Navree.TextStyle.TITLE -> 32
        Navree.TextStyle.LARGE_TITLE -> 40
        Navree.TextStyle.DISPLAY -> 48
        else -> 24
    }

    /** The part of an `adaptive` drawn in a room this wide. */
    fun room(adaptive: Navree.Adaptive, roomDp: Float): List<Navree.Node> = if (wide(roomDp)) adaptive.wide else adaptive.compact
}
