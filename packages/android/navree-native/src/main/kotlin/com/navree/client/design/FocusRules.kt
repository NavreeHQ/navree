package com.navree.client.design

/**
 * The order a keyboard's Tab goes through the three parts of a window, the
 * order a person reads it:
 *
 * - a phone (the navigation along the bottom): the top bar (back, title,
 *   search, actions), then the content, then the bottom navigation;
 * - a tablet and wider (the navigation at the start edge): the navigation,
 *   then the top bar, then the content.
 *
 * Right to left mirrors nothing in this order: the start edge is first in
 * either direction. Between the controls inside one part the system's own order
 * stands; only the order between the parts is set here.
 */
object FocusRules {
    /** The top bar of a screen. */
    const val TOP = 0

    /** What a screen is about: its list, its form. */
    const val CONTENT = 1

    /** The navigation: the bar along the bottom or the rail at the start. */
    const val NAV = 2

    /** The parts in the order Tab goes through them, for the navigation a window has. */
    fun sequence(kind: NavigationKind): List<Int> = when (kind) {
        NavigationKind.Rail, NavigationKind.ExpandedRail -> listOf(NAV, TOP, CONTENT)
        else -> listOf(TOP, CONTENT, NAV)
    }

    /** The part `step` places after (1) or before (-1) `at` among the parts that are on the screen; the order is a ring. Null when there is no other. */
    fun neighbour(sequence: List<Int>, present: Set<Int>, at: Int, step: Int): Int? {
        val live = sequence.filter { it in present }
        val here = live.indexOf(at)
        if (here < 0 || live.size < 2) return null
        return live[Math.floorMod(here + step, live.size)]
    }

    /** The first part of the screen that is there. */
    fun first(sequence: List<Int>, present: Set<Int>): Int? = sequence.firstOrNull { it in present }

    /** The last part of the screen that is there. */
    fun last(sequence: List<Int>, present: Set<Int>): Int? = sequence.lastOrNull { it in present }
}
