package com.navree.client.design

import com.navree.client.Fixtures
import com.navree.protocol.Navree
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Test
import java.time.DayOfWeek
import java.time.LocalDate

class RingRulesTest {
    @Test
    fun the_hero_ring_is_largest_and_rings_with_words_grow_with_the_text() {
        val high = RingRules.diameterDp(Navree.Prominence.HIGH, 1f)
        val medium = RingRules.diameterDp(Navree.Prominence.MEDIUM, 1f)
        val low = RingRules.diameterDp(Navree.Prominence.LOW, 1f)
        assertTrue(high > medium && medium > low)
        assertTrue("176 or more, as the protocol has the hero", high >= 176f)
        assertTrue("about 56 or less", low <= 56f)
        assertEquals(medium * 1.6f, RingRules.diameterDp(Navree.Prominence.MEDIUM, 2f), 0.01f)
        assertEquals("a small ring holds no words, so it keeps its size", low, RingRules.diameterDp(Navree.Prominence.LOW, 2f), 0.01f)
    }

    @Test
    fun a_small_ring_says_its_value_to_talkback_only() {
        assertFalse(RingRules.showsCentre(Navree.Prominence.LOW, Navree.Centre.VALUE))
        assertTrue(RingRules.showsCentre(Navree.Prominence.MEDIUM, Navree.Centre.VALUE))
        assertFalse(RingRules.showsCentre(Navree.Prominence.HIGH, Navree.Centre.NOTHING))
    }

    @Test
    fun a_value_past_its_goal_goes_round_again() {
        assertEquals(RingRules.Sweep(0, 0.83f), RingRules.sweep(83.0, 100.0))
        assertEquals("exactly at the goal is one full lap", RingRules.Sweep(0, 1f), RingRules.sweep(100.0, 100.0))
        val past = RingRules.sweep(38.0, 30.0)!!
        assertTrue(past.past)
        assertEquals(1, past.laps)
        assertEquals(8f / 30f, past.part, 1e-4f)
        assertNull("no value is an empty ring", RingRules.sweep(null, 100.0))
        assertEquals(RingRules.Sweep(0, 0f), RingRules.sweep(-5.0, 100.0))
    }

    @Test
    fun concentric_rings_keep_room_in_the_middle_and_a_gap_between_them() {
        val diameter = RingsRules.diameterDp(Navree.Prominence.MEDIUM, 3, 1f)
        val stroke = RingsRules.strokeDp(diameter, 3)
        val innermost = RingsRules.ringDiameterDp(diameter, 2, stroke)
        assertTrue("the innermost ring leaves half the width free", innermost - stroke >= diameter * 0.5f)
        assertEquals(stroke * 0.25f, RingsRules.gapDp(stroke), 1e-4f)
        assertTrue(RingsRules.ringDiameterDp(diameter, 0, stroke) > RingsRules.ringDiameterDp(diameter, 1, stroke))
        assertEquals(6, RingsRules.MOST)
    }
}

class ZoneRulesTest {
    private val zones = listOf(
        Navree.Zone(from = 0.0, to = 0.8, label = "Low"),
        Navree.Zone(from = 0.8, to = 1.3, label = "Optimal"),
        Navree.Zone(from = 1.3, to = 2.0, label = "High"),
    )

    @Test
    fun a_value_is_in_the_last_zone_that_holds_it() {
        assertEquals("Optimal", ZoneRules.zoneOf(1.12, zones)?.label)
        assertEquals("on the edge: the later zone", "High", ZoneRules.zoneOf(1.3, zones)?.label)
        assertNull(ZoneRules.zoneOf(2.5, zones))
        assertNull(ZoneRules.zoneOf(null, zones))
    }

    @Test
    fun a_value_outside_the_scale_sits_at_the_nearer_end() {
        assertEquals(0.5f, ZoneRules.position(80.0, 40.0, 120.0), 1e-4f)
        assertEquals(0f, ZoneRules.position(20.0, 40.0, 120.0), 0f)
        assertEquals(1f, ZoneRules.position(200.0, 40.0, 120.0), 0f)
    }
}

class ClockRulesTest {
    @Test
    fun midnight_is_at_the_top_and_the_day_goes_round_once() {
        assertEquals(-90f, ClockRules.angle(0.0), 1e-4f)
        assertEquals(0f, ClockRules.angle(6.0), 1e-4f)
        assertEquals(90f, ClockRules.angle(12.0), 1e-4f)
        assertEquals("a quarter to seven", 11.25f, ClockRules.angle(6.75), 1e-4f)
    }

