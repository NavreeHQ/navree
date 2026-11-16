package com.navree.client.design

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class FocusRulesTest {
    private val all = setOf(FocusRules.TOP, FocusRules.CONTENT, FocusRules.NAV)

    @Test
    fun on_a_phone_the_top_bar_comes_first_then_the_content_then_the_bottom_navigation() {
        assertEquals(listOf(FocusRules.TOP, FocusRules.CONTENT, FocusRules.NAV), FocusRules.sequence(NavigationKind.Bar))
        assertEquals(listOf(FocusRules.TOP, FocusRules.CONTENT, FocusRules.NAV), FocusRules.sequence(NavigationKind.None))
    }

    @Test
    fun on_a_tablet_the_navigation_comes_first_then_the_top_bar_then_the_content() {
        val expected = listOf(FocusRules.NAV, FocusRules.TOP, FocusRules.CONTENT)
        assertEquals(expected, FocusRules.sequence(NavigationKind.Rail))
        assertEquals(expected, FocusRules.sequence(NavigationKind.ExpandedRail))
    }

    @Test
    fun forwards_and_backwards_go_round_the_parts_that_are_there() {
        val phone = FocusRules.sequence(NavigationKind.Bar)
        assertEquals(FocusRules.CONTENT, FocusRules.neighbour(phone, all, FocusRules.TOP, 1))
        assertEquals(FocusRules.NAV, FocusRules.neighbour(phone, all, FocusRules.CONTENT, 1))
        assertEquals("past the last the first", FocusRules.TOP, FocusRules.neighbour(phone, all, FocusRules.NAV, 1))
        assertEquals("before the first the last", FocusRules.NAV, FocusRules.neighbour(phone, all, FocusRules.TOP, -1))
        assertEquals(FocusRules.TOP, FocusRules.neighbour(phone, all, FocusRules.CONTENT, -1))
    }

    @Test
    fun a_part_that_is_not_on_the_screen_is_skipped_and_a_single_part_has_no_neighbour() {
        val phone = FocusRules.sequence(NavigationKind.Bar)
        val noBar = setOf(FocusRules.CONTENT, FocusRules.NAV)
        assertEquals(FocusRules.NAV, FocusRules.neighbour(phone, noBar, FocusRules.CONTENT, 1))
        assertEquals(FocusRules.CONTENT, FocusRules.neighbour(phone, noBar, FocusRules.NAV, 1))
        assertNull(FocusRules.neighbour(phone, setOf(FocusRules.CONTENT), FocusRules.CONTENT, 1))
        assertNull("a part that is not there", FocusRules.neighbour(phone, noBar, FocusRules.TOP, 1))
    }

    @Test
    fun the_first_and_the_last_of_a_window_are_the_first_and_the_last_that_are_there() {
        val tablet = FocusRules.sequence(NavigationKind.Rail)
        assertEquals(FocusRules.NAV, FocusRules.first(tablet, all))
        assertEquals(FocusRules.CONTENT, FocusRules.last(tablet, all))
        assertEquals(FocusRules.TOP, FocusRules.first(tablet, setOf(FocusRules.TOP, FocusRules.CONTENT)))
        assertNull(FocusRules.first(tablet, emptySet()))
    }
}
