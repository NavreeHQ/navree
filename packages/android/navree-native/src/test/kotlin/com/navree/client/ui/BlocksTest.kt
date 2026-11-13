package com.navree.client.ui

import com.navree.client.Fixtures
import com.navree.client.core.Content
import com.navree.client.core.RegionView
import com.navree.client.core.ScreenView
import com.navree.protocol.Navree
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

private fun view(screen: Navree.Screen, states: Map<String, List<Navree.State>> = emptyMap(), regions: Map<String, RegionView> = emptyMap()) =
    ScreenView(id = 1, url = "/orders", content = Content.Native(screen), states = states, regions = regions)

class BlocksTest {
    private val orders = Fixtures.screen("store-demo/orders.json")

    @Test
    fun cards_side_by_side_on_a_wide_window_stand_in_a_wider_column_and_a_card_alone_spans_it() {
        val designed = Fixtures.screen("screen-designed-health.json")
        val wide = blocks(designed.body, view(designed), widthDp = 1060)
        // The score card has no partner, so it spans the line; the day and the activity share one.
        assertEquals(listOf(1, 2), wide.filterIsInstance<Block.Cards>().map { it.columns })
        assertTrue(cardsSideBySide(wide))
        val phone = blocks(designed.body, view(designed), widthDp = 412)
        assertTrue("one column on a phone: no line of cards", phone.none { it is Block.Cards })
        assertFalse(cardsSideBySide(phone))
    }

    @Test
    fun a_stack_that_turns_along_the_line_is_one_block_and_widens_the_column() {
        val lead = Navree.Stack(
            id = "lead",
            wide = Navree.Axis.HORIZONTAL,
            children = listOf(
                Navree.Stack(align = Navree.Align.CENTER, children = listOf(Navree.Ring(max = 100.0, value = 79.0, prominence = Navree.Prominence.HIGH))),
                Navree.Stack(grow = true, children = listOf(Navree.Text(value = "words"))),
            ),
        )
        val body = Navree.Stack(children = listOf(Navree.Text(value = "Hello"), lead, Navree.Text(value = "end")))
        val flat = blocks(body, view(Navree.Screen(body = body, title = "Home")), widthDp = 1060)
        val single = flat.filterIsInstance<Block.Single>().map { it.node }
        assertTrue("the lead is drawn whole, so its centred ring stays centred and it can turn", single.any { it === lead })
        assertTrue(single.none { it is Navree.Ring })
        assertTrue(cardsSideBySide(flat))
        assertFalse(cardsSideBySide(blocks(Navree.Stack(children = listOf(Navree.Text(value = "a"))), view(Navree.Screen(title = "x")), widthDp = 1060)))
    }

    @Test
    fun a_line_can_be_made_as_tall_as_its_tallest_unless_a_part_measures_while_it_lays_out() {
        assertTrue("a grid knows its width from the room it is given", measuresAhead(Navree.Card(children = listOf(Navree.Grid(children = listOf(Navree.Text(value = "a")))))))
        assertTrue(measuresAhead(Navree.Card(children = listOf(Navree.Meter()))))
        assertTrue(measuresAhead(Navree.Card(children = listOf(Navree.Stack(wide = Navree.Axis.HORIZONTAL)))))
        assertFalse(measuresAhead(Navree.Card(children = listOf(Navree.Carousel()))))
        assertFalse(measuresAhead(Navree.Card(children = listOf(Navree.Tabs()))))
    }

    @Test
    fun the_real_orders_table_is_its_filters_then_a_row_a_record_then_the_next_page() {
        val flat = blocks(orders.body, view(orders), widthDp = 412)
        assertTrue(flat.first { it !is Block.Space } is Block.Filters)
        val rows = flat.filterIsInstance<Block.TableItem>()
        assertTrue("one block a row, so only the rows on screen are drawn", rows.size >= 10)
        val more = flat.filterIsInstance<Block.More>()
        assertEquals("the next rows are asked for by the table's region", listOf("orders"), more.map { it.id })
    }

    @Test
    fun every_block_has_a_key_of_its_own() {
        val flat = blocks(orders.body, view(orders), widthDp = 412)
        assertEquals(flat.size, flat.map { it.key }.toSet().size)
    }

    @Test
    fun a_table_is_one_block_where_there_is_room_for_its_columns() {
        val flat = blocks(orders.body, view(orders), widthDp = 1600)
        assertTrue(flat.none { it is Block.TableItem })
        assertTrue(flat.any { it is Block.Single && it.node is Navree.Table })
    }

