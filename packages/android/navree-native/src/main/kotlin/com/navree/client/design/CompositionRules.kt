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
