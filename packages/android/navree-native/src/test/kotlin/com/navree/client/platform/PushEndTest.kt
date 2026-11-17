package com.navree.client.platform

import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.File
import java.nio.file.Files

/**
 * The push service's side of the end of a session, with a push service of
 * the test's own that answers a deletion only when the test has it answer:
 * the time between the client being told and the service's answer is the
 * test's to hold open.
 */
class PushEndTest {
    private class Service : PushProvider {
        override val provider = "fcm"

        /** The deletions asked for, each with where its answer goes. */
        val asked = ArrayList<(Boolean) -> Unit>()

        override fun token(done: (String?) -> Unit) = done(null)

        override fun forget(done: (gone: Boolean) -> Unit) {
            asked += done
        }

        /** The service answers the deletion asked for last, or the one asked [at]. */
        fun answer(gone: Boolean, at: Int = asked.lastIndex) = asked[at](gone)
    }

    private val folder = Files.createTempDirectory("navree-push").toFile()
    private val mark = File(folder, "navree-push-token")

    /** What the client did when the push service answered, in order. */
    private val did = ArrayList<String>()

    @After
    fun clear() {
        folder.deleteRecursively()
    }

    /** The client's side as one run of the app has it: what the mark says is read from the file, as a start of the app reads it. */
    private fun client(service: PushProvider?) = PushEnd(service, OwedToken(mark), onMain = { it() }, emptyShade = { did += "the shade is emptied" }, ask = { did += "a token is asked for" })

    @Test
    fun the_token_is_owed_on_the_disk_from_the_moment_the_session_is_ending_and_the_push_service_is_asked_then() {
        val service = Service()
        val end = client(service)
        assertFalse(end.owed)
        end.owe()
        assertTrue("the mark is on the disk before anything is asked or waited for", mark.isFile)
        assertTrue(end.owed)
        assertEquals(0, service.asked.size)
        // The core says the session is ending: the server has not answered the token's removal, and is not waited for.
        end.delete()
        assertEquals("the push service is asked at once", 1, service.asked.size)
        assertTrue("and until it answers the token stays owed", end.owed)
        assertEquals(emptyList<String>(), did)
    }

    @Test
    fun when_the_push_service_says_the_token_is_gone_the_shade_is_emptied_again_and_only_then_a_new_token_is_asked_for() {
        val service = Service()
        val end = client(service)
        end.owe()
        end.delete()
        // Whatever was sent to the token and drawn while it still stood is in the shade now.
        service.answer(gone = true)
        assertEquals(listOf("the shade is emptied", "a token is asked for"), did)
        assertFalse(end.owed)
        assertFalse("nothing is owed on the disk either", mark.exists())
    }

    @Test
    fun the_session_forgotten_after_it_was_ending_has_the_push_service_asked_no_second_time() {
        val service = Service()
        val end = client(service)
        end.owe()
        end.delete()
        // `forget_session` follows `session_ending` while the service has not answered.
        end.delete()
        assertEquals(1, service.asked.size)
        service.answer(gone = true)
        // And where it follows after the answer: nothing is owed, so nothing is asked, and the new token is not deleted.
        end.delete()
        assertEquals(1, service.asked.size)
        assertEquals(listOf("the shade is emptied", "a token is asked for"), did)
    }

    @Test
    fun a_run_of_the_app_that_ends_before_the_push_service_answered_leaves_the_deletion_to_the_next_start() {
        val before = Service()
        client(before).apply {
            owe()
            delete()
        }
        assertEquals(1, before.asked.size)
        // The run ends here: the service never answers it. The next start reads the mark and asks, whatever else is owed.
        val service = Service()
        val next = client(service)
        assertTrue("the next start finds the token owed", next.owed)
        next.delete()
        assertEquals("and asks the push service to delete it", 1, service.asked.size)
        service.answer(gone = true)
        assertFalse(mark.exists())
        assertFalse(client(Service()).owed)
    }

    @Test
    fun a_deletion_the_push_service_says_failed_stays_owed_and_is_asked_again() {
        val service = Service()
        val end = client(service)
        end.owe()
        end.delete()
        service.answer(gone = false)
        assertTrue("the token is still owed its deletion", end.owed)
        assertTrue(mark.isFile)
        assertEquals("no token is asked for while the one to delete stands", emptyList<String>(), did)
        // The app comes to the front, or starts again.
        end.delete()
        assertEquals(2, service.asked.size)
        service.answer(gone = true)
        assertFalse(end.owed)
        assertEquals(listOf("the shade is emptied", "a token is asked for"), did)
    }

    @Test
    fun a_session_that_ends_while_the_push_service_is_asked_has_it_asked_once_more_before_anything_is_called_gone() {
        val service = Service()
        val end = client(service)
        end.owe()
        end.delete()
        // The next person signed in and out again before the service answered the first deletion.
        end.owe()
        end.delete()
        assertEquals("one deletion at a time", 1, service.asked.size)
        service.answer(gone = true)
        assertEquals("the answer is for a deletion asked before the last session ended: the service is asked again", 2, service.asked.size)
        assertTrue(end.owed)
        assertEquals(emptyList<String>(), did)
        service.answer(gone = true)
        assertFalse(end.owed)
        assertEquals(listOf("the shade is emptied", "a token is asked for"), did)
    }

