package com.navree.client.platform

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class WorkerRulesTest {
    private val shop = Origins("https://shop.example")
    private val said = mutableListOf<String>()
    private val told = Told { said += it }

    /** A way of making the rules that fails [failures] times, then works; [made] holds the origins each try was for. */
    private class Maker(private var failures: Int, private val error: Throwable = UnsupportedOperationException("no controller")) {
        val made = mutableListOf<Origins>()

        fun make(origins: Origins) {
            made += origins
            if (failures > 0) {
                failures--
                throw error
            }
        }
    }

    @Test
    fun nothing_is_made_before_the_guard_says_which_origins_it_is_for() {
        val maker = Maker(0)
        val rules = WorkerRules(maker::make, told)
        assertFalse(rules.ensure())
        assertEquals(emptyList<Origins>(), maker.made)
        assertEquals("not being told is said, once, in the log: $said", 1, said.size)
        assertTrue(said.single(), said.single().contains("not in force"))
        assertFalse(rules.ensure())
        assertEquals("and not again for the next page", 1, said.size)
    }

    @Test
    fun a_guard_that_is_made_is_in_force_and_is_made_once() {
        val maker = Maker(0)
        val rules = WorkerRules(maker::make, told)
        rules.arm(shop)
        assertTrue(rules.ensure())
        assertTrue(rules.ensure())
        assertTrue(rules.ensure())
        assertEquals(listOf(shop), maker.made)
        assertEquals(emptyList<String>(), said)
    }

    @Test
    fun a_guard_that_could_not_be_made_is_not_in_force_and_is_tried_again_by_the_next_page() {
        val maker = Maker(failures = 2)
        val rules = WorkerRules(maker::make, told)
        rules.arm(shop)
        assertFalse("the first try fails", rules.ensure())
        assertFalse("a page made after it asks again, and fails again", rules.ensure())
        assertTrue("the third asks again and it is made", rules.ensure())
        assertEquals("every ask made it", listOf(shop, shop, shop), maker.made)
        assertTrue("once made it stays made", rules.ensure())
        assertEquals(3, maker.made.size)
    }

    @Test
    fun a_failure_is_told_once_with_what_failed() {
        val maker = Maker(failures = 3)
        val rules = WorkerRules(maker::make, told)
        rules.arm(shop)
        repeat(3) { rules.ensure() }
        assertEquals("three failures, one line: $said", 1, said.size)
        assertTrue(said.single(), said.single().contains("no controller"))
        assertTrue("it says what is not checked: ${said.single()}", said.single().contains("service worker"))
    }

    @Test
    fun a_failure_that_is_an_error_and_not_an_exception_counts_as_a_failure_too() {
        val maker = Maker(failures = 1, error = LinkageError("the engine is not there"))
        val rules = WorkerRules(maker::make, told)
        rules.arm(shop)
        assertFalse(rules.ensure())
        assertTrue(rules.ensure())
        assertTrue(said.single(), said.single().contains("the engine is not there"))
    }

    @Test
    fun arming_again_makes_the_guard_again_for_the_new_origins_and_tells_a_new_failure() {
        val maker = Maker(failures = 1)
        val rules = WorkerRules(maker::make, told)
        rules.arm(shop)
        assertFalse(rules.ensure())
        val other = Origins("https://other.example")
        rules.arm(other)
        assertTrue(rules.ensure())
        assertEquals(listOf(shop, other), maker.made)
        val again = Maker(failures = 1)
        val rules2 = WorkerRules(again::make, told)
        rules2.arm(other)
        assertFalse(rules2.ensure())
        assertEquals("a new arming that fails is told again: $said", 2, said.size)
    }

    @Test
    fun a_thing_is_told_once_and_each_thing_on_its_own() {
        told.once("workers", "first")
        told.once("workers", "again")
        told.once("documents", "other")
        assertEquals(listOf("first", "other"), said)
        told.again("workers")
        told.once("workers", "third")
        assertEquals(listOf("first", "other", "third"), said)
    }

    @Test
    fun the_guard_is_made_when_the_main_thread_is_idle_and_the_handler_is_not_kept() {
        val maker = Maker(0)
        val rules = WorkerRules(maker::make, told)
        rules.arm(shop)
        var handler: (() -> Boolean)? = null
        rules.whenIdle { handler = it }
        assertEquals("nothing is made until the main thread is idle", emptyList<Origins>(), maker.made)
        assertEquals("the handler answers that it is not wanted again", false, handler!!())
        assertEquals(listOf(shop), maker.made)
        assertTrue("a web page made after it finds the guard in force", rules.ensure())
        assertEquals("and does not make it again", listOf(shop), maker.made)
    }

    @Test
    fun a_guard_that_the_idle_handler_could_not_make_is_tried_again_by_the_next_page() {
        val maker = Maker(failures = 1)
        val rules = WorkerRules(maker::make, told)
        rules.arm(shop)
        var handler: (() -> Boolean)? = null
        rules.whenIdle { handler = it }
        assertEquals(false, handler!!())
        assertEquals("one failure, told once", 1, said.size)
        assertTrue(rules.ensure())
        assertEquals(listOf(shop, shop), maker.made)
    }

    @Test
    fun an_idle_handler_before_the_guard_is_armed_makes_nothing_and_does_not_fail() {
        val maker = Maker(0)
        val rules = WorkerRules(maker::make, told)
        var handler: (() -> Boolean)? = null
        rules.whenIdle { handler = it }
        assertEquals(false, handler!!())
        assertEquals(emptyList<Origins>(), maker.made)
    }
}
