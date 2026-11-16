package com.navree.client

import android.content.pm.ApplicationInfo
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class StartMarksTest {
    @Test
    fun the_marks_of_a_start_are_logged_only_by_a_build_that_can_be_debugged() {
        assertTrue(StartMarks.loggedWith(ApplicationInfo.FLAG_DEBUGGABLE or ApplicationInfo.FLAG_SUPPORTS_RTL))
        assertFalse("a build a person has", StartMarks.loggedWith(ApplicationInfo.FLAG_SUPPORTS_RTL))
        assertFalse(StartMarks.loggedWith(0))
    }
}
