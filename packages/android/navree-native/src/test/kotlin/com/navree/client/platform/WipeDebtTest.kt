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
    fun a_worker_of_an_origin_that_is_held_back_is_refused_wherever_it_writes_where_its_request_says_where_it_is_from() {
        val debt = debt()
        assertFalse("with nothing owed nothing is refused, whoever asks", debt.refuses(to = shop, from = null, said = true))
        val end = debt.owe()
        assertTrue("until a go has ended every origin is held back", debt.refuses(to = shop, from = shop, said = true))
        debt.narrow(end)
        // The go cleared the shop and left the payments host: the worker of the person before may still be registered there.
        debt.over(setOf(pay), end)
        assertTrue("to the origin that was left", debt.refuses(to = pay, from = shop, said = true))
        assertTrue("and from its worker to an origin that was cleared", debt.refuses(to = shop, from = pay, said = true))
        assertTrue("or to a host of the app no document was ever on", debt.refuses(to = patiala, from = pay, said = true))
        assertFalse("a worker of a cleared origin asks for its own pages", debt.refuses(to = shop, from = shop, said = true))
        assertTrue("a request from what is no origin of the app is not known to be from a cleared one", debt.refuses(to = shop, from = null, said = true))
        // The limit: a request that does not say where it is from goes by its address alone.
        assertFalse(debt.refuses(to = shop, from = null, said = false))
        assertTrue(debt.refuses(to = pay, from = null, said = false))
        // A later go clears the origin that was left: nothing is refused any more.
        val go = debt.begin()!!
        debt.over(emptySet(), go)
        assertFalse(debt.refuses(to = shop, from = pay, said = true))
        assertFalse(debt.refuses(to = pay, from = pay, said = true))
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
    fun when_another_person_is_signed_in_what_pages_stored_is_owed_at_every_origin_and_never_the_whole() {
        val debt = debt()
        debt.saw(patiala)
        assertTrue("what is owed changed", debt.owePages())
        assertTrue(debt.owed)
        assertFalse("the cookies are the new person's: the whole of an end of a session is not owed", debt.all)
        assertEquals("the mark is on the disk, owing what pages stored and no more", "", mark.readText())
        assertTrue("no web page is shown until it is gone", debt.held(shop))
        assertEquals("at every origin known", listOf(shop, pay, patiala), debt.targets(own = listOf(shop), hosts = listOf(pay)))
        // A run of the app that ends here: the next start finds it owed, and it is a go's to take, not the cookies'.
        assertTrue(debt().let { it.owed && !it.all && it.left == null })

        val go = debt.begin()
        assertTrue("the deletion is a go, asked for next", go != null && debt.running)
        assertTrue(debt.over(emptySet(), go!!))
        assertFalse(debt.owed)
        assertFalse(mark.exists())
    }

    @Test
    fun a_go_on_its_way_when_another_person_is_signed_in_says_nothing_about_what_is_owed_from_then() {
        val debt = debt()
        val end = debt.owe()
        debt.narrow(end)
        debt.over(setOf(pay), end)
        // "Try again" has a go at the origin a sign-out left. While it runs, another person is signed in.
        val earlier = debt.begin()!!
        assertTrue(debt.owePages())
        assertTrue("every origin is owed again, not only the one that was left", debt.held(shop))
        assertFalse("and nothing is on its way for it yet", debt.running)
        assertFalse(debt.over(emptySet(), earlier))
        assertTrue("the earlier go's end leaves it owed", debt.owed && debt.held(shop))
        // The go asked for next is the one that says when its pages may be shown.
        val go = debt.begin()!!
        assertTrue(go != earlier)
        assertTrue(debt.over(emptySet(), go))
        assertFalse(debt.owed)
    }

    @Test
    fun while_the_whole_of_an_end_of_a_session_is_owed_another_person_changes_nothing_of_it() {
        val debt = debt()
        debt.owe()
        assertFalse(debt.owePages())
        assertTrue("the whole is owed still, the cookies with it", debt.owed && debt.all && debt.running)
        assertEquals(WipeRules.EVERYTHING + "\n", mark.readText())
    }

    @Test
    fun a_cookie_store_that_does_not_answer_in_time_leaves_the_whole_of_an_end_of_a_session_owed() {
        val debt = debt()
        // Another person is signed in: what pages stored is owed at every origin, and a go is run for it.
        assertTrue(debt.owePages())
        assertTrue("every origin, and no more than what pages stored", debt.every)
        val go = debt.begin()!!
        assertFalse("while a go runs nothing more is begun for it", debt.begin() != null)
        // The store did not say its cookies are gone: what it holds is not known to be the new person's alone.
        debt.widen(go)
        assertTrue("the whole is owed", debt.owed && debt.all)
        assertFalse("and it is not what pages stored alone any more", debt.every)
        assertFalse(debt.running)
        assertEquals("on the disk, for the next start to do all of it", WipeRules.EVERYTHING + "\n", mark.readText())
        assertTrue("no web page is shown", debt.held(shop))
        assertNull("and no go ends that: \"Try again\" begins none for the rest of this run", debt.begin())
        assertFalse("what the go says after that changes nothing", debt.over(emptySet(), go))
        assertTrue(debt.owed && debt.all)
        assertTrue(debt().let { it.owed && it.all })
    }

    @Test
    fun the_whole_is_owed_again_only_for_what_is_owed_now() {
        val debt = debt()
        debt.widen(null)
        assertFalse("with nothing owed there is nothing to owe the whole of", debt.owed)
        assertFalse(mark.exists())

        val first = debt.owe()
        debt.narrow(first)
        debt.over(setOf(pay), first)
        val second = debt.owe()
        debt.narrow(second)
        // A late step of the first end says the store did not answer: the second end's own steps say how that one stands.
        debt.widen(first)
        assertFalse(debt.all)
        // Where the step names no end, it is whatever is owed now.
        debt.widen(null)
        assertTrue(debt.all)
        assertEquals(WipeRules.EVERYTHING + "\n", mark.readText())
    }

    @Test
    fun every_origin_is_owed_only_until_a_go_has_ended() {
        val debt = debt()
        assertFalse(debt.every)
        val end = debt.owe()
        assertFalse("the whole of an end of a session is more than what pages stored", debt.every)
        debt.narrow(end)
        assertTrue(debt.every)
        // A run that ends here leaves the mark saying so: the next start does the go over, with the cookie store emptied.
        assertTrue(debt().every)
        debt.over(setOf(pay), end)
        assertFalse("after a go only the origins it left are owed", debt.every)
        assertFalse(debt().every)
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