    @Test
    fun an_hours_mark_reaches_by_its_level_else_its_value_against_the_busiest() {
        assertEquals(0.9f, ClockRules.reach(Navree.Hour(value = 1850.0, level = 0.9), 1850.0)!!, 1e-4f)
        assertEquals(0.5f, ClockRules.reach(Navree.Hour(value = 925.0), 1850.0)!!, 1e-4f)
        assertNull("no reading is not zero", ClockRules.reach(Navree.Hour(), 1850.0))
        val clock = Navree.DayClock(hours = listOf(Navree.Hour(value = 10.0), Navree.Hour(value = 40.0), Navree.Hour()))
        assertEquals(40.0, ClockRules.most(clock)!!, 0.0)
        assertEquals(100.0, ClockRules.most(clock.copy(max = 100.0))!!, 0.0)
    }
}

class ShareRulesTest {
    @Test
    fun parts_are_shares_of_their_whole_and_a_missing_amount_is_none() {
        assertEquals(listOf(0.25f, 0.75f, 0f), ShareRules.shares(listOf(1.0, 3.0, null)))
        assertEquals(listOf(0f, 0f), ShareRules.shares(listOf(0.0, null)))
    }

    @Test
    fun a_share_says_its_percentage_inside_only_where_it_fits() {
        assertTrue(ShareRules.saysInside(0.4f, 320f, 1f))
        assertFalse(ShareRules.saysInside(0.05f, 320f, 1f))
        assertFalse("larger text needs more room", ShareRules.saysInside(0.2f, 320f, 2f))
        assertEquals("a part that has some is never 0%", 1, ShareRules.percent(0.001f))
        assertEquals(0, ShareRules.percent(0f))
    }

    @Test
    fun a_strips_key_gives_each_categorys_share_of_the_cells_with_one() {
        val cells = listOf(0L, 1L, 1L, null, 2L).map { Navree.StripCell(label = "x", category = it) }
        assertEquals(listOf(0.25f, 0.5f, 0.25f), ShareRules.categoryShares(cells, 3))
    }
}

class HeatRulesTest {
    @Test
    fun a_day_with_no_value_is_no_data_not_the_faintest_shade() {
        assertNull(HeatRules.shade(null, listOf(10.0, 20.0, 30.0), 40.0))
        assertEquals(0, HeatRules.shade(0.0, listOf(10.0, 20.0, 30.0), 40.0))
        assertEquals(2, HeatRules.shade(25.0, listOf(10.0, 20.0, 30.0), 40.0))
        assertEquals(3, HeatRules.shade(30.0, listOf(10.0, 20.0, 30.0), 40.0))
        assertEquals("without steps, four equal parts to the highest", 1, HeatRules.shade(10.0, emptyList(), 40.0))
    }

    @Test
    fun weeks_start_on_the_persons_first_weekday() {
        val first = LocalDate.parse("2026-09-05") // a Saturday
        assertEquals(LocalDate.parse("2026-08-31"), HeatRules.start(first, DayOfWeek.MONDAY))
        assertEquals(LocalDate.parse("2026-08-30"), HeatRules.start(first, DayOfWeek.SUNDAY))
        val start = HeatRules.start(first, DayOfWeek.MONDAY)
        assertEquals(0 to 5, HeatRules.cell(first, start))
        assertEquals(11, HeatRules.weeks(first, LocalDate.parse("2026-11-13"), DayOfWeek.MONDAY))
    }
}

class MatrixRulesTest {
    private val relations = listOf(Navree.Relation(a = 0, b = 1, value = 0.62), Navree.Relation(a = 2, b = 0, value = -0.4, significant = false), Navree.Relation(a = 1, b = 2))

    @Test
    fun a_pair_is_the_same_either_way_round() {
        assertEquals(0.62, MatrixRules.relation(relations, 1, 0)!!.value!!, 0.0)
        assertNull(MatrixRules.relation(relations, 3, 0))
    }

    @Test
    fun only_the_lower_half_is_drawn_when_the_page_says_so() {
        assertTrue(MatrixRules.drawn(2, 0, lower = true))
        assertFalse(MatrixRules.drawn(0, 2, lower = true))
        assertFalse("never a measure against itself", MatrixRules.drawn(1, 1, lower = false))
    }

    @Test
    fun chance_and_untested_pairs_have_no_strength() {
        assertEquals(0.62f, MatrixRules.strength(relations[0], 1.0), 1e-4f)
        assertEquals(0f, MatrixRules.strength(relations[1], 1.0), 0f)
        assertEquals(0f, MatrixRules.strength(relations[2], 1.0), 0f)
        assertEquals("against the peak", 1f, MatrixRules.strength(relations[0], 0.5), 0f)
    }
}

