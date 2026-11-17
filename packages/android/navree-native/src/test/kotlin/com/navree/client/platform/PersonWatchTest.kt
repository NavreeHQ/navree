package com.navree.client.platform

import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.File
import java.nio.file.Files
import java.security.MessageDigest

/**
 * Who the core says is signed in, heard each time it says so, and the mark
 * of whom what is kept on the device is kept for, in a folder of the test's
 * own. The client is told of no end of a session when one ran out and
 * somebody else signed in: this is how it knows to take away what was the
 * last person's, in the same run or at the next start.
 */
class PersonWatchTest {
    private val folder = Files.createTempDirectory("navree-person").toFile()
    private val file = File(folder, PersonWatch.MARK)
    private val shop = "https://shop.example:443"

    private val anjali = "Anjali Verma" to "anjali.verma@example.com"
    private val karan = "Karan Malhotra" to "karan.malhotra@example.com"
    private val pooja = "Pooja Khanna" to "pooja.khanna@example.com"

    @After
    fun clear() {
        folder.deleteRecursively()
    }

    /** The watch as one run of the app has it: what it remembers is read from the file, as a start of the app reads it. */
    private fun watch(origin: String = shop) = PersonWatch(file, origin)

    private fun PersonWatch.hear(who: Pair<String, String?>) = hear(who.first, who.second)

    /** Hears, and writes the mark down where the word says so, as the client does in the store's turn. */
    private fun PersonWatch.hearAndKeep(who: Pair<String, String?>): Heard = hear(who).also { if (it.write) keep(it) }.heard

    /** The mark the file holds, or null when there is no file. */
    private fun kept(): String? = if (file.isFile) file.readLines()[1] else null

    private fun mark(who: Pair<String, String?>) = PersonWatch.markOf(who.first, who.second)

    private fun sha(written: String) = MessageDigest.getInstance("SHA-256").digest(written.encodeToByteArray()).joinToString("") { "%02X".format(it) }

    @Test
    fun the_mark_of_a_person_is_made_from_name_and_email_the_way_every_client_makes_it() {
        assertEquals(sha("12:Anjali Verma+anjali.verma@example.com"), PersonWatch.markOf("Anjali Verma", "anjali.verma@example.com"))
        assertEquals(sha("12:Anjali Verma-"), PersonWatch.markOf("Anjali Verma", null))
        assertNotEquals("a person with no email and one whose email is empty are two", PersonWatch.markOf("Anjali Verma", null), PersonWatch.markOf("Anjali Verma", ""))
        assertNotEquals("a name is never taken for a name and an email", PersonWatch.markOf("Anjali+verma@example.com", null), PersonWatch.markOf("Anjali", "verma@example.com"))
        assertNotEquals(mark(anjali), mark(karan))
        assertEquals("sixty-four hex digits, from which neither is read", 64, mark(anjali).length)
        assertFalse(mark(anjali).contains("Anjali", ignoreCase = true))
    }

    @Test
    fun two_people_with_the_same_name_and_no_email_are_one_to_the_client_and_a_person_who_is_renamed_is_another_once() {
        val watch = watch()
        assertEquals(Heard.First, watch.hearAndKeep("Karan Malhotra" to null))
        assertEquals("what cannot be told apart is not", Heard.Same, watch.hearAndKeep("Karan Malhotra" to null))

        val renamed = watch.hear("Karan S. Malhotra" to null)
        assertEquals("a new name is another person to the client", Heard.Another, renamed.heard)
        watch.changed(renamed)
        assertEquals("once: from then on it is the name remembered", Heard.Same, watch.hearAndKeep("Karan S. Malhotra" to null))
    }

    @Test
    fun another_person_named_with_no_sign_out_between_is_a_change_of_person() {
        val watch = watch()
        assertEquals("the first person named follows nobody", Heard.First, watch.hearAndKeep(anjali))
        assertEquals(mark(anjali), kept())
        assertEquals(Heard.Same, watch.hearAndKeep(anjali))
        // Her session ran out; while the core names nobody nothing is heard; then somebody else signed in.
        assertEquals(Heard.Another, watch.hear(karan).heard)
        assertEquals("and is the one named from then on", Heard.Same, watch.hear(karan).heard)
    }

