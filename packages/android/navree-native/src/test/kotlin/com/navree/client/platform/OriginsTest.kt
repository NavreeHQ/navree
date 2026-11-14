package com.navree.client.platform

import com.navree.client.Fixtures
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.boolean
import kotlinx.serialization.json.booleanOrNull
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/** One row of the table: an address, the configuration it is read with, and the answer. */
data class AddressCase(val origin: String, val also: List<String>, val loopbackHttp: Boolean, val address: String, val ours: Boolean)

/**
 * Reads the cases of a table of addresses: an object whose list `ours` holds
 * `{"origin", "also"?, "loopback_http"?, "address", "ours"}`.
 */
fun addressCases(text: String): List<AddressCase> =
    Json.parseToJsonElement(text).jsonObject["ours"]!!.jsonArray.map { element ->
        val case = element as JsonObject
        AddressCase(
            origin = case["origin"]!!.jsonPrimitive.content,
            also = (case["also"] as? JsonArray)?.map { it.jsonPrimitive.content }.orEmpty(),
            loopbackHttp = (case["loopback_http"] as? JsonPrimitive)?.booleanOrNull ?: false,
            address = case["address"]!!.jsonPrimitive.content,
            ours = case["ours"]!!.jsonPrimitive.boolean,
        )
    }

class OriginsTest {
    @Test
    fun every_address_of_the_table_is_ours_or_not_as_it_says() {
        val cases = addressCases(Fixtures.text("addresses/ours.json"))
        assertTrue("the table has its cases", cases.size > 40)
        val wrong = cases.filter { Origins(it.origin, it.also, it.loopbackHttp).ours(it.address) != it.ours }
        assertEquals("addresses answered wrongly: ${wrong.map { it.address }}", emptyList<AddressCase>(), wrong)
    }

    @Test
    fun an_origin_that_is_not_https_or_has_more_than_an_origin_is_not_usable() {
        assertTrue(Origins("https://shop.example").usable)
        assertTrue(Origins("https://shop.example/").usable)
        assertFalse(Origins("http://shop.example", loopbackHttp = true).usable)
        assertFalse(Origins("https://shop.example/app").usable)
        assertFalse(Origins("https://user@shop.example").usable)
        assertFalse(Origins("shop.example").usable)
        assertTrue(Origins("http://10.0.2.2:8095", loopbackHttp = true).usable)
        assertFalse(Origins("http://10.0.2.2:8095").usable)
    }

    @Test
    fun pictures_load_from_the_apps_origins_and_the_named_hosts_over_https() {
        val origins = Origins("https://shop.example", pictureHosts = listOf("images.shop-cdn.example", "*.cdn.example"))
        assertEquals("https://shop.example/media/lota.jpg", origins.picture("/media/lota.jpg"))
        assertEquals("https://shop.example/media/lota.jpg", origins.picture("media/lota.jpg"))
        assertEquals("https://images.shop-cdn.example/lota.jpg", origins.picture("https://images.shop-cdn.example/lota.jpg"))
        assertEquals("https://in.cdn.example/lota.jpg", origins.picture("https://in.cdn.example/lota.jpg"))
        assertNull("not over plain http", origins.picture("http://images.shop-cdn.example/lota.jpg"))
        assertNull("not a host the app named", origins.picture("https://evil.example/lota.jpg"))
        assertNull("a name that only ends in the same letters", origins.picture("https://evilcdn.example/lota.jpg"))
        assertNull("another port", origins.picture("https://images.shop-cdn.example:8443/lota.jpg"))
        assertNull(origins.picture("//evil.example/lota.jpg"))
        assertNull(origins.picture("/\\evil.example/lota.jpg"))
        assertNull(origins.picture(" /media/lota.jpg"))
        assertNull(origins.picture(""))
        assertNull("none by default", Origins("https://shop.example").picture("https://images.shop-cdn.example/lota.jpg"))
    }

