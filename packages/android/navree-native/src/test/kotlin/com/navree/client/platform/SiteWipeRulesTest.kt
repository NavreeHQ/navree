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

    @Test
    fun the_clients_own_document_is_the_main_frames_request_at_the_address_that_carries_the_gos_word() {
        val address = WipeRules.address("https://shop.example", word)
        assertEquals("https://shop.example/?navree-wipe=$word", address)
        assertTrue(WipeRules.own(address, mainFrame = true, word = word))
        assertFalse("a frame's request is not it", WipeRules.own(address, mainFrame = false, word = word))
        assertFalse("another go's word is not it", WipeRules.own(WipeRules.address("https://shop.example", "ab12"), mainFrame = true, word = word))
        assertFalse("nor an address that only begins like it", WipeRules.own("$address&next=/orders", mainFrame = true, word = word))
        assertFalse(WipeRules.own("https://shop.example/", mainFrame = true, word = word))
        assertEquals("http://localhost:8097/?navree-wipe=$word", WipeRules.address("http://localhost:8097", word))
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
