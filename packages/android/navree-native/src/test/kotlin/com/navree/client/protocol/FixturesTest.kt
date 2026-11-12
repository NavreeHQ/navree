package com.navree.client.protocol

import com.navree.client.Fixtures
import com.navree.client.runtime.submitButton
import com.navree.client.runtime.walk
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class FixturesTest {
    @Test
    fun every_envelope_decodes() {
        val names = Fixtures.names("envelopes")
        assertTrue("there are fixtures to read", names.size >= 15)
        for (name in names) {
            val envelope = decodeEnvelope(Fixtures.text("envelopes/$name"))
            assertEquals("$name speaks protocol 1", 1, envelope.navree)
            val says = envelope.screen != null || envelope.ops.isNotEmpty() || envelope.web != null || envelope.upgrade != null
            assertTrue("$name says something", says)
        }
    }

    @Test
    fun the_servers_own_examples_and_real_answers_decode() {
        for (directory in listOf("corpus", "store-demo")) {
            val names = Fixtures.names(directory)
            assertTrue("$directory is there", names.size >= 9)
            for (name in names) {
                val text = Fixtures.text("$directory/$name")
                if (name == "shell.json") {
                    assertTrue("$directory/$name has destinations", decodeShell(text).nav.groups.isNotEmpty())
                } else {
                    val envelope = decodeEnvelope(text)
                    val says = envelope.screen != null || envelope.ops.isNotEmpty() || envelope.web != null || envelope.upgrade != null
                    assertTrue("$directory/$name says something", says)
                }
            }
        }
    }

    @Test
    fun the_real_orders_screen_is_a_region_holding_a_table_with_a_query() {
        val screen = decodeEnvelope(Fixtures.text("store-demo/orders.json")).screen!!
        assertEquals("/orders", screen.nav)
        val region = screen.walk().filterIsInstance<RegionNode>().first()
        assertEquals("orders", region.id)
        assertEquals("orders", region.live?.event)
        val table = region.children.single() as TableNode
        assertEquals(25, table.rows.size)
        assertEquals(ColumnRole.Title, table.columns.first().role)
        assertTrue(table.columns.first { it.key == "payment" }.secondary)
        assertTrue(table.columns.first { it.key == "total" }.numeric)
        assertEquals(SortDir.Descending, table.columns.first { it.key == "placed_on" }.sort?.dir)
        assertNotNull(table.more)
        val query = table.query!!
        assertEquals("get", query.method)
        assertEquals("orders-f", query.key)
        val peek = table.rows.first().cells.first().walk().filterIsInstance<FormNode>().single()
        assertEquals("a form with no id goes by its address", "@/orders/200000/peek", peek.key)
        assertEquals(Submit(), peek.walk().filterIsInstance<ButtonNode>().single().action)

        val shell = decodeShell(Fixtures.text("store-demo/shell.json"))
        assertEquals(listOf("Dashboard", "Orders", "Inventory"), shell.nav.groups.single().items.map { it.label })
        assertTrue(shell.nav.groups.single().items.all { it.icon!!.paths.isNotEmpty() })
        assertEquals(Request("post", "/sign-out"), (shell.user!!.menu.last() as ButtonNode).action)

        val peeked = decodeEnvelope(Fixtures.text("store-demo/order-peek.json")).ops
        assertTrue(peeked[0] is Op.Morph && (peeked[0] as Op.Morph).node is SheetNode)
        assertEquals(Op.Open("order"), peeked[1])
    }

    @Test
    fun the_shell_has_five_destinations_a_user_and_routes() {
        val shell = decodeShell(Fixtures.text("shell.json"))
        assertEquals("Store", shell.name)
        assertEquals("#0f766e", shell.theme?.accent)
        val items = shell.nav.groups.single().items
        assertEquals(listOf("Home", "Orders", "Products", "Customers", "Settings"), items.map { it.label })
        assertEquals("a count that arrives as a number is read as text", "3", items[1].badge)
        assertTrue(items.all { it.icon!!.paths.isNotEmpty() })
        assertEquals("Gurpreet Sandhu", shell.user?.name)
        assertEquals(4, shell.user?.menu?.size)
        assertEquals(Present.Sheet, shell.routes.first { it.match == "/orders/new" }.present)
        assertFalse(shell.routes.first { it.match == "/sign-in" }.chrome)
    }

    @Test
    fun the_orders_screen_carries_roles_not_positions() {
        val screen = decodeEnvelope(Fixtures.envelope("orders")).screen!!
        assertEquals("Search orders", screen.search?.placeholder)
        assertTrue(screen.refresh)
        assertEquals(listOf(ActionRole.Create, ActionRole.Secondary, ActionRole.Secondary), screen.actions.map { (it as ButtonNode).role })
        assertEquals(Navigate("/orders/new", Present.Sheet), (screen.actions[0] as ButtonNode).action)
        assertEquals(Commands(listOf(Command.Open("order-filters"))), (screen.actions[1] as ButtonNode).action)

        val list = screen.walk().filterIsInstance<ListNode>().first()
        assertEquals(10, list.children.size)
        assertEquals("/orders?after=1033", list.more)
        val first = list.children.first() as RowNode
        assertEquals(Tone.Success, first.badge?.tone)
        assertEquals("₹12,499", (first.trailing as TextNode).value)
        val cancel = first.menu.last() as ButtonNode
        assertEquals(ActionRole.Destructive, cancel.role)
        assertEquals(Request("post", "/orders/1042/cancel", listOf(NameValue("_token", "k3J9sQ"))), cancel.action)
        assertNotNull(screen.walk().filterIsInstance<SheetNode>().singleOrNull())
    }

    @Test
    fun a_form_carries_hidden_values_inputs_errors_and_its_own_button() {
        val form = decodeEnvelope(Fixtures.envelope("order-edit")).screen!!.body as FormNode
        assertEquals(listOf(Hidden("_token", "k3J9sQ"), Hidden("_method", "patch")), form.hidden)
        val fields = form.walk().filterIsInstance<FieldNode>().associateBy { it.name }
        assertEquals(listOf("Enter a 10-digit mobile number"), fields.getValue("phone").error)
        assertEquals(TextKind.Phone, (fields.getValue("phone").input as TextInput).kind)
        assertEquals(80, (fields.getValue("name").input as TextInput).maxLength)
        assertEquals(7, (fields.getValue("city").input as ChoiceInput).options.size)
        assertEquals(DateKind.Time, (fields.getValue("deliver_at").input as DateInput).kind)
        assertEquals("12", (fields.getValue("quantity").input as TextInput).max)
        assertTrue((fields.getValue("gift").input as ToggleInput).on)
        assertEquals("Save", form.submitButton()?.label)
    }

    @Test
    fun ops_decode_with_the_names_the_web_runtime_uses() {
        val errors = decodeEnvelope(Fixtures.envelope("ops-errors")).ops
        assertEquals(
            Op.Errors("order-form", mapOf("phone" to listOf("Enter a 10-digit mobile number"), "name" to listOf("Enter the customer's name"))),
            errors[0],
        )
        assertEquals(Op.Focus("order-name"), errors[1])
        assertEquals(Tone.Danger, (errors[2] as Op.Toast).tone)
        val saved = decodeEnvelope(Fixtures.envelope("ops-saved")).ops
        assertEquals(ToastAction("Undo", "/orders/1042/undo", post = true), (saved[0] as Op.Toast).action)
        assertEquals(Op.Navigate("/orders/1042"), saved[1])
        val shipped = decodeEnvelope(Fixtures.envelope("ops-shipped")).ops
        assertEquals(BadgeNode(id = "order-status", text = "Shipped", tone = Tone.Info), (shipped[0] as Op.Morph).node)
        val more = decodeEnvelope(Fixtures.envelope("orders-more")).ops.single() as Op.Append
        assertEquals(3, more.nodes.size)
        assertNull(more.more)
    }

    @Test
    fun what_a_newer_server_sends_is_read_without_failing() {
        val envelope = decodeEnvelope(Fixtures.envelope("newer-vocabulary"))
        val screen = envelope.screen!!
        val pin = screen.actions[0] as ButtonNode
        assertEquals("a role this client does not know is no role", ActionRole.None, pin.role)
        assertEquals(Prominence.Medium, pin.prominence)
        assertEquals(UnknownAction("teleport"), pin.action)
        assertEquals("hologram", (screen.actions[1] as UnknownNode).kind)

        val body = screen.body as StackNode
        assertEquals(Axis.Vertical, body.axis)
        val text = body.children[0] as TextNode
        assertEquals(TextLevel.Body, text.style)
        assertEquals(Tone.Neutral, text.tone)
        val gauge = body.children[1] as UnknownNode
        assertEquals(ProgressNode(value = 700, label = "Seventy percent"), gauge.alt)
        assertNull((body.children[2] as UnknownNode).alt)
        assertEquals(TextNode(value = "A bare string is a text"), body.children[3])
        assertEquals(UnknownInput("emoji"), (body.children[4] as FieldNode).input)
        val count = body.children[5] as FieldNode
        assertEquals(listOf("One message as a string"), count.error)
        assertEquals(TextInput(kind = TextKind.Plain, value = "12"), count.input)
        val row = body.children[6] as RowNode
        assertNull("a state this client does not know is no state", row.state)
        assertEquals(Navigate("/x", null), row.action)
        assertEquals(TextNode(value = "Ludhiana, Punjab"), (body.children[7] as NativeNode).alt)

        assertEquals(Op.Unknown("sparkle"), envelope.ops[0])
        assertEquals(Op.Append("x", listOf(TextNode(value = "One node where a list is meant"))), envelope.ops[1])
        assertEquals(Tone.Neutral, (envelope.ops[2] as Op.Toast).tone)
    }

    @Test
    fun a_page_for_the_web_view_and_an_upgrade_notice() {
        val web = decodeEnvelope(Fixtures.envelope("web")).web!!
        assertEquals("/reports", web.url)
        assertEquals(listOf("slot:chart", "tag:canvas"), web.why)
        assertEquals("1.2.0", decodeEnvelope(Fixtures.envelope("upgrade")).upgrade?.minApp)
    }

    @Test
    fun commands_decode_from_objects_and_from_the_webs_arrays() {
        val json = "{\"t\":\"commands\",\"list\":[{\"c\":\"toggle\",\"id\":\"a\"},{\"c\":\"set\",\"id\":\"b\",\"state\":\"hidden\",\"on\":true}," +
            "{\"c\":\"flip\",\"id\":\"b\",\"state\":\"open\"},{\"c\":\"count\",\"id\":\"c\",\"max\":160},{\"c\":\"toast\",\"tone\":\"success\",\"text\":\"Saved\"}," +
            "{\"c\":\"copy\",\"text\":\"1042\"},{\"c\":\"value\",\"id\":\"sort\",\"value\":\"total\"},[\"open\",\"d\"],{\"c\":\"fly\"}]}"
        val action = NavreeJson.decodeFromString(ActionSerializer, json) as Commands
        assertEquals(
            listOf(
                Command.Toggle("a"),
                Command.Set("b", "hidden", true),
                Command.Flip("b", "open"),
                Command.Count("c", 160),
                Command.Toast(Tone.Success, "Saved"),
                Command.Copy("1042"),
                Command.Value("sort", "total"),
                Command.Open("d"),
                Command.Unknown("fly"),
            ),
            action.list,
        )
    }

    @Test
    fun text_that_is_not_an_envelope_is_refused() {
        for (bad in listOf("", "<html>", "[1,2]", "{\"navree\":\"one\"}")) {
            val failed = try {
                decodeEnvelope(bad)
                false
            } catch (_: IllegalArgumentException) {
                true
            }
            assertTrue("'$bad' is refused", failed)
        }
    }
}
