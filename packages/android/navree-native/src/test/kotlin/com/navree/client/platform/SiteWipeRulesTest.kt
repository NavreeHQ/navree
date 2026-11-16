package com.navree.client.platform

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.File
import java.nio.file.Files

/** The rules of the client's own document that takes an origin's service workers and caches away when a session has ended. */
class SiteWipeRulesTest {
    private val word = "0f3a9c5e7b1d2468ace013579bdf2468"
    private val token = "7b1d2468ace013579bdf24680f3a9c5e"

    @Test
    fun the_clients_own_document_is_at_a_path_the_go_made_and_nothing_else_is_it() {
        val address = WipeRules.address("https://shop.example", token)
        assertEquals("https://shop.example/$token", address)
        assertTrue(WipeRules.own(address, token))
        assertTrue("on whichever origin is in turn", WipeRules.own(WipeRules.address("http://localhost:8097", token), token))
        assertFalse("another go's path is not it", WipeRules.own(WipeRules.address("https://shop.example", "ab12"), token))
        assertFalse("nor an address that only begins like it", WipeRules.own("$address?next=/orders", token))
        assertFalse(WipeRules.own("$address/orders", token))
        assertFalse(WipeRules.own("$address#top", token))
        assertFalse(WipeRules.own("https://shop.example/", token))
        assertFalse("what is not a web address", WipeRules.own("blob:https://shop.example/$token", token))
        assertFalse("no go, no document", WipeRules.own("https://shop.example/", ""))
        assertFalse("the word a document answers with is not in its address", address.contains(word))
    }

    @Test
    fun while_a_deletion_is_owed_every_page_is_held_back_until_a_go_has_ended_and_then_only_the_pages_of_an_origin_it_left() {
        assertTrue(WipeRules.held(left = null, origin = "https://shop.example"))
        val left = setOf("https://pay.example")
        assertTrue(WipeRules.held(left, "https://pay.example"))
        assertFalse("an origin that said it is clean is not held for another", WipeRules.held(left, "https://shop.example"))
        assertTrue("an address on none of the app's origins", WipeRules.held(left, null))
    }

    @Test
    fun an_origin_has_its_tries_and_is_then_left_while_the_go_moves_on() {
        val turns = WipeRules.Turns(listOf("https://shop.example", "https://pay.example", "https://patiala.shop.example"))
        assertEquals("https://shop.example", turns.current)
        turns.cleared()
        assertEquals("https://pay.example", turns.current)
        repeat(WipeRules.TRIES - 1) {
            turns.missed()
            assertEquals("it is shown the document again", "https://pay.example", turns.current)
        }
        turns.missed()
        assertEquals("the next origin is not kept waiting", "https://patiala.shop.example", turns.current)
        turns.cleared()
        assertNull(turns.current)
        assertEquals(setOf("https://pay.example"), turns.left)
        // What is written into the mark, and read from it at the next start.
        assertEquals(listOf("https://pay.example"), WipeRules.listed(WipeRules.lines(turns.left)))
    }

    @Test
    fun a_go_that_cannot_go_on_leaves_every_origin_not_yet_cleared() {
        val turns = WipeRules.Turns(listOf("https://shop.example", "https://pay.example", "https://patiala.shop.example"))
        turns.cleared()
        turns.stop()
        assertNull(turns.current)
        assertEquals(setOf("https://pay.example", "https://patiala.shop.example"), turns.left)
    }

    @Test
    fun a_line_that_does_not_read_as_an_origin_is_dropped_from_the_list_and_the_others_stay() {
        // A line cut short, one with no port after its colon, and two lines run together, among lines that read.
        val text = "https://shop.example\nhttps:/\nhttp://localhost:\nhttps://amritsar.shhttps://patiala.shop.example\nhttps://patiala.shop.example\n\n" +
            "https://shop.example\nhttp://localhost:8097/\nhttps://shop.example/orders\n"
        assertEquals(listOf("https://shop.example", "https://patiala.shop.example", "http://localhost:8097"), WipeRules.listed(text))
        assertEquals(emptyList<String>(), WipeRules.listed(""))
    }

