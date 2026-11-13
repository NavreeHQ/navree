package com.navree.client.design

import com.navree.protocol.Navree
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

private fun text(value: String) = Navree.Text(value = value)

private fun dispatch(vararg rows: Navree.TableRow, across: Navree.Across? = null) = Navree.Table(
    id = "dispatch",
    select = Navree.Selection(name = "ids[]", form = "bulk", across = across),
    columns = listOf(Navree.Column(label = "Order")),
    rows = rows.toList(),
)

private fun order(id: String?, selected: Boolean = false, unchoosable: Boolean = false) =
    Navree.TableRow(id = id, cells = listOf(text("CS-$id")), selected = selected, unchoosable = unchoosable)

class SelectionRulesTest {
    private val bulk = Navree.Form(
        id = "bulk",
        children = listOf(
            Navree.Button(label = "Mark packed", action = Navree.Action.Submit()),
            Navree.Button(label = "Cancel orders", role = Navree.Role.DESTRUCTIVE, action = Navree.Action.Submit()),
        ),
    )

    @Test
    fun a_row_has_a_working_box_only_with_an_id_in_a_table_that_can_be_chosen_from() {
        val table = dispatch(order("1041"), order(null), order("1043", unchoosable = true))
        assertTrue(SelectionRules.choosable(table, table.rows[0]))
        assertFalse("no id, no box", SelectionRules.choosable(table, table.rows[1]))
        assertFalse("the server left it out", SelectionRules.choosable(table, table.rows[2]))
        assertFalse("a table that cannot be chosen from", SelectionRules.choosable(table.copy(select = null), table.rows[0]))
        assertFalse("a table with no id cannot be told about", SelectionRules.choosable(table.copy(id = null), table.rows[0]))
    }

    @Test
    fun the_chosen_rows_are_the_ticked_ones_or_every_one_while_all_are_chosen() {
        val table = dispatch(order("1041", selected = true), order("1042"), order("1043", unchoosable = true))
        assertEquals(listOf("1041"), SelectionRules.chosen(table, across = false))
        assertEquals("across ticks every row that can be chosen", listOf("1041", "1042"), SelectionRules.chosen(table, across = true))
        assertFalse(SelectionRules.ticked(table, table.rows[2], across = true))
    }

    @Test
    fun choosing_every_row_is_offered_once_every_row_held_is_chosen() {
        val across = Navree.Across(name = "all", value = "1", total = 1204)
        val some = dispatch(order("1041", selected = true), order("1042"), across = across)
        val all = dispatch(order("1041", selected = true), order("1042", selected = true), order("1043", unchoosable = true), across = across)
        assertFalse(SelectionRules.offerAcross(some, across = false))
        assertTrue(SelectionRules.offerAcross(all, across = false))
        assertFalse("already on", SelectionRules.offerAcross(all, across = true))
        assertFalse("the page offers no such choice", SelectionRules.offerAcross(all.copy(select = all.select!!.copy(across = null)), across = false))
    }

    @Test
    fun the_form_of_buttons_the_rows_go_with_is_the_contextual_bars() {
        val table = dispatch(order("1041"))
        val body = Navree.Stack(children = listOf(bulk, table))
        assertEquals(table, SelectionRules.selectable(body))
        assertEquals(bulk, SelectionRules.bulkForm(body, table))
        val withField = bulk.copy(children = bulk.children + Navree.Field(name = "note", input = Navree.TextInput()))
        assertNull("a form with fields stays where it is", SelectionRules.bulkForm(Navree.Stack(children = listOf(withField, table)), table))
    }

    @Test
    fun a_tap_ticks_while_rows_are_chosen_and_opens_otherwise() {
        val table = dispatch(order("1041"), order("1042"))
        assertFalse(SelectionRules.tapTicks(table, table.rows[0], anyChosen = false))
        assertTrue(SelectionRules.tapTicks(table, table.rows[0], anyChosen = true))
    }
}

class ReorderRulesTest {
    private fun card(id: String) = Navree.Row(id = id, title = "Card $id")

    private fun list(id: String, group: String?, limit: Long? = null, vararg rows: String) =
        Navree.List(id = id, children = rows.map(::card), reorder = Navree.Reorder(href = "/cards/move", group = group, limit = limit))

    @Test
    fun a_row_moves_when_its_list_takes_moves() {
        val todo = list("todo", "board", null, "c-7")
        assertTrue(ReorderRules.draggable(todo, todo.children[0] as Navree.Row))
        assertFalse(ReorderRules.draggable(todo.copy(reorder = null), todo.children[0] as Navree.Row))
        assertFalse(ReorderRules.draggable(todo, Navree.Row(title = "No id")))
    }

    @Test
    fun half_a_row_dragged_counts_as_a_place() {
        assertEquals(0, ReorderRules.steps(20f, 100))
        assertEquals(1, ReorderRules.steps(60f, 100))
        assertEquals(-2, ReorderRules.steps(-180f, 100))
        assertEquals(0, ReorderRules.steps(500f, 0))
    }

    @Test
    fun a_moved_row_names_the_row_it_then_stands_before() {
        val ids = listOf("a", "b", "c", "d")
        assertEquals(ReorderRules.Landing(2, "d"), ReorderRules.landing(ids, 1, 1))
        assertEquals(ReorderRules.Landing(0, "a"), ReorderRules.landing(ids, 1, -1))
        assertEquals("past the last row: at the end", ReorderRules.Landing(3, null), ReorderRules.landing(ids, 1, 9))
        assertNull("not moved", ReorderRules.landing(ids, 1, 0))
        assertNull("the first row up stays", ReorderRules.landing(ids, 0, -1))
    }

    @Test
    fun a_row_goes_only_to_a_list_of_its_group_that_is_not_full() {
        val todo = list("todo", "board", null, "c-7", "c-8")
        val doing = list("doing", "board", 1, "c-5")
        val done = list("done", "board", null, "c-2")
        val other = list("elsewhere", "other", null)
        assertTrue(ReorderRules.accepts(todo, todo))
        assertTrue(ReorderRules.accepts(todo, done))
        assertFalse("full", ReorderRules.accepts(todo, doing))
        assertFalse("another group", ReorderRules.accepts(todo, other))
        assertFalse("no group: its own list only", ReorderRules.accepts(list("solo", null, null, "x"), done))
    }

    @Test
    fun the_lists_a_row_may_go_to_are_named_by_their_sections() {
        val todo = list("todo", "board", null, "c-7")
        val done = list("done", "board", null, "c-2")
        val body = Navree.Grid(
            children = listOf(
                Navree.Section(title = "To do", children = listOf(todo)),
                Navree.Section(title = "Done", children = listOf(done)),
            ),
        )
        assertEquals(listOf("Done"), ReorderRules.targets(body, todo).map { it.second })
    }
}
