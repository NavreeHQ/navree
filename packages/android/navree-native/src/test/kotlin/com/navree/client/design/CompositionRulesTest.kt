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
    fun a_dial_of_a_stack_that_turns_stands_in_the_middle_of_its_column_and_words_do_not() {
        val turning = Navree.Stack(axis = Navree.Axis.VERTICAL, wide = Navree.Axis.HORIZONTAL, align = Navree.Align.CENTER)
        val ring = Navree.Ring(max = 100.0, value = 83.0)
        assertTrue(CompositionRules.centresInColumn(turning, ring))
        assertFalse("words start at the start", CompositionRules.centresInColumn(turning, Navree.Text(value = "Five pillars")))
        assertFalse("a stack that does not turn centres by its own rule", CompositionRules.centresInColumn(turning.copy(wide = null), ring))
        assertFalse("not asked to centre", CompositionRules.centresInColumn(turning.copy(align = null), ring))
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
    fun a_part_for_each_room_shares_a_line_with_the_stack_that_grows_beside_it() {
        val adaptive = Navree.Adaptive()
        assertTrue(CompositionRules.grows(adaptive))
        // Beside one other part that grows it has half of what the line holds, not what is left over from nothing.
        assertEquals(493f, CompositionRules.roomLeftDp(1000f, listOf(Navree.Stack(grow = true), adaptive), 14f, 1f), 0.01f)
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

    @Test
    fun a_wall_of_cards_beside_a_side_column_goes_above_it_where_its_share_holds_one_column() {
        val cards = List(4) { Navree.Card() }
        val wall = Navree.Stack(grow = true, children = listOf(Navree.Grid(min = 160, gap = 3, children = cards)))
        val side = Navree.Stack(grow = true, children = listOf(Navree.Text(value = "Top sellers")))
        val line = Navree.Stack(axis = Navree.Axis.VERTICAL, wide = Navree.Axis.HORIZONTAL, gap = 4, children = listOf(wall, side))
        assertEquals("a medium window: one column of cards in half the room", Navree.Axis.VERTICAL, CompositionRules.laidAxis(line, line.children, 656f, 16f, 1f))
        assertEquals("a wide one: two columns of cards in half", Navree.Axis.HORIZONTAL, CompositionRules.laidAxis(line, line.children, 1012f, 16f, 1f))
        assertEquals(0f, CompositionRules.growingLeastDp(side, 1f), 0.001f)
        assertEquals(332f, CompositionRules.growingLeastDp(wall, 1f), 0.001f)
    }
}
