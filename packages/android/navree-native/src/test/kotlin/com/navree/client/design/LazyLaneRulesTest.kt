package com.navree.client.design

import com.navree.client.Fixtures
import com.navree.protocol.Navree
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Test

class LazyLaneRulesTest {
    /** Six rows 100 px tall, one after another from 0, as a lazy list reports them. */
    private val rows = (0..5).map { LaneRules.Slot(it, it * 100, 100) }

    private fun others(held: Int, from: List<LaneRules.Slot> = rows) = from.filter { it.place != held }

    @Test
    fun a_held_row_stays_until_its_middle_passes_the_middle_of_a_neighbour() {
        // Row 2 stands at 200..300; its middle is 250. Row 3's middle is 350.
        assertEquals(2, LaneRules.landing(2, 200f, 100, others(2), 0, 600, 200))
        assertEquals("a finger moved 90 px: the middle is at 340, short of 350", 2, LaneRules.landing(2, 290f, 100, others(2), 0, 600, 200))
        assertEquals("100 px of the finger and the middle is at 351", 3, LaneRules.landing(2, 301f, 100, others(2), 0, 600, 200))
        assertEquals("up: the middle at 149 is short of row 1's 150", 1, LaneRules.landing(2, 99f, 100, others(2), 0, 600, 200))
        assertEquals(2, LaneRules.landing(2, 101f, 100, others(2), 0, 600, 200))
    }

    @Test
    fun a_row_held_far_down_passes_every_row_in_between_in_one_go() {
        assertEquals(5, LaneRules.landing(1, 520f, 100, others(1), 0, 600, 100))
        assertEquals(0, LaneRules.landing(4, -80f, 100, others(4), 0, 600, 400))
    }

    @Test
    fun a_row_that_has_just_swapped_does_not_swap_back() {
        // Row 2 passed row 3 (the rows then stand 0,1,3,2: row 3 at 200..300, the held at 300..400), the finger still at 301.
        val after = listOf(LaneRules.Slot(0, 0, 100), LaneRules.Slot(1, 100, 100), LaneRules.Slot(2, 200, 100), LaneRules.Slot(4, 400, 100))
        assertEquals(3, LaneRules.landing(3, 301f, 100, after, 0, 600, 300))
    }

    @Test
    fun rows_of_different_heights_are_passed_by_their_own_middles() {
        val tall = listOf(LaneRules.Slot(0, 0, 40), LaneRules.Slot(1, 40, 300), LaneRules.Slot(2, 340, 60))
        // Held row 0 (40 px): the tall row's middle is 190.
        assertEquals(0, LaneRules.landing(0, 150f, 40, tall.drop(1), 0, 400, 0))
        assertEquals(1, LaneRules.landing(0, 171f, 40, tall.drop(1), 0, 400, 0))
    }

    @Test
    fun a_row_held_below_the_window_passes_only_the_rows_that_can_be_seen() {
        // The window ends at 400, so rows 4 and 5 are not laid: held row 1 is dragged to 900 and goes as far as the last visible row.
        val seen = rows.take(4)
        assertEquals(3, LaneRules.landing(1, 900f, 100, others(1, seen), 0, 400, 100))
        // Nor does a gap in the rows that are laid let it jump over what it cannot see: row 2 is not laid, so row 3 is not passed.
        val gap = listOf(LaneRules.Slot(1, 100, 100), LaneRules.Slot(3, 300, 100))
        assertEquals(1, LaneRules.landing(0, 900f, 100, gap, 0, 400, 0))
    }

    @Test
    fun a_row_held_above_the_window_is_taken_as_at_its_top_edge() {
        // The window starts at 100: a row held at -300 goes up past the rows that can be seen, and no further.
        val seen = rows.drop(1)
        assertEquals(1, LaneRules.landing(3, -300f, 100, others(3, seen), 100, 600, 300))
    }

