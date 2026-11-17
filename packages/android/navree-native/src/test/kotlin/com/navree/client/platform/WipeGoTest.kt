package com.navree.client.platform

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * One go at the origins whose service workers and caches go when a session
 * has ended, with a web view of the test's own: one that loads what it is
 * told and says only what the test has it say, and a clock that moves only
 * when the test moves it.
 */
class WipeGoTest {
    private val token = "7b1d2468ace013579bdf24680f3a9c5e"
    private val word = "0f3a9c5e7b1d2468ace013579bdf2468"
    private val shop = "https://shop.example"
    private val pay = "https://pay.example"
    private val patiala = "https://patiala.shop.example"

    private class Surface(private val opens: Boolean = true) : WipeSurface {
        val loaded = ArrayList<String>()
        var rules: Set<String> = emptySet()
        var closed = 0
        private var say: ((String?, String, Boolean) -> Unit)? = null
        private var lost: (() -> Unit)? = null
        private var late: (() -> Unit)? = null

        override fun open(origins: Set<String>, script: String, heard: (text: String?, source: String, mainFrame: Boolean) -> Unit, broke: () -> Unit) {
            if (!opens) throw UnsupportedOperationException("no web view here")
            rules = origins
            say = heard
            lost = broke
        }

        override fun load(address: String) {
            loaded += address
        }

        override fun after(ms: Long, what: () -> Unit) {
            late = what
        }

        override fun cancel() {
            late = null
        }

        override fun close() {
            closed++
        }

        /** The time an origin is given passes with nothing said. */
        fun pass() {
            val what = late
            late = null
            what?.invoke()
        }

        /** A document says something, as the web view hands it on. */
        fun says(text: String?, source: String, mainFrame: Boolean = true) {
            say?.invoke(text, source, mainFrame)
        }

        /** The web view's process ends. */
        fun lose() {
            lost?.invoke()
        }

        val waiting: Boolean get() = late != null
    }

    private fun clean(word: String = this.word) = """{"n":"$word","workers":1,"caches":2,"left":0}"""

    private fun address(origin: String) = WipeRules.address(origin, token)

    /** The go, and what it said at its end: null until then. */
    private fun go(surface: Surface, vararg origins: String): () -> Set<String>? {
        var left: Set<String>? = null
        var times = 0
        WipeGo(origins.toList(), token, word, surface) {
            left = it
            times++
        }.start()
        return {
            assertTrue("the end is told once", times <= 1)
            left
        }
    }

    @Test
    fun a_web_view_that_never_says_anything_leaves_every_origin_after_its_tries_and_the_go_ends() {
        val surface = Surface()
        val left = go(surface, shop, pay)
        assertEquals("the listener and the script are given to the origins of the go and no other", setOf(shop, pay), surface.rules)
        repeat(WipeRules.TRIES * 2) {
            assertNull("the go is not over while an origin has a try left", left())
            assertTrue(surface.waiting)
            surface.pass()
        }
        assertEquals(setOf(shop, pay), left())
        assertEquals("each origin was shown the document as often as it has tries, in turn", List(WipeRules.TRIES) { address(shop) } + List(WipeRules.TRIES) { address(pay) }, surface.loaded)
        assertEquals("the web view is closed, once", 1, surface.closed)
        assertFalse("and nothing is left to wait for", surface.waiting)
    }

    @Test
    fun an_origin_that_does_not_say_it_is_clean_is_left_and_the_others_are_cleared_all_the_same() {
        val surface = Surface()
        val left = go(surface, shop, pay, patiala)
        surface.says(clean(), shop)
        assertEquals(address(pay), surface.loaded.last())
        repeat(WipeRules.TRIES) { surface.pass() }
        assertNull("the origin after the one that was left has its turn", left())
        assertEquals(address(patiala), surface.loaded.last())
        surface.says(clean(), patiala)
        assertEquals(setOf(pay), left())
        assertEquals(1, surface.closed)
    }

    @Test
    fun every_origin_that_says_it_is_clean_leaves_nothing() {
        val surface = Surface()
        val left = go(surface, shop, pay)
        surface.says(clean(), shop)
        surface.says(clean(), "$pay:443")
        assertEquals(emptySet<String>(), left())
        assertEquals(listOf(address(shop), address(pay)), surface.loaded)
    }

    @Test
    fun only_the_main_frame_of_the_origin_in_turn_with_the_gos_own_word_is_heard() {
        val surface = Surface()
        val left = go(surface, shop, pay)
        surface.says(clean(), pay)
        surface.says(clean(), shop, mainFrame = false)
        surface.says(clean(word = "ab12"), shop)
        // The path of the address is not the word: a page that read its own address cannot answer for the script.
        surface.says(clean(word = token), shop)
        surface.says("clean", shop)
        surface.says(null, shop)
        assertEquals("the first origin is still in turn", listOf(address(shop)), surface.loaded)
        assertNull(left())
        surface.says(clean(), shop)
        assertEquals(address(pay), surface.loaded.last())
    }

