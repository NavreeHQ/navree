package com.navree.client.ui

import com.navree.client.Fixtures
import com.navree.client.core.Content
import com.navree.client.core.ScreenView
import com.navree.protocol.Navree
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

private fun viewOf(screen: Navree.Screen) = ScreenView(id = 1, url = "/board", content = Content.Native(screen))

/** What a screen's rows that are moved become when its body is flattened into the items of its own list. */
class LaneBlocksTest {
    private val board = Fixtures.screen("screen-board.json")
    private val reorder = Navree.Reorder(href = "/cards/move", group = "board")

    private fun card(id: String?) = Navree.Row(id = id, title = "Card $id")

    private fun checklist(more: String? = null, vararg rows: Navree.Node) =
        Navree.List(id = "todo", more = more, reorder = reorder, children = rows.toList())

    private fun blocksOf(list: Navree.List, lanes: LaneLayout? = LaneLayout(16)): List<Block> {
        val screen = Navree.Screen(title = "Todo", body = Navree.Stack(children = listOf(list)))
        return blocks(screen.body, viewOf(screen), widthDp = 412, lanes = lanes)
    }

    @Test
    fun the_rows_of_a_list_that_takes_moves_are_items_of_the_screens_list_with_a_key_of_their_own() {
        val list = checklist(null, card("a"), card("b"), card("c"))
        val moving = blocksOf(list).filterIsInstance<Block.Moving>()
        assertEquals(listOf("a", "b", "c"), moving.map { it.row.id })
        assertEquals("the keys are those of the lane's rows, wherever the rows stand", moving.map { com.navree.client.design.LaneRules.key("todo", it.row.id!!) }, moving.map { it.key })
        assertEquals("no key is used twice", moving.size, moving.map { it.key }.toSet().size)
    }

    @Test
    fun a_list_is_drawn_whole_where_the_caller_has_no_lanes_to_lay() {
        val blocks = blocksOf(checklist(null, card("a"), card("b")), lanes = null)
        assertTrue(blocks.none { it is Block.Moving })
        assertTrue("one block, the list", blocks.single() is Block.Single)
    }

    @Test
    fun a_row_with_no_id_or_an_id_an_earlier_row_has_stays_in_its_place_with_no_handle() {
        val between = Navree.Text(value = "between")
        val blocks = blocksOf(checklist(null, card("a"), card(null), between, card("a"), card("b")))
        // The rows that cannot be moved are items, the text is drawn as itself, and only the first "a" and "b" are moved.
        assertEquals(listOf("a", "b"), blocks.filterIsInstance<Block.Moving>().map { it.row.id })
        assertEquals(2, blocks.filterIsInstance<Block.Item>().size)
        assertTrue(blocks.any { it is Block.Single && it.node === between })
    }

    @Test
    fun the_end_of_a_list_that_has_more_asks_for_them_and_one_that_holds_all_does_not() {
        val more = blocksOf(checklist("/cards?after=c", card("a"), card("b")))
        assertTrue("the last block is the end of the list, which names the list", (more.last() as Block.More).id == "todo")
        assertEquals("after the rows, not among them", listOf(true, true, false), more.drop(0).map { it is Block.Moving })
        assertTrue(blocksOf(checklist(null, card("a"), card("b"))).none { it is Block.More })
    }

    @Test
    fun a_phone_shows_the_lane_in_hand_as_part_of_the_screens_list_under_the_tabs() {
        val phone = blocks(board.body, viewOf(board), widthDp = 412, lanes = LaneLayout(16))
        val tabs = phone.filterIsInstance<Block.LaneTabs>().single()
        assertEquals(listOf("To do", "Doing", "Done"), tabs.lanes.map { it.title })
        assertEquals(0, tabs.chosen)
        assertEquals("the first lane's cards", listOf("c-7", "c-8"), phone.filterIsInstance<Block.Moving>().map { it.row.id })
        assertTrue("the lanes are not one block that is drawn whole", phone.none { it is Block.Single && it.node is Navree.Grid })
        // The tabs come before the cards.
        assertTrue(phone.indexOf(tabs) < phone.indexOfFirst { it is Block.Moving })
    }

    @Test
    fun the_lane_that_is_chosen_is_the_one_whose_cards_follow_the_tabs() {
        val gridId = blocks(board.body, viewOf(board), widthDp = 412, lanes = LaneLayout(16)).filterIsInstance<Block.LaneTabs>().single().id
        val doing = blocks(board.body, viewOf(board), widthDp = 412, lanes = LaneLayout(16, mapOf(gridId to 1)))
        assertEquals(1, doing.filterIsInstance<Block.LaneTabs>().single().chosen)
        assertEquals(listOf("c-5"), doing.filterIsInstance<Block.Moving>().map { it.row.id })
        val outOfRange = blocks(board.body, viewOf(board), widthDp = 412, lanes = LaneLayout(16, mapOf(gridId to 9)))
        assertEquals("a lane that is not there is the last", listOf("c-2"), outOfRange.filterIsInstance<Block.Moving>().map { it.row.id })
    }

    @Test
    fun a_wide_window_keeps_its_lanes_side_by_side_in_one_block_for_the_lanes_to_lay_themselves_in() {
        val wide = blocks(board.body, viewOf(board), widthDp = 1280, lanes = LaneLayout(24))
        assertTrue(wide.none { it is Block.LaneTabs || it is Block.Moving })
        assertTrue(wide.any { it is Block.Single && it.node is Navree.Grid })
    }
}