    @Test
    fun a_web_host_is_for_the_web_view_alone() {
        val origins = Origins("https://shop.example", webHosts = listOf("checkout.pay.example", "*.cdn.example", "maps.example:8443"))
        assertTrue(origins.webHost("https://checkout.pay.example/frame"))
        assertTrue(origins.webHost("https://in.cdn.example/app.js"))
        assertTrue(origins.webHost("https://maps.example:8443/tile"))
        assertFalse("its own port only", origins.webHost("https://maps.example/tile"))
        assertFalse(origins.webHost("https://checkout.pay.example:8443/frame"))
        assertFalse("not over plain http", origins.webHost("http://checkout.pay.example/frame"))
        assertFalse("under the name, not ending in its letters", origins.webHost("https://evilcdn.example/app.js"))
        assertFalse(origins.webHost("https://checkout.pay.example//evil.example/"))
        assertFalse("never ours", origins.ours("https://checkout.pay.example/frame"))
        assertFalse("never a native request", origins.mayLoad("https://checkout.pay.example/frame"))
        assertNull("never a picture", origins.picture("https://checkout.pay.example/logo.png"))
        val local = Origins("http://localhost:8095", loopbackHttp = true, webHosts = listOf("localhost:8097"))
        assertTrue("plain http only to the machine itself, when the app allows it", local.webHost("http://localhost:8097/frame"))
        assertFalse(Origins("https://shop.example", webHosts = listOf("localhost:8097")).webHost("http://localhost:8097/frame"))
        assertFalse("an entry that does not read is none", Origins("https://shop.example", webHosts = listOf("https://pay.example", "pay.example:", "pay example")).webHost("https://pay.example/"))
    }

    @Test
    fun only_six_schemes_are_handed_to_the_system() {
        for (address in listOf("https://pay.example/invoice/7", "http://pay.example/", "mailto:help@shop.example", "tel:+919876543210", "sms:+919876543210", "geo:30.9,75.85", "MAILTO:help@shop.example")) {
            assertTrue(address, handedToSystem(address))
        }
        for (address in listOf("intent://x#Intent;end", "file:///etc/hosts", "javascript:1", "content://x/y", "market://details?id=x", "data:text/html,x", "", " https://pay.example/", "https://pay.example/a\nb", "//pay.example/", "https://user@pay.example/", "tel")) {
            assertFalse(address, handedToSystem(address))
        }
    }

    @Test
    fun only_a_link_that_can_be_the_sign_ins_return_ends_it() {
        val origins = Origins("https://shop.example")
        val callback = "https://shop.example/app/signed-in"
        assertTrue(signInReturn("https://shop.example/app/signed-in?code=c1&state=s1", callback, origins))
        assertTrue(signInReturn("HTTPS://SHOP.EXAMPLE/app/signed-in", callback, origins))
        assertFalse("another path of the origin", signInReturn("https://shop.example/orders/7", callback, origins))
        assertFalse("a longer path", signInReturn("https://shop.example/app/signed-in/more", callback, origins))
        assertFalse("another host", signInReturn("https://evil.example/app/signed-in?code=c1", callback, origins))
        assertFalse(signInReturn("https://shop.example//evil.example/app/signed-in", callback, origins))
        assertTrue("a scheme callback", signInReturn("shop://signed-in?code=c1", "shop", origins))
        assertTrue("an address of the app's own scheme", signInReturn("com.charkha.supply:/signed-in?code=c1", "com.charkha.supply:/signed-in", origins))
        assertFalse(signInReturn("https://shop.example/signed-in", "com.charkha.supply:/signed-in", origins))
        assertFalse(signInReturn("https://shop.example/app/signed-in", "shop", origins))
        assertTrue("no callback: a link of the origin", signInReturn("https://shop.example/app/signed-in", null, origins))
        assertFalse(signInReturn("https://evil.example/", null, origins))
        assertFalse(signInReturn(" shop://signed-in", "shop", origins))
    }

    @Test
    fun the_browser_inside_the_app_takes_only_a_web_address() {
        assertTrue(webAddress("https://shop.example/auth/app/sign-in?state=x"))
        assertTrue(webAddress("http://10.0.2.2:8095/auth/app/sign-in"))
        assertFalse(webAddress("http://shop.example/auth/app/sign-in"))
        assertFalse(webAddress("mailto:help@shop.example"))
        assertFalse(webAddress("shop://signed-in"))
    }
}
