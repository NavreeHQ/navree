package com.navree.client.platform

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class WebRulesTest {
    private fun navigation(ours: Boolean, mainFrame: Boolean, redirect: Boolean, gesture: Boolean, webHost: Boolean = false) =
        WebRules.navigation(ours, webHost, mainFrame, redirect, gesture)

    private fun request(ours: Boolean, mainFrame: Boolean, scheme: String?, accept: String?, webHost: Boolean = false) =
        WebRules.request(ours, webHost, mainFrame, scheme, accept)

    @Test
    fun the_pages_own_navigations_on_the_origins_are_the_cores_to_decide() {
        assertEquals(WebNavigation.Core, navigation(ours = true, mainFrame = true, redirect = false, gesture = true))
        assertEquals(WebNavigation.Core, navigation(ours = true, mainFrame = true, redirect = false, gesture = false))
        assertEquals("a redirect on the origins is part of the load", WebNavigation.Load, navigation(ours = true, mainFrame = true, redirect = true, gesture = false))
    }

    @Test
    fun a_navigation_that_would_leave_is_stopped_and_handed_on_only_when_the_person_started_it() {
        assertEquals(WebNavigation.Core, navigation(ours = false, mainFrame = true, redirect = false, gesture = true))
        assertEquals(WebNavigation.Stop, navigation(ours = false, mainFrame = true, redirect = false, gesture = false))
        assertEquals(WebNavigation.Stop, navigation(ours = false, mainFrame = true, redirect = true, gesture = false))
        assertEquals(WebNavigation.Core, navigation(ours = false, mainFrame = true, redirect = true, gesture = true))
        assertEquals("a web host never holds the page itself", WebNavigation.Stop, navigation(ours = false, mainFrame = true, redirect = false, gesture = false, webHost = true))
    }

    @Test
    fun a_page_redirected_off_the_origins_ends_as_a_failed_screen() {
        assertTrue(WebRules.leftByRedirect(mainFrame = true, redirect = true, navigation = WebNavigation.Stop))
        assertTrue(WebRules.leftByRedirect(mainFrame = true, redirect = true, navigation = WebNavigation.Core))
        assertFalse(WebRules.leftByRedirect(mainFrame = true, redirect = true, navigation = WebNavigation.Load))
        assertFalse("a script's navigation leaves the page as it was", WebRules.leftByRedirect(mainFrame = true, redirect = false, navigation = WebNavigation.Stop))
        assertFalse(WebRules.leftByRedirect(mainFrame = false, redirect = true, navigation = WebNavigation.Stop))
    }

    @Test
    fun a_frame_goes_on_only_on_the_origins_or_a_web_host() {
        assertEquals(WebNavigation.Load, navigation(ours = true, mainFrame = false, redirect = false, gesture = true))
        assertEquals(WebNavigation.Load, navigation(ours = false, mainFrame = false, redirect = false, gesture = false, webHost = true))
        assertEquals(WebNavigation.Stop, navigation(ours = false, mainFrame = false, redirect = false, gesture = true))
        assertEquals(WebNavigation.Stop, navigation(ours = false, mainFrame = false, redirect = true, gesture = false))
    }

    @Test
    fun a_request_elsewhere_is_never_sent() {
        assertEquals(WebRequest.Send, request(ours = true, mainFrame = true, scheme = "https", accept = "text/html"))
        assertEquals(WebRequest.Send, request(ours = true, mainFrame = false, scheme = "https", accept = "image/*"))
        assertEquals("a form posted elsewhere keeps the page", WebRequest.KeepPage, request(ours = false, mainFrame = true, scheme = "https", accept = "text/html"))
        assertEquals(WebRequest.Refuse, request(ours = false, mainFrame = false, scheme = "https", accept = "text/html"))
        assertEquals(WebRequest.Refuse, request(ours = false, mainFrame = false, scheme = "https", accept = "image/*"))
        assertEquals(WebRequest.Refuse, request(ours = false, mainFrame = false, scheme = "file", accept = null))
        assertEquals(WebRequest.Refuse, request(ours = false, mainFrame = false, scheme = "content", accept = null))
    }

    @Test
    fun a_web_host_serves_what_a_page_loads_inside_it_and_never_the_page() {
        assertEquals(WebRequest.Send, request(ours = false, mainFrame = false, scheme = "https", accept = "text/html", webHost = true))
        assertEquals(WebRequest.Send, request(ours = false, mainFrame = false, scheme = "https", accept = "text/css", webHost = true))
        assertEquals(WebRequest.KeepPage, request(ours = false, mainFrame = true, scheme = "https", accept = "text/html", webHost = true))
    }

    @Test
    fun what_a_page_writes_into_itself_loads_unless_it_is_a_page_in_a_frame() {
        assertEquals(WebRequest.Send, request(ours = false, mainFrame = false, scheme = "data", accept = "image/avif,image/webp,*/*"))
        assertEquals(WebRequest.Send, request(ours = false, mainFrame = false, scheme = "blob", accept = null))
        assertEquals(WebRequest.Refuse, request(ours = false, mainFrame = false, scheme = "data", accept = "text/html,application/xhtml+xml"))
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

    @Test
    fun the_document_check_names_every_origin_a_document_may_have_and_nothing_else() {
        val origins = Origins("https://shop.example", also = listOf("https://*.shop.example"), webHosts = listOf("checkout.pay.example", "*.cdn.example:8443"))
        val script = WebRules.documentScript(origins.documentOrigins())
        assertTrue(script, script.contains("""[["https","shop.example",443],["https","*.shop.example",443],["https","checkout.pay.example",443],["https","*.cdn.example",8443]]"""))
        assertTrue("it stops a document that is not one of them", script.contains("window.stop()"))
        assertTrue("and takes out what the parser puts in after", script.contains("new MutationObserver") && script.contains("removeChild"))
        assertTrue("a failure of the check empties the document too", script.endsWith("catch(e){stop()}})();"))
        assertTrue("it is short: it holds up every load", script.length < 1_500)
    }
}
