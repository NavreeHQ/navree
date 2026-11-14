package com.navree.client.platform

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class StoredStateTest {
    /** A thread of work the test turns by hand: each piece runs when the test says so. */
    private class ByHand {
        val queued = ArrayDeque<Runnable>()

        fun turn() = queued.removeFirst().run()

        fun all() {
            while (queued.isNotEmpty()) turn()
        }
    }

    @Test
    fun work_runs_one_piece_at_a_time_in_the_order_it_was_asked_for() {
        val thread = ByHand()
        val state = StoredState(thread.queued::addLast)
        val done = ArrayList<String>()
        state.write { done += "page" }
        state.always { done += "clear" }
        state.read { done += "read" }
        assertEquals("only the first is handed to the thread", 1, thread.queued.size)
        thread.all()
        assertEquals(listOf("page", "clear", "read"), done)
    }

    @Test
    fun a_write_of_a_session_that_has_ended_is_dropped() {
        val thread = ByHand()
        val state = StoredState(thread.queued::addLast)
        val kept = ArrayList<String>()
        val before = state.generation
        state.write(before) { kept += "a page written before the end" }
        state.end { done -> done() }
        // A late answer of the session before: its cookies arrive after the end.
        state.write(before) { kept += "a cookie of the session before" }
        state.write { kept += "a page of the new session" }
        thread.all()
        assertEquals(listOf("a page of the new session"), kept)
        assertEquals(before + 1, state.generation)
    }

    @Test
    fun a_read_is_told_whether_its_session_is_still_the_one_on_the_device() {
        val thread = ByHand()
        val state = StoredState(thread.queued::addLast)
        val answers = ArrayList<Boolean>()
        val before = state.generation
        state.read(before) { answers += it }
        state.end { done -> done() }
        state.read(before) { answers += it }
        state.read { answers += it }
        thread.all()
        assertEquals(listOf(false, false, true), answers)
    }

    @Test
    fun nothing_after_the_wipe_runs_before_the_wipe_has_finished() {
        val thread = ByHand()
        val state = StoredState(thread.queued::addLast)
        val order = ArrayList<String>()
        var finish: (() -> Unit)? = null
        state.end { done ->
            order += "wipe started"
            finish = done
        }
        state.always { order += "the next request reads its cookies" }
        thread.all()
        assertEquals(listOf("wipe started"), order)
        // The cookie store says it has removed every cookie.
        finish!!()
        thread.all()
        assertEquals(listOf("wipe started", "the next request reads its cookies"), order)
        finish!!()
        thread.all()
        assertEquals("a second word from the wipe changes nothing", 2, order.size)
    }

    @Test
    fun the_end_of_a_session_arms_its_time_limit_before_anything_it_guards() {
        val order = ArrayList<String>()
        var limit: (() -> Unit)? = null
        var released = 0
        endWithLimit({ released++ }, arm = { release ->
            order += "limit armed"
            limit = release
        }) {
            order += "wipe"
            throw StackOverflowError("the web view did not start")
        }
        assertEquals(listOf("limit armed", "wipe"), order)
        assertEquals("a failure of any kind lets the queue go on at once", 1, released)

        // A wipe that never says it is done is ended by its limit.
        var quietLimit: (() -> Unit)? = null
        var quietReleased = 0
        endWithLimit({ quietReleased++ }, arm = { quietLimit = it }) { order += "a cookie store that never answers" }
        assertEquals(0, quietReleased)
        quietLimit!!()
        assertEquals(1, quietReleased)
        assertTrue(limit != null)
    }

    @Test
    fun a_piece_of_work_that_fails_does_not_stop_the_rest() {
        val thread = ByHand()
        val state = StoredState(thread.queued::addLast)
        var ran = false
        state.always { throw IllegalStateException("the disk is full") }
        state.end { throw IllegalStateException("no web view") }
        // Not only exceptions: an error of the system releases the queue too.
        state.end { throw LinkageError("no web view library") }
        state.write { throw OutOfMemoryError("a page too large") }
        state.always { ran = true }
        thread.all()
        assertTrue("the work after four failures ran", ran)
        assertFalse(thread.queued.isNotEmpty())
    }
}