    @Test
    fun a_row_does_not_pass_a_neighbour_when_the_place_it_would_stand_in_is_not_wholly_in_the_window() {
        // Row 2 stands at 200..300; passing row 3 would put it at 300..400, and the window ends at 380.
        assertEquals(2, LaneRules.landing(2, 450f, 100, others(2), 0, 380, 200))
        // With a window to 400 it would stand wholly inside: it passes.
        assertEquals(3, LaneRules.landing(2, 450f, 100, others(2), 0, 400, 200))
        // Up: row 2 passing row 1 would stand at 100..200; the window starts at 120.
        assertEquals(2, LaneRules.landing(2, -50f, 100, others(2), 120, 600, 200))
        // Rows passed on the way count: row 1 would reach 400..500 passing row 4, beyond a window that ends at 480, so it goes as far as row 3.
        assertEquals(3, LaneRules.landing(1, 600f, 100, others(1), 0, 480, 100))
    }

    @Test
    fun a_row_whose_own_place_is_not_reported_passes_nothing() {
        assertEquals(2, LaneRules.landing(2, 450f, 100, others(2), 0, 600, null))
    }

    @Test
    fun nothing_moves_without_rows_to_pass_or_a_height() {
        assertEquals(2, LaneRules.landing(2, 500f, 100, emptyList(), 0, 600, 200))
        assertEquals(2, LaneRules.landing(2, 500f, 0, others(2), 0, 600, 200))
    }

    @Test
    fun the_list_scrolls_by_itself_only_near_an_edge_and_only_the_way_the_finger_went() {
        val edge = 72f
        val most = 20f
        // The window is 0 to 800, the row 100 tall.
        assertEquals("clear of both edges", 0f, LaneRules.scrollBy(300f, 100, 0, 800, edge, most, 1f), 0f)
        assertEquals("a row's bottom at the start of the edge's 72 px", 0f, LaneRules.scrollBy(628f, 100, 0, 800, edge, most, 1f), 0f)
        assertEquals("half way into the bottom edge", 10f, LaneRules.scrollBy(664f, 100, 0, 800, edge, most, 1f), 0.001f)
        assertEquals("at the edge or past it: the most", 20f, LaneRules.scrollBy(760f, 100, 0, 800, edge, most, 1f), 0f)
        assertEquals("half way into the top edge", -10f, LaneRules.scrollBy(36f, 100, 0, 800, edge, most, -1f), 0.001f)
        assertEquals("above the window: the most", -20f, LaneRules.scrollBy(-90f, 100, 0, 800, edge, most, -1f), 0f)
        assertEquals("a row picked up beside the top edge does not scroll before the finger went up", 0f, LaneRules.scrollBy(10f, 100, 0, 800, edge, most, 0f), 0f)
        assertEquals("nor when the finger went down", 0f, LaneRules.scrollBy(10f, 100, 0, 800, edge, most, 1f), 0f)
        assertEquals("a window that starts above 0 (a padding that is drawn in) counts from where it starts", -20f, LaneRules.scrollBy(-60f, 100, -8, 800, edge, most, -1f), 0f)
    }

    @Test
    fun the_end_of_a_lane_asks_for_more_only_when_it_has_more_and_a_name_to_ask_with() {
        assertEquals(LaneRules.End.Nothing, LaneRules.end(false, "todo", false))
        assertEquals(LaneRules.End.Nothing, LaneRules.end(true, null, false))
        assertEquals(LaneRules.End.Asks, LaneRules.end(true, "todo", false))
        assertEquals("the last ask failed: the end says so, and asks again when it is pressed", LaneRules.End.Retry, LaneRules.end(true, "todo", true))
        assertEquals("a lane that holds everything has nothing to retry", LaneRules.End.Nothing, LaneRules.end(false, "todo", true))
    }

    @Test
    fun keys_name_a_row_of_a_list_wherever_it_stands() {
        val key = LaneRules.key("board-todo", "c:7")
        assertEquals("c:7", LaneRules.idOf("board-todo", key))
        assertNull("another list's row", LaneRules.idOf("board-doing", key))
        assertNull("not a row's key", LaneRules.idOf("board-todo", "id:c-7"))
        assertNull(LaneRules.idOf("board-todo", 3))
        assertTrue(LaneRules.key("a", "b") != LaneRules.key("a:b", ""))
    }

    private fun card(id: String?) = Navree.Row(id = id, title = "Card $id")

