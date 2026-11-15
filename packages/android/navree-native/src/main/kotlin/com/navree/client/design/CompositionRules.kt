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
     * width of their own. A ring, a shape or an icon keeps its size. A part
     * for each room fills the line, so it shares what is left with the others
     * that grow: left to its own size it would take the whole line.
     */
    fun grows(node: Navree.Node): Boolean = when (node) {
        is Navree.Stack -> node.grow
        is Navree.Layers -> node.grow
        is Navree.Grid, is Navree.Carousel, is Navree.Chart, is Navree.Adaptive -> true
        // A field takes what is left of the line beside a button that keeps its size.
        is Navree.Field -> true
        else -> false
    }

    /**
     * Whether a column of parts takes the whole width it is given: one that
     * centres or ends its parts does, because they are placed within that
     * width, not within the width of the widest of them. A line across
     * (where the column is one part of a line) keeps the width it needs.
     */
    fun fillsWidth(stack: Navree.Stack, axis: Navree.Axis, inLine: Boolean): Boolean =
        axis == Navree.Axis.VERTICAL && !inLine && (columnAlign(stack) == Navree.Align.CENTER || columnAlign(stack) == Navree.Align.END)

    /**
     * How a stack laid as a column places its parts across: by the stack's own
     * `align`, whether or not it turns (the protocol: where parts sit across
     * the axis; a stack that turns is a column in a narrow room and a line in a
     * wide one, and says the same of both).
     */
    fun columnAlign(stack: Navree.Stack): Navree.Align? = stack.align

    /** Whether a line of parts holds a dial: a ring, a ring set or a gauge. */
    fun holdsDial(parts: List<Navree.Node>): Boolean = parts.any { it is Navree.Ring || it is Navree.Rings || it is Navree.Gauge }

    /** The share of a card's width a line of parts may fill and still be put in the middle of the card, not at its start. */
    const val SHORT_LINE = 0.7f

    /**
     * Whether a line of parts that fills `usedPx` of a room `roomPx` wide is
     * put in the middle: a line with a dial in it that leaves most of a wide
     * card empty beside it is a group in the card's middle, not a start with
     * a void after it.
     */
    fun centresShortLine(usedPx: Int, roomPx: Int): Boolean = roomPx > 0 && usedPx < roomPx * SHORT_LINE

    /**
     * How tall the cards of one line are: the tallest height any of them has
     * been seen to have (a card never shrinks under a line's other cards).
     */
    fun tallest(seenPx: Int, reportedPx: Int): Int = maxOf(seenPx, reportedPx)

    /**
     * How a stack laid as a column places its parts across, in a cell at the
     * end of a table's column: the lines of a block at the end end where the
     * widest line ends, so a short line stands under the others' end.
     */
    fun columnAlignIn(stack: Navree.Stack, endAligned: Boolean): Navree.Align? =
        columnAlign(stack) ?: if (endAligned) Navree.Align.END else null

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
        if (!fitsAcross(roomDp, leasts, gapDp)) return Navree.Axis.VERTICAL
        // Each part that grows is given an equal share of what is left of the line: a wall of cards that would have
        // one column in its share (and so end far below its neighbour) goes above it instead, as the guidance for a
        // supporting pane has the supporting content go below the main one where the main one does not fit its half.
        val share = roomLeftDp(roomDp, shown, gapDp, fontScale)
        return if (shown.filter(::grows).all { share >= growingLeastDp(it, fontScale) }) Navree.Axis.HORIZONTAL else Navree.Axis.VERTICAL
    }

    /**
     * The least a part that grows needs of its share of a line, in dp: a stack
     * that holds a wall of cards (a grid of at least three, with no count of
     * columns of its own) needs two columns of them; other parts give way.
     */
    fun growingLeastDp(node: Navree.Node, fontScale: Float): Float {
        if (node !is Navree.Stack) return 0f
        val wall = node.walk().filterIsInstance<Navree.Grid>().firstOrNull { it.cols == null && it.children.size >= 3 } ?: return 0f
        val column = ((wall.min ?: 160) * fontScale.coerceAtLeast(1f)).toInt().coerceAtLeast(48)
        return (2 * column + wall.gap * STEP_DP).toFloat()
    }

    /**
     * The room a part that grows has in a line: what is left of the line
     * after the parts that keep their size and the gaps between. At least 1.
     */
    fun roomLeftDp(roomDp: Float, shown: List<Navree.Node>, gapDp: Float, fontScale: Float): Float {
        val kept = shown.filter { !grows(it) }.sumOf { leastAcrossDp(it, fontScale).toDouble() }.toFloat()
        val growing = shown.count { grows(it) }.coerceAtLeast(1)
        return ((roomDp - kept - gapDp * (shown.size - 1).coerceAtLeast(0)) / growing).coerceAtLeast(1f)
    }

    /**
     * Whether a vertical stack is taken apart into the blocks of its screen,
     * each part drawn as the screen's own line. A stack that says more than
     * "one under the other from the start" is drawn whole, so what it says
     * holds: one that centres or ends its parts, one that turns along the
     * line in a wide room, one with padding. In a pane too narrow to turn it,
     * a stack that turns is a plain column and is taken apart like any.
     */
    fun splitsIntoBlocks(stack: Navree.Stack, paneDp: Float): Boolean =
        stack.axis == Navree.Axis.VERTICAL &&
            (stack.align == null || stack.align == Navree.Align.START) &&
            (stack.wide == null || stack.wide == stack.axis || !wide(paneDp - 2 * 24)) &&
            stack.pad == 0

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
