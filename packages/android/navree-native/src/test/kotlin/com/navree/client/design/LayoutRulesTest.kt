package com.navree.client.design

import com.navree.client.Fixtures
import com.navree.protocol.Navree
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import java.util.Locale

class LineRulesTest {
    private val activity = Fixtures.screen("screen-designed-health.json").body!!.walk()
        .filterIsInstance<Navree.Stack>()
        .first { it.axis == Navree.Axis.HORIZONTAL && it.children.any { child -> child is Navree.Rings } }

    @Test
    fun parts_keep_the_least_width_they_can_be_read_in() {
        assertEquals(184f, CompositionRules.leastAcrossDp(Navree.Ring(max = 100.0, prominence = Navree.Prominence.HIGH), 1f), 0.01f)
        assertEquals("rings with a legend: the dial, the gap, the legend", 148f + 24f + 96f, CompositionRules.leastAcrossDp(activity.children[0], 1f), 0.01f)
        assertEquals("a grid keeps one column", 132f, CompositionRules.leastAcrossDp(activity.children[1], 1f), 0.01f)
        assertEquals("one column grows with the text", 264f, CompositionRules.leastAcrossDp(activity.children[1], 2f), 0.01f)
        assertEquals("words give way", 0f, CompositionRules.leastAcrossDp(Navree.Text(value = "Steps"), 1f), 0f)
    }

    @Test
    fun a_line_fits_when_its_least_widths_and_gaps_fit_the_room() {
        assertTrue(CompositionRules.fitsAcross(416f, listOf(268f, 132f), 16f))
        assertFalse(CompositionRules.fitsAcross(415f, listOf(268f, 132f), 16f))
        assertTrue("one part has no gap", CompositionRules.fitsAcross(100f, listOf(100f), 16f))
    }

    @Test
    fun a_line_that_would_squeeze_a_part_is_a_column() {
        assertEquals("a phone's card", Navree.Axis.VERTICAL, CompositionRules.laidAxis(activity, activity.children, 347f, 16f, 1f))
        assertEquals("half a tablet's column", Navree.Axis.HORIZONTAL, CompositionRules.laidAxis(activity, activity.children, 466f, 16f, 1f))
        assertEquals("larger text no longer fits there", Navree.Axis.VERTICAL, CompositionRules.laidAxis(activity, activity.children, 466f, 16f, 1.4f))
    }

    @Test
    fun a_column_that_centres_its_parts_takes_the_width_unless_it_is_a_part_of_a_line() {
        val centred = Navree.Stack(align = Navree.Align.CENTER)
        assertTrue(CompositionRules.fillsWidth(centred, Navree.Axis.VERTICAL, inLine = false))
        assertTrue(CompositionRules.fillsWidth(centred.copy(align = Navree.Align.END), Navree.Axis.VERTICAL, inLine = false))
        assertFalse("a part of a line keeps what it needs", CompositionRules.fillsWidth(centred, Navree.Axis.VERTICAL, inLine = true))
        assertFalse("parts at the start stand at the start either way", CompositionRules.fillsWidth(Navree.Stack(), Navree.Axis.VERTICAL, inLine = false))
        assertFalse("a line is not a column", CompositionRules.fillsWidth(centred, Navree.Axis.HORIZONTAL, inLine = false))
    }

    @Test
    fun a_short_last_line_of_cards_or_figures_stretches_and_one_of_other_parts_does_not() {
        val pillars = Fixtures.screen("screen-designed-health.json").body!!.walk().filterIsInstance<Navree.Grid>().first { it.id == "pillars" }
        assertTrue(ListRules.stretchesLastLine(pillars.children))
        assertTrue(ListRules.stretchesLastLine(listOf(Navree.Figure(label = "Steps", value = "9,412"))))
        assertFalse("words and rings keep the columns of the other lines", ListRules.stretchesLastLine(listOf(Navree.Card(), Navree.Text(value = "a"))))
        assertFalse(ListRules.stretchesLastLine(emptyList()))
    }

    @Test
    fun a_line_the_page_lets_wrap_is_left_to_wrap() {
        val wrapping = activity.copy(wrap = true)
        assertEquals(Navree.Axis.HORIZONTAL, CompositionRules.laidAxis(wrapping, wrapping.children, 200f, 16f, 1f))
        assertFalse(CompositionRules.turns(wrapping))
    }

    @Test
    fun a_stack_that_turns_is_one_with_a_wide_axis_or_a_line_of_parts_with_a_least_width() {
        assertTrue(CompositionRules.turns(activity))
        assertTrue(CompositionRules.turns(Navree.Stack(axis = Navree.Axis.VERTICAL, wide = Navree.Axis.HORIZONTAL)))
        assertFalse(CompositionRules.turns(Navree.Stack(axis = Navree.Axis.HORIZONTAL, children = listOf(Navree.Text(value = "a"), Navree.Text(value = "b")))))
        assertFalse(CompositionRules.turns(Navree.Stack(children = listOf(Navree.Grid()))))
    }