    @Test
    fun an_answer_is_heard_once() {
        val service = Service()
        val end = client(service)
        end.owe()
        end.delete()
        service.answer(gone = true)
        service.answer(gone = true)
        service.answer(gone = false)
        assertEquals(listOf("the shade is emptied", "a token is asked for"), did)
        assertFalse(end.owed)
    }

    @Test
    fun a_provider_that_fails_when_it_is_asked_leaves_the_token_owed_for_the_next_time() {
        var asked = 0
        val broken = object : PushProvider {
            override val provider = "fcm"

            override fun token(done: (String?) -> Unit) = done(null)

            override fun forget(done: (gone: Boolean) -> Unit) {
                asked++
                throw IllegalStateException("the push service's library did not start")
            }
        }
        val end = client(broken)
        end.owe()
        end.delete()
        assertTrue(end.owed)
        end.delete()
        assertEquals("it is asked again the next time", 2, asked)
        assertEquals(emptyList<String>(), did)
    }

    @Test
    fun a_provider_with_nothing_to_delete_answers_at_once_that_it_is_gone() {
        val plain = object : PushProvider {
            override val provider = "fcm"

            override fun token(done: (String?) -> Unit) = done(null)
        }
        val end = client(plain)
        end.owe()
        end.delete()
        assertFalse(end.owed)
        assertFalse(mark.exists())
        assertEquals(listOf("the shade is emptied", "a token is asked for"), did)
    }

    /** The shade as a list of what is in it, and the end of a session whose own turn empties it, as the client's does. */
    private class Device(mark: File, val service: Service = Service()) {
        val shade = ArrayList<String>()
        val debt = OwedToken(mark)
        var begun = 0
        private val tokens = PushEnd(service, debt, onMain = { it() }, emptyShade = { shade.clear() }, ask = {})
        val end = SessionEnd(
            begin = {
                begun++
                tokens.owe()
                shade.clear()
            },
            tokens = tokens,
            emptyShade = { shade.clear() },
        )
    }

    @Test
    fun a_notification_that_reaches_the_shade_between_the_cores_two_words_is_out_of_it_when_the_session_is_forgotten() {
        val device = Device(mark)
        device.shade += "Your order from Amritsar is packed"
        // The core asks the server to remove the push token, and says the session is ending. The server has not answered.
        device.end.ending()
        assertEquals("the end of the session begins at once", 1, device.begun)
        assertEquals(emptyList<String>(), device.shade)
        assertEquals("the push service is asked to delete the token at once", 1, device.service.asked.size)
        assertTrue("from now a message that arrives is not drawn: the messaging service asks this first", device.debt.owed)

        // A message for the person who left that the push service's own library drew: the library is not asked about it.
        device.shade += "Karan: the invoice for ₹1,20,000 is attached"
        // The server answers the removal, and the core says the session is forgotten.
        device.end.forgotten()
        assertEquals("the shade is emptied again", emptyList<String>(), device.shade)
        assertEquals("nothing of the end of the session is begun a second time", 1, device.begun)
        assertEquals(1, device.service.asked.size)

        // One more was on its way from the push service when the server stopped sending.
        device.shade += "Karan: can you confirm by today"
        device.service.answer(gone = true)
        assertEquals("and again when the push service says the token is gone", emptyList<String>(), device.shade)
        assertFalse(device.debt.owed)
    }

    @Test
    fun the_session_forgotten_with_no_word_before_it_begins_the_end_of_the_session_and_has_the_token_deleted() {
        // No push token was registered with the server, so the core says only that the session is forgotten.
        val device = Device(mark)
        device.end.forgotten()
        assertEquals(1, device.begun)
        assertEquals(1, device.service.asked.size)
        // The next end of a session begins anew.
        device.service.answer(gone = true)
        device.end.forgotten()
        assertEquals(2, device.begun)
        assertEquals(2, device.service.asked.size)
    }

    @Test
    fun the_session_forgotten_after_it_was_ending_leaves_nothing_begun_so_the_next_end_of_a_session_begins_anew() {
        val device = Device(mark)
        device.end.ending()
        device.end.forgotten()
        assertEquals("the second word begins nothing a second time", 1, device.begun)
        device.service.answer(gone = true)
        // The next person signs out with no push token registered: the core says only that the session is forgotten.
        device.end.forgotten()
        assertEquals("with no word before it, the word that the session is forgotten begins its end", 2, device.begun)
        assertEquals(2, device.service.asked.size)
        // And where the core says both words again, the second again begins nothing.
        device.service.answer(gone = true)
        device.end.ending()
        device.end.forgotten()
        assertEquals(3, device.begun)
        assertEquals(3, device.service.asked.size)
    }

    @Test
    fun an_app_without_a_provider_owes_no_token_and_lets_go_of_a_mark_left_before() {
        OwedToken(mark).owe()
        assertTrue(mark.isFile)
        val end = client(null)
        assertFalse("nothing in the app could delete a token, and no messaging service waits on the mark", mark.exists())
        end.owe()
        end.delete()
        assertFalse(end.owed)
        assertFalse(mark.exists())
        assertEquals(emptyList<String>(), did)
    }
}
