package com.navree.client.platform

import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.File
import java.nio.file.Files

/**
 * What the end of a session owes of what the web view holds, and what the
 * mark on the disk says of it to the next run of the app, in a folder of the
 * test's own.
 */
class WipeDebtTest {
    private val shop = "https://shop.example"
    private val pay = "https://pay.example"
    private val patiala = "https://patiala.shop.example"

    private val folder = Files.createTempDirectory("navree-wipe").toFile()
    private val mark = File(folder, WipeDebt.OWED)

    @After
    fun clear() {
        folder.deleteRecursively()
    }

    /** What is owed as one run of the app has it: read from the files, as a start of the app reads it. */
    private fun debt() = WipeDebt { folder }.apply { read() }

    @Test
    fun the_mark_owes_the_whole_until_the_cookies_are_gone_and_then_what_a_go_left() {
        val debt = debt()
        assertFalse(debt.owed)
        val end = debt.owe()
        assertEquals("the whole is on the disk when the end of a session is owed", WipeRules.EVERYTHING + "\n", mark.readText())
        assertTrue(debt.owed && debt.all && debt.running)
        assertTrue("every page is held back", debt.held(shop))
        // A run of the app that ends here: the next one finds the whole owed.
        assertTrue(debt().let { it.owed && it.all })

        debt.narrow(end)
        assertEquals("only what pages stored is owed, at every origin", "", mark.readText())
        assertTrue(debt.owed && !debt.all)
        assertTrue(debt().let { it.owed && !it.all && it.left == null })

        assertTrue("which pages are held back has changed", debt.over(setOf(pay), end))
        assertEquals(WipeRules.lines(setOf(pay)), mark.readText())
        assertFalse(debt.running)
        assertTrue("a page on the origin that was left stays held back", debt.held(pay))
        assertFalse("the pages of the others are shown again", debt.held(shop))
        assertEquals("the next run has its go at the origin that was left", setOf(pay), debt().left)
    }

    @Test
    fun asking_whether_a_page_is_held_back_begins_nothing_however_often_it_is_asked() {
        val debt = debt()
        val end = debt.owe()
        debt.narrow(end)
        debt.over(setOf(pay), end)
        // A page on the origin that was left is on the screen: it is drawn again each time what is owed changes, and asks each time.
        repeat(1_000) { assertTrue(debt.held(pay)) }
        assertFalse("no go was begun by the asking", debt.running)
        assertEquals("the mark is as the go left it", WipeRules.lines(setOf(pay)), mark.readText())

        // "Try again" asks for a go, and one begins; a second asking while it runs begins none.
        assertEquals(end, debt.begin())
        assertTrue(debt.running)
        assertNull(debt.begin())
        // The go leaves the origin again. Nothing begins another until somebody asks.
        debt.over(setOf(pay), end)
        repeat(1_000) { assertTrue(debt.held(pay)) }
        assertFalse(debt.running)
    }

    @Test
    fun a_go_that_ends_with_nothing_left_ends_what_is_owed() {
        val debt = debt()
        val end = debt.owe()
        debt.narrow(end)
        assertTrue(debt.over(emptySet(), end))
        assertFalse(debt.owed)
        assertFalse(debt.held(shop))
        assertFalse("no mark is left for the next run", mark.exists())
        assertFalse(debt().owed)
        assertNull("with nothing owed no go begins", debt.begin())
    }

    @Test
    fun what_was_begun_for_an_earlier_end_of_a_session_says_nothing_about_a_later_one() {
        val debt = debt()
        val first = debt.owe()
        debt.narrow(first)
        // The first session's go is still at its origins when a second person has signed in and signs out.
        val second = debt.owe()
        assertTrue(second != first)
        assertEquals(WipeRules.EVERYTHING + "\n", mark.readText())

        // A late step of the first end: its cookies were gone long ago, the second one's are not.
        debt.narrow(first)
        assertTrue("the whole of the second end is owed still", debt.all)
        assertEquals(WipeRules.EVERYTHING + "\n", mark.readText())

        // The first go ends with nothing left. It decides nothing for the second end.
        assertFalse(debt.over(emptySet(), first))
        assertTrue(debt.owed)
        assertTrue("the second end's own deletion is still on its way", debt.running)
        assertTrue("and its pages stay held back", debt.held(shop))
        assertEquals(WipeRules.EVERYTHING + "\n", mark.readText())

        debt.narrow(second)
        assertFalse(debt.over(emptySet(), first))
        assertTrue(debt.owed)
        assertTrue(debt.over(setOf(patiala), second))
        assertTrue("an origin only the second session's pages were on is owed until its own go clears it", debt.held(patiala))
        assertEquals(WipeRules.lines(setOf(patiala)), mark.readText())
    }

    @Test
    fun a_go_asked_for_before_a_later_end_of_a_session_says_nothing_about_it_either() {
        val debt = debt()
        val first = debt.owe()
        debt.narrow(first)
        debt.over(setOf(pay), first)
        // "Try again" has a go at the origin that was left; a session ends while it runs.
        val go = debt.begin()
        assertEquals(first, go)
        val second = debt.owe()
        assertFalse(debt.over(emptySet(), go!!))
        assertTrue(debt.owed && debt.all && debt.running)
        assertEquals(WipeRules.EVERYTHING + "\n", mark.readText())
        assertTrue(second > first)
    }

    @Test
    fun a_deletion_that_could_do_nothing_is_no_longer_on_its_way_and_the_whole_stays_owed() {
        val debt = debt()
        val end = debt.owe()
        // The cookie store never said its cookies are gone, so the mark was not narrowed; the deletion gives up.
        assertFalse(debt.over(null, end))
        assertFalse("nothing is being done any more", debt.running)
        assertTrue(debt.owed && debt.all)
        assertTrue(debt.held(shop))
        assertNull("the cookies are not a go's to take: the next start does the whole", debt.begin())
        assertTrue(debt().let { it.owed && it.all })

        // The cookie store answers after all: what pages stored is then a go's to take, when one is asked for.
        debt.narrow(end)
        assertEquals(end, debt.begin())
    }

    @Test
    fun a_go_is_at_every_origin_known_until_one_has_ended_and_then_at_those_it_left() {
        val debt = debt()
        debt.saw(patiala)
        debt.saw(patiala)
        assertEquals("an origin is written down once", patiala + "\n", File(folder, WipeDebt.SEEN).readText())
        val end = debt.owe()
        debt.narrow(end)
        assertEquals(listOf(shop, pay, patiala), debt.targets(own = listOf(shop), hosts = listOf(pay)))
        // A run of the app that ended here still knows the origin a page reached under the pattern.
        assertEquals(listOf(shop, pay, patiala), debt().targets(own = listOf(shop), hosts = listOf(pay)))

        debt.over(setOf(patiala), end)
        assertEquals(listOf(patiala), debt.targets(own = listOf(shop), hosts = listOf(pay)))
        assertEquals("the list of origins seen starts again from those that were left", patiala + "\n", File(folder, WipeDebt.SEEN).readText())
    }

    @Test
    fun a_mark_that_names_no_origin_owes_every_origin_and_one_that_cannot_be_read_owes_the_whole() {
        mark.writeText("https:/\n")
        assertTrue("a line that does not read names no origin: every origin is owed", debt().let { it.owed && !it.all && it.left == null })
        mark.delete()
        // Something is there under the mark's name, and it does not read as a file.
        assertTrue(mark.mkdir())
        assertTrue(debt().let { it.owed && it.all })
    }
}
