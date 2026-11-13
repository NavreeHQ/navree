package com.navree.client.design

import com.navree.client.Fixtures
import com.navree.protocol.Navree
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class CompositionRulesTest {
    @Test
    fun a_room_is_wide_from_600_dp_whatever_the_window() {
        assertFalse(CompositionRules.wide(599f))
        assertTrue(CompositionRules.wide(600f))
    }

    @Test
    fun a_stack_turns_to_its_wide_axis_only_in_a_wide_room() {
        val stack = Navree.Stack(axis = Navree.Axis.VERTICAL, wide = Navree.Axis.HORIZONTAL)
        assertEquals(Navree.Axis.VERTICAL, CompositionRules.axis(stack, 412f))
        assertEquals(Navree.Axis.HORIZONTAL, CompositionRules.axis(stack, 900f))
        assertEquals("no wide axis: its own", Navree.Axis.VERTICAL, CompositionRules.axis(stack.copy(wide = null), 900f))
    }

    @Test
    fun a_card_has_the_systems_padding_unless_the_page_gives_steps() {
        assertEquals(16, CompositionRules.cardPadDp(null))
        assertEquals(24, CompositionRules.cardPadDp(6))
    }

    @Test
    fun a_carousel_shows_as_many_parts_as_fit_and_at_least_one() {
        assertEquals("380 dp with 300 dp parts", 1, CompositionRules.carouselCount(380f, 300, 3))
        assertEquals(2, CompositionRules.carouselCount(700f, 300, 3))
        assertEquals(1, CompositionRules.carouselCount(120f, 300, 3))
        assertNull("no least width: each part its own width", CompositionRules.carouselCount(700f, null, 3))
        assertEquals((700f - 12f) / 2f, CompositionRules.carouselItemDp(700f, 2, 3), 1e-4f)
    }

    @Test
    fun parts_with_no_width_of_their_own_take_the_room_left() {
        assertTrue(CompositionRules.grows(Navree.Grid()))
        assertTrue(CompositionRules.grows(Navree.Chart()))
        assertTrue(CompositionRules.grows(Navree.Stack(grow = true)))
        assertFalse(CompositionRules.grows(Navree.Stack()))
        assertFalse("a ring keeps its size", CompositionRules.grows(Navree.Ring(max = 100.0)))
    }

    @Test
    fun icons_take_fixed_sizes_by_the_style_they_match() {
        assertEquals(24, CompositionRules.iconDp(null))
        assertEquals(16, CompositionRules.iconDp(Navree.TextStyle.CAPTION))
        assertEquals(24, CompositionRules.iconDp(Navree.TextStyle.BODY))
        assertEquals(48, CompositionRules.iconDp(Navree.TextStyle.DISPLAY))
    }

    @Test
    fun an_adaptive_draws_one_room() {
        val adaptive = Navree.Adaptive(compact = listOf(Navree.Text(value = "narrow")), wide = listOf(Navree.Text(value = "wide")))
        assertEquals("narrow", (CompositionRules.room(adaptive, 412f).single() as Navree.Text).value)
        assertEquals("wide", (CompositionRules.room(adaptive, 840f).single() as Navree.Text).value)
    }

    @Test
    fun the_designed_health_screen_has_its_day_control_in_the_app_bar() {
        val screen = Fixtures.screen("screen-designed-health.json")
        assertNotNull(screen.pager)
        assertEquals(screen.pager, PagerRules.inBar(screen))
    }
}
