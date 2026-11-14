package com.navree.client.platform

import org.junit.Assert.assertEquals
import org.junit.Test

class PushTest {
    @Test
    fun a_locked_screen_shows_a_notification_the_systems_way_unless_the_app_asks_for_nothing() {
        assertEquals(LockScreen.System, LockScreen.of(""))
        assertEquals(LockScreen.System, LockScreen.of("  "))
        assertEquals(LockScreen.Hidden, LockScreen.of("nothing"))
        assertEquals("a name this build does not know shows the least", LockScreen.Hidden, LockScreen.of("titles_only"))
    }
}