    @Test
    fun the_mark_is_read_at_the_start_so_a_change_of_person_while_the_app_was_closed_is_seen() {
        watch().hearAndKeep(anjali)
        // The app is closed. Her session runs out, and the next start is somebody else's.
        assertEquals(Heard.Another, watch().hear(karan).heard)
        // Where it is hers again, nothing changed.
        val hers = watch().hear(anjali)
        assertEquals(Heard.Same, hers.heard)
        assertFalse("the mark kept is hers already", hers.write)
    }

    @Test
    fun the_mark_of_the_person_before_stays_until_every_step_has_finished_and_a_run_that_ends_in_the_middle_leaves_the_next_start_to_do_it_over() {
        val first = watch()
        first.hearAndKeep(anjali)
        val word = first.hear(karan)
        assertEquals(Heard.Another, word.heard)
        assertFalse("nothing is written for him yet", word.write)
        first.keep(word)
        assertEquals("the mark on the disk is still hers", mark(anjali), kept())
        // He is named again and again while her things are being taken away: nothing is written.
        assertEquals(Heard.Same, first.hearAndKeep(karan))
        assertEquals(mark(anjali), kept())

        // The app ends here. The next start remembers her, hears him, and does the change over.
        val second = watch()
        val again = second.hear(karan)
        assertEquals(Heard.Another, again.heard)
        // This time every step finishes.
        second.changed(again)
        assertEquals("what is kept on the device is kept for him now", mark(karan), kept())
        assertEquals(Heard.Same, watch().hear(karan).heard)
    }

    @Test
    fun when_yet_another_person_is_named_while_the_steps_run_the_mark_is_theirs_to_write() {
        val watch = watch()
        watch.hearAndKeep(anjali)
        val his = watch.hear(karan)
        val hers = watch.hear(pooja)
        assertEquals(Heard.Another, hers.heard)
        // The steps begun for him finish first: they say nothing of what was kept since for her.
        watch.changed(his)
        assertEquals(mark(anjali), kept())
        watch.changed(hers)
        assertEquals(mark(pooja), kept())
    }

    @Test
    fun a_sign_out_forgets_who_was_named_so_whoever_is_named_next_is_the_first() {
        val watch = watch()
        watch.hearAndKeep(anjali)
        // She signed out: the end of the session takes everything of hers, the mark with it.
        watch.signedOut()
        assertFalse("no mark is left on the disk", file.exists())
        assertEquals(Heard.First, watch.hearAndKeep(karan))
        assertEquals(mark(karan), kept())
        // And across a start: a run that ended signed out remembers nobody.
        watch.signedOut()
        assertEquals(Heard.First, watch().hear(anjali).heard)
    }

    @Test
    fun what_was_heard_before_a_sign_out_writes_nothing_after_it() {
        val watch = watch()
        val heard = watch.hear(anjali)
        assertTrue(heard.write)
        watch.signedOut()
        // The write was still waiting for its turn when she signed out.
        watch.keep(heard)
        assertFalse(file.exists())

        watch.hearAndKeep(anjali)
        val change = watch.hear(karan)
        watch.signedOut()
        // So were the steps of a change of person.
        watch.changed(change)
        assertFalse(file.exists())
        assertEquals(Heard.First, watch.hear(pooja).heard)
    }

    @Test
    fun a_mark_is_asked_to_be_written_once_and_again_only_when_it_was_not() {
        val watch = watch()
        val first = watch.hear(anjali)
        assertTrue(first.write)
        assertFalse("the write is on its way: it is not asked for a second time", watch.hear(anjali).write)
        watch.keep(first)
        assertEquals(mark(anjali), kept())
        assertFalse(watch.hear(anjali).write)
    }

    @Test
    fun a_mark_kept_at_another_origin_is_not_compared_with() {
        watch().hearAndKeep(anjali)
        // The same build is pointed at another server, where somebody else is signed in.
        val elsewhere = watch("https://staging.shop.example:443")
        assertEquals("nobody was named at this origin before", Heard.First, elsewhere.hearAndKeep(karan))
        assertEquals("and what is kept from now on is kept for him, there", listOf("https://staging.shop.example:443", mark(karan)), file.readLines())
    }

    @Test
    fun a_mark_that_does_not_read_remembers_nobody() {
        file.writeText(shop + "\n")
        assertEquals(Heard.First, watch().hear(anjali).heard)
        file.writeText("\n\n")
        assertEquals(Heard.First, watch().hear(anjali).heard)
        // Something is there under the mark's name, and it does not read as a file.
        file.delete()
        assertTrue(file.mkdir())
        assertEquals(Heard.First, watch().hear(anjali).heard)
    }
}