    @Test
    fun the_rows_that_are_moved_are_the_first_of_each_id_in_a_list_that_takes_moves() {
        val reorder = Navree.Reorder(href = "/cards/move", group = "board")
        val list = Navree.List(
            id = "todo",
            reorder = reorder,
            children = listOf(card("a"), Navree.Text(value = "between"), card(null), card("b"), card("a"), card("c")),
        )
        assertEquals(listOf("a", "b", "c"), LaneRules.movable(list))
        assertEquals("a list that takes no moves moves none", emptyList<String>(), LaneRules.movable(list.copy(reorder = null)))
        assertEquals("a list with no id cannot be told about", emptyList<String>(), LaneRules.movable(list.copy(id = null)))
    }

    @Test
    fun a_move_is_a_move_of_one_id_and_leaves_the_others_in_their_order() {
        val order = listOf("a", "b", "c", "d")
        assertEquals(listOf("b", "c", "a", "d"), LaneRules.moved(order, 0, 2))
        assertEquals(listOf("d", "a", "b", "c"), LaneRules.moved(order, 3, 0))
        assertSame("nowhere", order, LaneRules.moved(order, 1, 1))
        assertSame("from a place there is not", order, LaneRules.moved(order, 9, 0))
        assertEquals("c", LaneRules.before(listOf("a", "b", "c"), "b"))
        assertNull("the last stands before none", LaneRules.before(listOf("a", "b", "c"), "c"))
        assertNull("a row that is not there", LaneRules.before(listOf("a", "b", "c"), "z"))
    }

    @Test
    fun the_rows_of_a_lane_take_the_order_in_the_places_rows_stood_and_what_is_between_keeps_its_place() {
        val items = listOf("a", "-", "b", "c", "-", "d")
        val rowsOnly: (String) -> String? = { if (it == "-") null else it }
        assertEquals(listOf("c", "-", "a", "d", "-", "b"), LaneRules.arranged(items, rowsOnly, listOf("c", "a", "d", "b")))
        assertSame("no order: as it is", items, LaneRules.arranged(items, rowsOnly, null))
        assertSame("an order that is missing a row is not used", items, LaneRules.arranged(items, rowsOnly, listOf("a", "b", "c")))
        assertSame("nor one that names a row that is not there", items, LaneRules.arranged(items, rowsOnly, listOf("a", "b", "c", "z")))
    }

    @Test
    fun a_window_stays_where_it_is_when_the_card_at_its_first_place_moves_and_not_for_anything_else() {
        val first = LaneRules.key("todo", "a")
        val next = LaneRules.key("todo", "b")
        assertTrue("the first card moved down: another stands at its place", LaneRules.windowStays(first, next) { true })
        assertTrue("the same card at the place: nothing moved", !LaneRules.windowStays(first, first) { true })
        assertTrue("the card was taken away, not moved", !LaneRules.windowStays(first, next) { false })
        assertTrue("content that is not a card is not held", !LaneRules.windowStays("plain:0:", next) { true })
        assertTrue("no item stood there", !LaneRules.windowStays(null, next) { true })
        assertTrue("a list that ends before the place", LaneRules.windowStays(first, null) { true })
    }

    @Test
    fun the_boards_lanes_are_shown_one_at_a_time_where_the_window_holds_one_column_of_them() {
        val board = Fixtures.screen("screen-board.json").body.walk().filterIsInstance<Navree.Grid>().first { ListRules.areLanes(it.children) }
        val parts = board.children
        assertTrue("a phone", LaneRules.oneAtATime(board, parts, 412, 16, 1f))
        assertTrue("a phone with the largest text", LaneRules.oneAtATime(board, parts, 412, 16, 2f))
        assertTrue("a wide window", !LaneRules.oneAtATime(board, parts, 1280, 24, 1f))
        assertTrue("three lanes fit side by side", !LaneRules.oneAtATime(board, parts, 800, 24, 1f))
        assertTrue("what is not lanes is no board", !LaneRules.oneAtATime(Navree.Grid(children = listOf(Navree.Text(value = "a"), Navree.Text(value = "b"))), listOf(Navree.Text(value = "a"), Navree.Text(value = "b")), 412, 16, 1f))
    }
}
