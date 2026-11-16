package com.navree.client.platform

import com.navree.client.core.Body
import com.navree.client.core.Effect
import okhttp3.CookieJar
import okhttp3.RequestBody.Companion.toRequestBody
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
    fun a_files_name_in_a_multipart_request_is_never_a_path_a_header_or_a_way_out_of_its_quotes() {
        val names = listOf(
            "Rasid \"Moga\".pdf" to "Rasid %22Moga%22.pdf",
            "Bill\r\nX-Moga: 1\r\n\r\nJalandhar.pdf" to "BillX-Moga: 1Jalandhar.pdf",
            "../../papers/Rasid Ludhiana.pdf" to "Rasid Ludhiana.pdf",
            "C:\\papers\\Rasid.pdf" to "Rasid.pdf",
        )
        for ((given, written) in names) {
            val parts = listOf(
                com.navree.client.core.Part.Text("label", "#16a34a"),
                com.navree.client.core.Part.File("papers", "0123456789abcdef0123456789abcdef", given, "application/pdf"),
                com.navree.client.core.Part.Text("city", "Bathinda"),
            )
            val request = fetchRequest(fetch(method = "post", body = Body.Multipart(parts)), origins, CookieJar.NO_COOKIES, files = { "%PDF".toRequestBody(null) })
            assertNotNull(given, request)
            val text = Buffer().also { request!!.body!!.writeTo(it) }.readUtf8()
            val lines = text.split("\r\n")
            assertEquals("$given: one file part, under the field's name, with the name written as $written: $text", 1, lines.count { it == "Content-Disposition: form-data; name=\"papers\"; filename=\"$written\"" })
            assertEquals("$given: nothing else starts a part: $text", 3, lines.count { it.startsWith("Content-Disposition:") })
            assertEquals("$given: no header of the name's own: $text", 0, lines.count { it.startsWith("X-Moga") })
            assertTrue("$given: the other parts are as they were: $text", text.contains("name=\"label\"\r\nContent-Length: 7\r\n\r\n#16a34a") && text.contains("name=\"city\"\r\nContent-Length: 8\r\n\r\nBathinda"))
            assertTrue("$given: the file's bytes follow its headers: $text", text.contains("\r\n\r\n%PDF\r\n"))
        }
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
    fun an_answers_cache_control_reaches_the_core_and_no_other_header_beside_the_navree_ones() {
        val answer = okhttp3.Headers.headersOf(
            "Content-Type", "application/vnd.navree.native+json",
            "Cache-Control", "private, no-store",
            "X-Navree-Shell", "9f3c",
            "Set-Cookie", "session=s1",
            "Location", "/orders",
            "X-Other", "1",
        )
        val passed = passedHeaders(answer)
        assertEquals(listOf("cache-control" to "private, no-store", "x-navree-shell" to "9f3c"), passed.sortedBy { it.first })
        val event = com.navree.client.core.Events.response(5, 200, "application/vnd.navree.native+json", null, passed, "{}")
        val names = event["headers"]!!.toString()
        assertTrue(names, names.contains("[\"cache-control\",\"private, no-store\"]"))
        assertTrue(names, !names.contains("session") && !names.contains("x-other"))
    }

    @Test
    fun clear_text_is_offered_only_where_the_app_allows_plain_http() {
        assertEquals(1, connectionSpecs(loopbackHttp = false).size)
        assertTrue(connectionSpecs(loopbackHttp = false).single().isTls)
        assertEquals(2, connectionSpecs(loopbackHttp = true).size)
    }
}