    @Test
    fun a_part_that_grows_has_the_room_the_parts_that_keep_their_size_leave() {
        // The rings keep 268 dp, one gap of 16 dp, and the grid has the rest.
        assertEquals(466f - 268f - 16f, CompositionRules.roomLeftDp(466f, activity.children, 16f, 1f), 0.01f)
        assertEquals("never less than one", 1f, CompositionRules.roomLeftDp(100f, activity.children, 16f, 1f), 0f)
    }
}

class ColumnRulesTest {
    @Test
    fun the_column_is_as_wide_as_cards_need_only_where_cards_stand_side_by_side() {
        assertEquals(840, ContentRules.columnMostDp(false))
        assertEquals(1240, ContentRules.columnMostDp(true))
    }

    @Test
    fun what_a_column_holds_is_its_width_between_the_margins() {
        assertEquals("a pane narrower than the column", 1012f, ContentRules.contentDp(1060f, 1240, 24), 0.01f)
        assertEquals("a pane wider than the column", 1192f, ContentRules.contentDp(1600f, 1240, 24), 0.01f)
        assertEquals(792f, ContentRules.contentDp(1060f, 840, 24), 0.01f)
        assertEquals(0f, ContentRules.contentDp(30f, 840, 24), 0f)
    }

    @Test
    fun cards_in_a_line_share_the_width_with_a_gap_between() {
        assertEquals(498f, ContentRules.cardWidthDp(1012f, 2), 0.01f)
        assertEquals(1012f, ContentRules.cardWidthDp(1012f, 1), 0.01f)
        assertEquals(12f, ContentRules.cardWidthDp(48f, 3, gapDp = 6), 0.01f)
    }

    @Test
    fun a_line_with_fewer_cards_than_columns_stretches_them_across_it() {
        assertEquals(1, ContentRules.lineColumns(1, 2))
        assertEquals(2, ContentRules.lineColumns(2, 3))
        assertEquals(2, ContentRules.lineColumns(5, 2))
        assertEquals("never fewer than one", 1, ContentRules.lineColumns(0, 2))
    }
}

class DialRulesTest {
    @Test
    fun the_quarters_are_named_in_the_clock_the_person_keeps() {
        assertEquals(listOf(0, 6, 12, 18), ClockRules.QUARTERS)
        assertEquals(listOf("12 AM", "6 AM", "12 PM", "6 PM"), ClockRules.QUARTERS.map { ClockRules.quarterLabel(it, is24 = false, locale = Locale.US) })
        assertEquals(listOf("00", "06", "12", "18"), ClockRules.QUARTERS.map { ClockRules.quarterLabel(it, is24 = true, locale = Locale.US) })
    }

    @Test
    fun the_dial_has_the_bars_inside_the_moments_line_and_the_names_outside_it() {
        val dial = ClockRules.dial(150f, 1f)
        assertTrue(dial.inner < dial.outer && dial.outer < dial.moments && dial.moments < dial.labelEdge && dial.labelEdge < 150f)
        assertEquals("the moments stand on one line, ten dp beyond the longest bar", dial.outer + 10f, dial.moments, 0.01f)
        // The widest name, "12 PM" at the side, still fits between its edge and the dial's.
        assertTrue(150f - dial.labelEdge >= 30f)
    }

    @Test
    fun the_names_band_grows_with_the_text_to_a_limit() {
        val normal = ClockRules.dial(150f, 1f)
        val large = ClockRules.dial(150f, 1.3f)
        val largest = ClockRules.dial(150f, 2f)
        assertTrue(large.outer < normal.outer)
        assertEquals("beyond the limit the dial stops giving way", large.outer, largest.outer, 0.01f)
    }

    @Test
    fun capped_words_grow_with_the_text_only_to_the_cap() {
        assertEquals(11f, ClockRules.cappedSp(11f, 1f), 0.001f)
        assertEquals("grown 1.3 times once the size is applied", 11f * 1.3f, ClockRules.cappedSp(11f, 1.3f) * 1.3f, 0.001f)
        assertEquals(11f * 1.3f, ClockRules.cappedSp(11f, 2f) * 2f, 0.001f)
    }

    @Test
    fun the_total_in_the_hole_is_never_wider_than_the_hole() {
        // 28 sp at the normal size: five characters of it are about 78 dp, in a hole 96 dp across.
        assertEquals(28f, ClockRules.centreSp(96f, 5, 28f, 1f), 0.001f)
        val large = ClockRules.centreSp(96f, 5, 28f, 2f) * 2f
        assertTrue("at the largest text it is 31 dp or less, not 56", large < 32f)
    }

    @Test
    fun an_hours_bar_is_most_of_the_gap_between_neighbours() {
        assertEquals(2f * Math.PI.toFloat() * 56f / 24f * 0.62f, ClockRules.barDp(56f), 0.001f)
    }
}

class BidiRulesTest {
    @Test
    fun an_amount_with_its_unit_is_one_run_left_to_right() {
        assertEquals("⁦540/600 kcal⁩", BidiRules.amount("540/600 kcal"))
    }
}