    @Test
    fun the_list_is_written_whole_and_put_in_its_place_in_one_step() {
        val folder = Files.createTempDirectory("navree-origins").toFile()
        try {
            val file = File(folder, "origins")
            WipeRules.put(file, "https://shop.example\n")
            assertEquals("https://shop.example\n", file.readText())
            WipeRules.put(file, "https://shop.example\nhttps://patiala.shop.example\n")
            assertEquals(listOf("https://shop.example", "https://patiala.shop.example"), WipeRules.listed(file.readText()))
            assertEquals("nothing is left beside the list", listOf("origins"), folder.list()?.toList())
        } finally {
            folder.deleteRecursively()
        }
    }

    @Test
    fun the_origins_to_clear_are_the_apps_own_then_its_web_hosts_then_the_ones_seen_each_once() {
        assertEquals(
            listOf("https://shop.example", "https://pay.example", "https://amritsar.shop.example", "https://patiala.shop.example"),
            WipeRules.origins(
                own = listOf("https://shop.example"),
                hosts = listOf("https://pay.example"),
                seen = listOf("https://patiala.shop.example", "https://shop.example", "https://amritsar.shop.example", "https://pay.example"),
            ),
        )
        assertEquals(emptyList<String>(), WipeRules.origins(emptyList(), emptyList(), emptyList()))
    }

    @Test
    fun two_origins_are_the_same_by_scheme_host_and_port_however_the_port_is_written() {
        assertTrue(WipeRules.same("https://shop.example", "https://shop.example:443"))
        assertTrue(WipeRules.same("http://localhost:8097", "http://localhost:8097"))
        assertFalse(WipeRules.same("https://shop.example", "http://shop.example"))
        assertFalse(WipeRules.same("https://shop.example", "https://pay.shop.example"))
        assertFalse(WipeRules.same("http://localhost:8097", "http://localhost:8098"))
        assertFalse("what does not read is no origin", WipeRules.same("null", "https://shop.example"))
    }

    @Test
    fun what_a_document_said_is_read_only_with_the_gos_own_word_and_is_clean_only_when_nothing_is_left_and_nothing_failed() {
        assertEquals(WipeRules.Report(workers = 1, caches = 2, clean = true), WipeRules.read("""{"n":"$word","workers":1,"caches":2,"left":0}""", word))
        assertEquals(WipeRules.Report(workers = 0, caches = 0, clean = true), WipeRules.read("""{"n":"$word","workers":0,"caches":0,"left":0}""", word))
        assertEquals("a cache that stayed", false, WipeRules.read("""{"n":"$word","workers":0,"caches":1,"left":1}""", word)?.clean)
        assertEquals("a failure", false, WipeRules.read("""{"n":"$word","workers":0,"caches":0,"left":0,"error":"InvalidStateError"}""", word)?.clean)
        assertNull("another go's word", WipeRules.read("""{"n":"ab12","workers":0,"caches":0,"left":0}""", word))
        assertNull("no count of what is left", WipeRules.read("""{"n":"$word","workers":0,"caches":0}""", word))
        assertNull(WipeRules.read("done", word))
        assertNull(WipeRules.read("[1,2]", word))
        assertNull(WipeRules.read(null, word))
    }

    @Test
    fun the_script_carries_the_gos_word_and_its_listener_and_takes_only_hex_digits_into_its_text() {
        val script = WipeRules.script(word)
        assertTrue(script.startsWith("(function(){") && script.endsWith("})();"))
        assertTrue("n:'$word'" in script)
        assertTrue("self.${WipeRules.LISTENER}" in script)
        // The document is stopped and emptied before the listener is looked for: a page a worker answered with never runs.
        assertTrue(script.indexOf("window.stop()") in 0 until script.indexOf("self.${WipeRules.LISTENER}"))
        assertTrue(script.indexOf("d.remove()") in 0 until script.indexOf("self.${WipeRules.LISTENER}"))
        assertThrows(IllegalArgumentException::class.java) { WipeRules.script("ab';alert(1);'") }
        assertThrows(IllegalArgumentException::class.java) { WipeRules.script("") }
    }
}