    @Test
    fun an_origin_that_says_something_is_left_is_shown_the_document_again_and_then_left() {
        val surface = Surface()
        val left = go(surface, shop, pay)
        val dirty = """{"n":"$word","workers":1,"caches":1,"left":1}"""
        repeat(WipeRules.TRIES) {
            assertEquals(address(shop), surface.loaded.last())
            surface.says(dirty, shop)
        }
        assertEquals(address(pay), surface.loaded.last())
        surface.says("""{"n":"$word","workers":0,"caches":0,"left":0,"error":"SecurityError"}""", pay)
        assertEquals("a failure is another try", address(pay), surface.loaded.last())
        surface.says(clean(), pay)
        assertEquals(setOf(shop), left())
    }

    @Test
    fun a_web_view_that_is_lost_leaves_every_origin_not_yet_cleared() {
        val surface = Surface()
        val left = go(surface, shop, pay, patiala)
        surface.says(clean(), shop)
        surface.lose()
        assertEquals(setOf(pay, patiala), left())
        assertEquals(1, surface.closed)
        // What comes after the end changes nothing.
        surface.says(clean(), pay)
        surface.pass()
        surface.lose()
        assertEquals(setOf(pay, patiala), left())
        assertEquals(1, surface.closed)
    }

    @Test
    fun a_web_view_that_cannot_be_made_leaves_every_origin() {
        val surface = Surface(opens = false)
        val left = go(surface, shop, pay)
        assertEquals(setOf(shop, pay), left())
        assertEquals(emptyList<String>(), surface.loaded)
    }

    @Test
    fun a_go_at_no_origin_is_over_at_once_with_nothing_left() {
        val surface = Surface()
        val left = go(surface)
        assertEquals(emptySet<String>(), left())
        assertEquals(emptyList<String>(), surface.loaded)
    }

    /** The path and the word of a second go: the first go's, read backwards, so they are hex digits and another go's. */
    private val laterPath = token.reversed()
    private val laterWord = word.reversed()

    @Test
    fun a_go_begun_while_another_runs_takes_its_place_and_the_older_one_ends_leaving_what_it_had_not_cleared() {
        val goes = WipeGoes()
        val first = Surface()
        var firstLeft: Set<String>? = null
        var firstEnds = 0
        goes.start(listOf(shop, pay), token, word, first) {
            firstLeft = it
            firstEnds++
        }
        first.says(clean(), shop)
        assertEquals("the first go is at its second origin", address(pay), first.loaded.last())
        assertEquals("its address is the one answered as the client's own document", token, goes.going)

        // A later session ended: its go begins while the first is still at an origin.
        val second = Surface()
        var secondLeft: Set<String>? = null
        goes.start(listOf(patiala), laterPath, laterWord, second) { secondLeft = it }
        assertEquals("the first go is told to end, with the origin it had not cleared left", setOf(pay), firstLeft)
        assertEquals("and its web view is gone, so nothing of it asks for its address any more", 1, first.closed)
        assertFalse(first.waiting)
        assertEquals("the one address answered is the second go's", laterPath, goes.going)
        assertNull(secondLeft)

        // What the first go's web view might still say changes nothing, and does not take the second go's address away.
        first.says(clean(), pay)
        first.pass()
        first.lose()
        assertEquals(1, firstEnds)
        assertEquals(laterPath, goes.going)

        second.says(clean(laterWord), patiala)
        assertEquals(emptySet<String>(), secondLeft)
        assertNull("with no go on its way no address is the client's own document", goes.going)
        assertEquals(1, second.closed)
    }

    @Test
    fun a_go_that_ended_by_itself_is_not_ended_again_by_the_next() {
        val goes = WipeGoes()
        val first = Surface()
        var firstEnds = 0
        goes.start(listOf(shop), token, word, first) { firstEnds++ }
        first.says(clean(), shop)
        assertEquals(1, firstEnds)
        assertNull(goes.going)

        val second = Surface(opens = false)
        var secondLeft: Set<String>? = null
        goes.start(listOf(pay), laterPath, laterWord, second) { secondLeft = it }
        assertEquals("the first go is told its end once", 1, firstEnds)
        assertEquals(1, first.closed)
        assertEquals("a go whose web view cannot be made leaves its origins", setOf(pay), secondLeft)
        assertNull("and its address is answered no longer", goes.going)
    }
}