    @Test
    fun an_inset_section_groups_its_rows_into_one_rounded_block() {
        val section = Navree.Section(style = Navree.SectionStyle.INSET, title = "Shipping", children = listOf(Navree.Row(title = "Amritsar"), Navree.Row(title = "Jalandhar"), Navree.Row(title = "Ludhiana")))
        val flat = blocks(section, view(Navree.Screen(body = section, title = "Order")), widthDp = 412)
        assertTrue(flat.first() is Block.Header)
        assertEquals(listOf(Group.First, Group.Middle, Group.Last), flat.filterIsInstance<Block.Item>().map { it.group })
    }

    @Test
    fun a_forms_button_is_not_drawn_twice_when_the_bar_holds_it() {
        val save = Navree.Button(label = "Save", role = Navree.Role.PRIMARY, action = Navree.Action.Submit())
        val form = Navree.Form(id = "customer", children = listOf(Navree.Field(id = "customer.name", name = "name", label = "Name", input = Navree.TextInput()), save))
        val screen = Navree.Screen(body = form, title = "New customer")
        assertTrue(blocks(form, view(screen), 412).any { it is Block.Single && it.node === save })
        assertFalse(blocks(form, view(screen), 412, omit = save).any { it is Block.Single && it.node === save })
        assertEquals("the fields know their form", "customer", blocks(form, view(screen), 412).filterIsInstance<Block.Single>().first().form)
    }

    @Test
    fun what_the_core_hid_is_not_in_the_flow() {
        val body = Navree.Stack(children = listOf(Navree.Text(id = "note", value = "Packed"), Navree.Text(value = "Shipped")))
        val screen = Navree.Screen(body = body, title = "Order")
        val shown = blocks(body, view(screen, states = mapOf("note" to listOf(Navree.State.HIDDEN))), 412)
        assertEquals(listOf("Shipped"), shown.filterIsInstance<Block.Single>().map { (it.node as Navree.Text).value })
    }

    @Test
    fun a_region_still_on_its_way_holds_its_place() {
        val region = Navree.Region(id = "figures", src = "/fragments/figures", lazy = true)
        val flat = blocks(region, view(Navree.Screen(body = region, title = "Dashboard"), regions = mapOf("figures" to RegionView(loading = true))), 412)
        assertEquals(listOf(Block.Waiting("b/waiting", "figures")), flat)
    }

    @Test
    fun an_empty_state_that_is_the_whole_screen_fills_it() {
        val empty = Navree.Empty(title = "No orders yet")
        assertTrue(blocks(empty, view(Navree.Screen(body = empty, title = "Orders")), 412).single() is Block.Fill)
    }

    @Test
    fun a_toolbar_floats_over_the_content_and_is_not_in_it() {
        val bar = Navree.Toolbar(items = listOf(Navree.ToolbarItem(icon = Navree.Icon(name = "share-2"), title = "Share", standard = Navree.Standard.SHARE)))
        val body = Navree.Stack(children = listOf(Navree.Text(value = "Order CH-1042"), bar))
        val screen = Navree.Screen(body = body, title = "Order")
        assertTrue(blocks(body, view(screen), 412).none { it is Block.Single && it.node is Navree.Toolbar })
        assertEquals(listOf(bar), toolbars(body, view(screen)))
    }

    @Test
    fun the_block_that_holds_a_node_is_found_for_focus() {
        val form = Navree.Form(id = "f", children = listOf(Navree.Field(id = "f.phone", name = "phone", input = Navree.TextInput())))
        val flat = blocks(form, view(Navree.Screen(body = form, title = "Edit")), 412)
        assertTrue(blockOf(flat, "f.phone") >= 0)
        assertEquals(-1, blockOf(flat, "f.missing"))
    }
}

class EchoTest {
    @Test
    fun late_echoes_pass_and_outside_changes_are_seen() {
        val echo = Echo()
        // Someone types "a" then "ab"; the state says "a" for a moment before it catches up.
        echo.report("a")
        echo.report("ab")
        assertFalse("the state catching up with the first letter is an echo", echo.changedOutside(current = "a", shown = "ab"))
        assertFalse(echo.changedOutside(current = "ab", shown = "ab"))
        assertTrue("the server sending the form again is a change", echo.changedOutside(current = "Harpreet Kaur", shown = "ab"))
        assertFalse(echo.changedOutside(current = "same", shown = "same"))
    }
}