class AxisRulesTest {
    @Test
    fun bars_stand_on_zero_and_the_top_leaves_room_over_the_highest_mark() {
        val chart = Navree.Chart(kind = Navree.ChartKind.BAR, labels = listOf("Mon", "Tue"), series = listOf(Navree.Series(label = "Hours", values = listOf(6.2, 8.1))), references = listOf(Navree.Reference(value = 9.0)))
        val bounds = AxisRules.values(chart)
        assertEquals(0.0, bounds.low, 0.0)
        assertTrue("the reference is inside", bounds.high > 9.0)
    }

    @Test
    fun ends_the_page_did_not_give_are_round_numbers() {
        assertEquals(AxisRules.Bounds(200.0, 500.0), AxisRules.nice(267.3, 477.6))
        assertEquals(AxisRules.Bounds(0.0, 10.0), AxisRules.nice(0.0, 9.72))
        assertEquals(AxisRules.Bounds(650.0, 900.0), AxisRules.nice(664.6, 858.4))
    }

    @Test
    fun the_measures_own_bounds_win() {
        val chart = Navree.Chart(series = listOf(Navree.Series(label = "Load", values = listOf(310.0, 462.0))), y = Navree.Measure(min = 200.0, max = 500.0))
        assertEquals(AxisRules.Bounds(200.0, 500.0), AxisRules.values(chart))
    }

    @Test
    fun stacked_values_add_up_and_shares_fill_the_whole() {
        val series = listOf(Navree.Series(label = "Light", values = listOf(30.0)), Navree.Series(label = "Hard", values = listOf(10.0)))
        val values = Navree.Chart(kind = Navree.ChartKind.BAR, stacking = Navree.Stacking.VALUES, labels = listOf("Mon"), series = series)
        assertEquals(listOf(0.0 to 30.0, 30.0 to 40.0), AxisRules.stack(values, 0))
        assertTrue(AxisRules.values(values).high >= 40.0)
        val shares = values.copy(stacking = Navree.Stacking.SHARES)
        assertEquals(listOf(0.0 to 0.75, 0.75 to 1.0), AxisRules.stack(shares, 0))
        assertEquals(AxisRules.Bounds(0.0, 1.0), AxisRules.values(shares))
    }

    @Test
    fun a_chart_of_stages_has_an_axis_of_its_levels() {
        val chart = Navree.Chart(levels = listOf("Deep", "Core", "REM", "Awake"), series = listOf(Navree.Series(label = "Stage", values = listOf(3.0, 1.0, 0.0))))
        assertEquals(AxisRules.Bounds(0.0, 3.0), AxisRules.values(chart))
    }

    @Test
    fun equal_axes_share_one_range() {
        val plot = Navree.Scatter(
            equalAxes = true,
            series = listOf(Navree.PointSeries(name = "Rest", points = listOf(Navree.Point(x = 800.0, y = 900.0), Navree.Point(x = 1000.0, y = 700.0)))),
        )
        val (x, y) = AxisRules.scatter(plot)
        assertEquals(x, y)
        assertTrue(x.low < 700.0 && x.high > 1000.0)
    }
}

class PagerRulesTest {
    private fun corpus(name: String): Navree.Screen = Fixtures.screen(name)

    @Test
    fun a_pager_that_leads_the_screen_stands_in_the_app_bar() {
        val today = corpus("screen-today.json")
        val pager = PagerRules.inBar(today)!!
        assertSame((today.body as Navree.Stack).children.first(), pager)
        assertEquals("2026-11-13", pager.date)
        assertNull("a screen that does not start with one", PagerRules.inBar(corpus("screen-fitness.json")))
    }

    @Test
    fun only_the_days_there_are_may_be_chosen() {
        assertTrue(PagerRules.choosable(LocalDate.parse("2026-06-01"), "2026-01-02", "2026-11-13"))
        assertFalse(PagerRules.choosable(LocalDate.parse("2026-11-14"), "2026-01-02", "2026-11-13"))
        assertFalse(PagerRules.choosable(LocalDate.parse("2026-01-01"), "2026-01-02", null))
        assertEquals("2026-10-31", PagerRules.text(LocalDate.parse("2026-10-31")))
    }
}

class CardRulesTest {
    @Test
    fun cards_stand_side_by_side_only_where_there_is_room() {
        assertEquals(1, ContentRules.cardColumns(412))
        assertEquals(1, ContentRules.cardColumns(700))
        assertEquals(2, ContentRules.cardColumns(1000))
        assertEquals(3, ContentRules.cardColumns(1400))
        assertEquals("larger text keeps fewer", 1, ContentRules.cardColumns(1000, 2f))
    }
}
