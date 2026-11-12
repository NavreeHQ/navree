package com.navree.client.ui

import com.navree.client.Fixtures
import com.navree.client.protocol.ButtonNode
import com.navree.client.design.ScreenRules
import com.navree.client.protocol.FieldNode
import com.navree.client.protocol.FormNode
import com.navree.client.protocol.TextNode
import com.navree.client.runtime.submitButton
import com.navree.client.protocol.RowNode
import com.navree.client.protocol.Screen
import com.navree.client.protocol.TableNode
import com.navree.client.protocol.decodeEnvelope
import com.navree.client.runtime.Local
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class BlocksTest {
    private fun screen(name: String): Screen = decodeEnvelope(Fixtures.envelope(name)).screen!!

    private fun flat(name: String, widthDp: Int = 412, local: Local = Local()): List<Block> =
        blocks(screen(name).body, local, widthDp)

    @Test
    fun a_list_is_one_block_per_row_so_only_what_is_on_screen_is_drawn() {
        val blocks = flat("orders")
        val rows = blocks.filterIsInstance<Block.Item>()
        assertEquals(10, rows.size)
        assertTrue(rows.all { it.node is RowNode && it.group == Group.None })
        assertEquals("id:order-1042", rows.first().key)
        assertEquals(Block.More("b/0/more", "orders-list", "/orders?after=1033"), blocks.last())
        assertEquals("the sheet is not in the flow", 11, blocks.size)
    }

    @Test
    fun every_block_has_a_key_of_its_own() {
        for (name in listOf("orders", "order", "order-edit", "home", "products", "customers", "settings", "sign-in")) {
            val keys = flat(name).map { it.key }
            assertEquals("$name has no key twice", keys.size, keys.toSet().size)
        }
    }

    @Test
    fun an_inset_section_groups_its_rows_into_one_rounded_block() {
        val blocks = flat("settings")
        val store = blocks.dropWhile { it !is Block.Header }.drop(1).takeWhile { it is Block.Item }.map { (it as Block.Item).group }
        assertEquals(listOf(Group.First, Group.Middle, Group.Last), store)
        assertEquals(listOf("Store", "Notifications", "About"), blocks.filterIsInstance<Block.Header>().map { it.text })
        assertTrue(blocks.any { it is Block.Single && (it.node as? TextNode)?.value == "Order alerts are sent to +91 98765 43210." })
        val switches = blocks.filterIsInstance<Block.Item>().filter { it.node is FieldNode }
        assertEquals(3, switches.size)
        assertTrue("a switch knows the form it posts with", switches.all { it.form == "notify-form" })
    }

    @Test
    fun a_table_is_rows_on_a_phone_and_one_table_where_there_is_room() {
        assertEquals(7, flat("products", widthDp = 412).filterIsInstance<Block.TableItem>().size)
        val wide = flat("products", widthDp = 900)
        assertTrue((wide.single() as Block.Single).node is TableNode)
    }

    @Test
    fun a_forms_button_is_in_the_flow_only_when_the_bar_does_not_hold_it() {
        val form = screen("order-edit").body as FormNode
        val inline = blocks(form, Local(), 412)
        val last = inline.last() as Block.Single
        assertEquals("Save", (last.node as ButtonNode).label)
        assertEquals("order-form", last.form)
        assertEquals(listOf("Customer", "Delivery", "Payment", "Notes"), inline.filterIsInstance<Block.Header>().map { it.text })
        val withBar = blocks(form, Local(), 412, omit = form.submitButton())
        assertFalse(withBar.any { it is Block.Single && it.node is ButtonNode })
    }

    @Test
    fun the_real_orders_table_is_chips_for_its_filters_then_a_row_a_record_then_the_next_page() {
        val orders = decodeEnvelope(Fixtures.text("store-demo/orders.json")).screen!!
        val query = ScreenRules.query(orders)!!.form
        val blocks = blocks(orders.body, Local(), 412, hoisted = query)
        assertEquals(Block.Filters("b/0/0/filters", query, search = false), blocks.first())
        assertEquals(25, blocks.filterIsInstance<Block.TableItem>().size)
        val more = blocks.last() as Block.More
        assertEquals("the table goes by the id of its region", "orders", more.list)
    }

    @Test
    fun a_region_still_on_its_way_holds_its_place() {
        val waiting = com.navree.client.protocol.RegionNode(id = "steps", src = "/orders/1/steps", lazy = true)
        assertTrue(blocks(waiting, Local(), 412).single() is Block.Waiting)
    }

    @Test
    fun an_empty_state_that_is_the_whole_screen_fills_it() {
        assertTrue(flat("customers-empty").single() is Block.Fill)
    }

    @Test
    fun what_a_command_hid_is_not_in_the_flow() {
        val shown = flat("order")
        val hidden = flat("order", local = Local(hidden = mapOf("pickup" to true)))
        assertEquals(shown.size - 2, hidden.size)
        assertTrue(hidden.none { it.key == "id:pickup" })
    }

    @Test
    fun the_block_that_holds_a_field_is_found_for_focus() {
        val blocks = flat("order-edit")
        val byId = blockOf(blocks, "order-name")
        val byName = blockOf(blocks, "order-form/phone")
        assertTrue(byId >= 0 && byName > byId)
        assertEquals(-1, blockOf(blocks, "order-form/missing"))
    }
}

class EchoTest {
    @Test
    fun late_echoes_pass_and_outside_changes_are_seen() {
        val echo = Echo()
        // The user types "a" then "ab"; the state says "a" for a moment before it catches up.
        echo.report("a")
        echo.report("ab")
        assertFalse("the state catching up with the first letter is an echo", echo.changedOutside(current = "a", shown = "ab"))
        assertFalse(echo.changedOutside(current = "ab", shown = "ab"))
        assertTrue("the server sending the form again is a change", echo.changedOutside(current = "Harpreet Kaur", shown = "ab"))
        assertFalse(echo.changedOutside(current = "same", shown = "same"))
    }
}
