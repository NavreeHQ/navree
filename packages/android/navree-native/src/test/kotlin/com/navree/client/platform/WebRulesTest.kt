package com.navree.client.platform

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class WebRulesTest {
    @Test
    fun the_pages_own_navigations_on_the_origins_are_the_cores_to_decide() {
        assertEquals(WebNavigation.Core, WebRules.navigation(ours = true, mainFrame = true, redirect = false, gesture = true))
        assertEquals(WebNavigation.Core, WebRules.navigation(ours = true, mainFrame = true, redirect = false, gesture = false))
        assertEquals("a redirect on the origins is part of the load", WebNavigation.Load, WebRules.navigation(ours = true, mainFrame = true, redirect = true, gesture = false))
    }

    @Test
    fun a_navigation_that_would_leave_is_stopped_and_handed_on_only_when_the_person_started_it() {
        assertEquals(WebNavigation.Core, WebRules.navigation(ours = false, mainFrame = true, redirect = false, gesture = true))
        assertEquals(WebNavigation.Stop, WebRules.navigation(ours = false, mainFrame = true, redirect = false, gesture = false))
        assertEquals(WebNavigation.Stop, WebRules.navigation(ours = false, mainFrame = true, redirect = true, gesture = false))
        assertEquals(WebNavigation.Core, WebRules.navigation(ours = false, mainFrame = true, redirect = true, gesture = true))
    }

    @Test
    fun a_frame_goes_on_only_on_the_origins() {
        assertEquals(WebNavigation.Load, WebRules.navigation(ours = true, mainFrame = false, redirect = false, gesture = true))
        assertEquals(WebNavigation.Stop, WebRules.navigation(ours = false, mainFrame = false, redirect = false, gesture = true))
        assertEquals(WebNavigation.Stop, WebRules.navigation(ours = false, mainFrame = false, redirect = true, gesture = false))
    }

    @Test
    fun a_request_elsewhere_is_never_sent() {
        assertEquals(WebRequest.Send, WebRules.request(ours = true, mainFrame = true, scheme = "https", accept = "text/html"))
        assertEquals(WebRequest.Send, WebRules.request(ours = true, mainFrame = false, scheme = "https", accept = "image/*"))
        assertEquals("a form posted elsewhere keeps the page", WebRequest.KeepPage, WebRules.request(ours = false, mainFrame = true, scheme = "https", accept = "text/html"))
        assertEquals(WebRequest.Refuse, WebRules.request(ours = false, mainFrame = false, scheme = "https", accept = "text/html"))
        assertEquals(WebRequest.Refuse, WebRules.request(ours = false, mainFrame = false, scheme = "https", accept = "image/*"))
        assertEquals(WebRequest.Refuse, WebRules.request(ours = false, mainFrame = false, scheme = "file", accept = null))
        assertEquals(WebRequest.Refuse, WebRules.request(ours = false, mainFrame = false, scheme = "content", accept = null))
    }

    @Test
    fun what_a_page_writes_into_itself_loads_unless_it_is_a_page_in_a_frame() {
        assertEquals(WebRequest.Send, WebRules.request(ours = false, mainFrame = false, scheme = "data", accept = "image/avif,image/webp,*/*"))
        assertEquals(WebRequest.Send, WebRules.request(ours = false, mainFrame = false, scheme = "blob", accept = null))
        assertEquals(WebRequest.Refuse, WebRules.request(ours = false, mainFrame = false, scheme = "data", accept = "text/html,application/xhtml+xml"))
    }

    @Test
    fun only_a_page_on_the_origins_is_shown() {
        assertTrue(WebRules.shown(ours = true, url = "https://shop.example/"))
        assertTrue(WebRules.shown(ours = false, url = "about:blank"))
        assertFalse(WebRules.shown(ours = false, url = "https://evil.example/"))
        assertFalse(WebRules.shown(ours = false, url = "data:text/html,x"))
    }

    @Test
    fun a_web_process_that_ends_loads_the_page_again_once_then_shows_the_failure() {
        assertTrue(WebRules.reloadAfterGone(1))
        assertFalse(WebRules.reloadAfterGone(2))
    }
}
