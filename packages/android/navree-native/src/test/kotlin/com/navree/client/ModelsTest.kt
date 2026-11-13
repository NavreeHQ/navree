package com.navree.client

import com.navree.client.design.walk
import com.navree.protocol.Navree
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * The protocol's generated models read every example answer the protocol
 * keeps, every answer recorded from the store demo, and an answer from a
 * newer server, the way an app older than its server must.
 */
class ModelsTest {
    @Test
    fun every_example_answer_of_the_protocol_reads() {
        val names = Fixtures.corpus()
        assertTrue("the protocol's examples are on the classpath", names.size >= 15)
        for (name in names) {
            val text = Fixtures.text(name)
            if (name == "shell.json") {
                val shell = Navree.json.decodeFromString(Navree.Shell.serializer(), text)
                assertEquals("Charkha Supply Co.", shell.name)
                assertTrue(shell.nav.groups.sumOf { it.items.size } >= 3)
                assertTrue("the shell says how routes are presented", shell.routes.isNotEmpty())
            } else {
                val envelope = Navree.json.decodeFromString(Navree.Envelope.serializer(), text)
                assertEquals("$name speaks the protocol's major", Navree.MAJOR.toLong(), envelope.navree)
                assertTrue("$name holds something", envelope.screen != null || envelope.ops.isNotEmpty() || envelope.web != null || envelope.upgrade != null)
            }
        }
    }

    @Test
    fun every_answer_recorded_from_the_store_demo_reads() {
        val names = Fixtures.store()
        assertTrue(names.size >= 14)
        for (name in names) {
            val text = Fixtures.text("store-demo/$name")
            if (name.startsWith("_navree_native_shell")) {
                assertEquals("Charkha Supply Co.", Navree.json.decodeFromString(Navree.Shell.serializer(), text).name)
            } else {
                val envelope = Navree.json.decodeFromString(Navree.Envelope.serializer(), text)
                assertTrue(name, envelope.screen != null || envelope.ops.isNotEmpty() || envelope.web != null)
            }
        }
    }

    @Test
    fun an_answer_from_a_newer_server_reads_with_its_stand_ins() {
        val envelope = Fixtures.envelope("from-the-future.json")
        val screen = assertNotNullScreen(envelope)
        val nodes = (sequenceOf(screen.body) + screen.actions.asSequence()).flatMap { it.walk() }.toList()

        // A kind this build has never heard of keeps its stand-in, or has none.
        val unknown = nodes.filterIsInstance<Navree.Node.Unknown>()
        assertEquals(listOf("hologram", "hologram"), unknown.map { it.tag })
        assertTrue(unknown.first().alt is Navree.Image)
        assertNull(unknown.last().alt)

        // An action, a command, an input and a device call from the future.
        val buttons = nodes.filterIsInstance<Navree.Button>()
        assertTrue(buttons.first { it.label == "Teleport" }.action is Navree.Action.Unknown)
        val commands = (buttons.first { it.label == "Celebrate" }.action as Navree.Action.Commands).list
        assertTrue(commands.first() is Navree.Command.Unknown)
        assertTrue(commands.last() is Navree.Command.Toast)
        val measure = buttons.first { it.label == "Measure the aisle" }.action as Navree.DeviceCall
        assertTrue(measure.call is Navree.Call.Unknown)
        val fields = nodes.filterIsInstance<Navree.Field>()
        assertTrue(fields.first { it.name == "signature" }.input is Navree.Input.Unknown)
        assertEquals("24", (fields.first { it.name == "units" }.input as Navree.TextInput).value)

        // Ops it does not know are kept as such; the ones it knows read.
        assertTrue(envelope.ops.first() is Navree.Op.Unknown)
        assertTrue("a toast with a field it does not know is still a toast", envelope.ops.last() is Navree.Toast)
    }

    @Test
    fun the_kinds_the_protocol_gained_read_as_themselves() {
        val kinds = Fixtures.screen("screen-kinds.json")
        val dashboard = Fixtures.screen("screen-dashboard.json")
        val inbox = Fixtures.screen("screen-inbox.json")
        val nodes = sequenceOf(kinds, dashboard, inbox)
            .flatMap { screen -> (sequenceOf(screen.body) + screen.actions.asSequence()).flatMap { it.walk() } }
            .map { it::class.simpleName }
            .toSet()
        for (kind in listOf("Toolbar", "Gauge", "Figure", "Timeline", "FeedRow", "Message", "Pane")) {
            assertTrue("$kind is in the examples and reads as itself", kind in nodes)
        }
    }

    @Test
    fun a_screen_says_its_actions_by_role() {
        val orders = Fixtures.screen("screen-list.json")
        val create = orders.actions.filterIsInstance<Navree.Button>().single()
        assertEquals(Navree.Role.CREATE, create.role)
    }

    @Test
    fun a_word_a_closed_list_lacks_reads_as_the_fields_default() {
        val read = Navree.json.decodeFromString(Navree.Button.serializer(), """{"label":"Go","role":"celebrate","prominence":"loudest"}""")
        assertEquals(Navree.Role.NONE, read.role)
        assertEquals(Navree.Prominence.MEDIUM, read.prominence)
    }

    private fun assertNotNullScreen(envelope: Navree.Envelope): Navree.Screen {
        assertNotNull(envelope.screen)
        return envelope.screen!!
    }
}
