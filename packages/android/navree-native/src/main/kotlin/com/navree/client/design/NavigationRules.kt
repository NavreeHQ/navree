package com.navree.client.design


/** The component that holds the top-level destinations. */
enum class NavigationKind {
    /** One destination or none: no navigation component. */
    None,

    /** A navigation bar along the bottom. */
    Bar,

    /** A navigation rail along the start edge, icons with labels under them. */
    Rail,

    /** The rail expanded: icons with labels beside them, and group headings. */
    ExpandedRail,
}

/** Where the destinations go in this window. Destinations are numbered in the shell's order, across groups. */
data class NavigationPlan(
    val kind: NavigationKind,
    /** The destinations the component shows. */
    val shown: List<Int>,
    /** The destinations that did not fit. They are reached from the menu button. */
    val more: List<Int>,
    /**
     * Whether first screens carry a menu button. It opens the expanded rail
     * over the content with every destination in its group.
     */
    val menuButton: Boolean,
    /** Whether group headings are drawn in the component itself. */
    val groupHeadings: Boolean,
    /**
     * How many lines a destination's label may take. A label is never cut:
     * at the largest text sizes it wraps onto a second line and the bar grows.
     */
    val labelLines: Int = 1,
)

/**
 * Top-level navigation by window size, as Android's own adaptive scaffold
 * chooses it and Material 3 describes it:
 *
 * - compact width, or compact height (a phone on its side): a navigation bar;
 * - medium and expanded width: a navigation rail;
 * - large and extra-large width: the rail expanded, which takes the place the
 *   navigation drawer had before Material 3 retired it.
 *
 * A bar holds three to five destinations and a rail up to seven. What does
 * not fit is reached from a menu button that opens the expanded rail over the
 * content. Two destinations still get a bar; one or none gets nothing.
 */
object NavigationRules {
    const val BAR_MOST = 5
    const val RAIL_MOST = 7

    /** `groups` is how many destinations each group of the shell holds. */
    fun plan(window: WindowInfo, groups: List<Int>): NavigationPlan {
        val total = groups.sum()
        if (total <= 1) return NavigationPlan(NavigationKind.None, emptyList(), emptyList(), menuButton = false, groupHeadings = false)
        val lines = if (window.largeText) 2 else 1
        val all = (0 until total).toList()
        val kind = when {
            window.width == WidthClass.Compact || window.height == HeightClass.Compact -> NavigationKind.Bar
            window.width == WidthClass.Medium || window.width == WidthClass.Expanded -> NavigationKind.Rail
            else -> NavigationKind.ExpandedRail
        }
        val most = when (kind) {
            NavigationKind.Bar -> BAR_MOST
            NavigationKind.Rail -> RAIL_MOST
            else -> total
        }
        val shown = all.take(most)
        val more = all.drop(most)
        return NavigationPlan(
            kind = kind,
            shown = shown,
            more = more,
            menuButton = more.isNotEmpty(),
            groupHeadings = kind == NavigationKind.ExpandedRail && groups.count { it > 0 } > 1,
            labelLines = lines,
        )
    }
}
