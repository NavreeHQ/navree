package com.navree.client.platform

import com.navree.client.core.Body
import com.navree.client.core.Effect
import okhttp3.CookieJar
import okio.Buffer
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class HttpTest {
    private val origins = Origins("https://shop.example", also = listOf("https://*.shop.example"))

    private fun fetch(url: String = "https://shop.example/orders", headers: List<Pair<String, String>> = emptyList(), method: String = "get", body: Body? = null) =
        Effect.Fetch(request = 5, method = method, url = url, headers = headers, body = body, timeoutMs = 30_000, xsrf = false)

    @Test
    fun a_request_on_the_apps_origins_is_built_with_its_headers() {
        val request = fetchRequest(fetch(headers = listOf("x-navree-region" to "orders", "accept" to "application/vnd.navree.native+json")), origins, CookieJar.NO_COOKIES)
        assertNotNull(request)
        assertEquals("orders", request!!.header("x-navree-region"))
        assertEquals("GET", request.method)
        assertNotNull(fetchRequest(fetch(url = "https://patiala.shop.example/orders"), origins, CookieJar.NO_COOKIES))
    }

    @Test
    fun a_header_value_http_cannot_carry_is_a_request_not_sent() {
        assertNull("a region named in Devanagari", fetchRequest(fetch(headers = listOf("x-navree-region" to "बिक्री")), origins, CookieJar.NO_COOKIES))
        assertNull("a line break", fetchRequest(fetch(headers = listOf("x-navree-region" to "orders\r\nx-other: 1")), origins, CookieJar.NO_COOKIES))
        assertNull("a line break in a name", fetchRequest(fetch(headers = listOf("x-navree\nregion" to "orders")), origins, CookieJar.NO_COOKIES))
    }

    @Test
    fun an_address_that_is_not_ours_is_a_request_not_sent() {
        for (url in listOf("//evil.example/x", "https://shop.example//evil.example/x", "https://evil.example/", "http://shop.example/", "", "https://shop.example/a b")) {
            assertNull(url, fetchRequest(fetch(url = url), origins, CookieJar.NO_COOKIES))
        }
    }

    @Test
    fun a_method_that_cannot_carry_its_body_is_a_request_not_sent() {
        assertNull(fetchRequest(fetch(method = "get", body = Body.Form("a=1")), origins, CookieJar.NO_COOKIES))
        assertNotNull(fetchRequest(fetch(method = "post", body = Body.Form("a=1"), headers = listOf("content-type" to "application/x-www-form-urlencoded")), origins, CookieJar.NO_COOKIES))
    }

    @Test
    fun a_stream_is_asked_only_of_the_apps_origins_with_headers_http_can_carry() {
        assertNotNull(streamRequest(Effect.StreamOpen(1, "https://shop.example/broadcasting/stream?topics=orders", listOf("x-navree-native" to "1")), origins))
        assertNull(streamRequest(Effect.StreamOpen(1, "https://evil.example/stream", emptyList()), origins))
        assertNull(streamRequest(Effect.StreamOpen(1, "https://shop.example/stream", listOf("x-navree-region" to "बिक्री")), origins))
    }

    @Test
    fun a_body_is_read_whole_up_to_its_limit_and_not_past_it() {
        val small = Buffer().write(ByteArray(10) { 7 })
        assertArrayEquals(ByteArray(10) { 7 }, readCapped(small, 10))
        val large = Buffer().write(ByteArray(11))
        assertNull(readCapped(large, 10))
        assertEquals("nothing is taken from a body past its limit", 11L, large.size)
    }

    @Test
    fun clear_text_is_offered_only_where_the_app_allows_plain_http() {
        assertEquals(1, connectionSpecs(loopbackHttp = false).size)
        assertTrue(connectionSpecs(loopbackHttp = false).single().isTls)
        assertEquals(2, connectionSpecs(loopbackHttp = true).size)
    }
}
