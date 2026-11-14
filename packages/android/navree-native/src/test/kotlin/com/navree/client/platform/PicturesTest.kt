package com.navree.client.platform

import org.junit.Assert.assertEquals
import org.junit.Test

class PicturesTest {
    @Test
    fun a_picture_is_sampled_down_until_no_side_is_longer_than_the_limit() {
        assertEquals(1, sampleSize(1200, 800))
        assertEquals(1, sampleSize(2048, 2048))
        assertEquals(2, sampleSize(4000, 3000))
        assertEquals(32, sampleSize(65_000, 10))
        assertEquals(64, sampleSize(100_000, 100_000))
        assertEquals(2, sampleSize(10, 4097))
    }

    @Test
    fun a_picture_host_gets_no_cookie_and_keeps_none() {
        val jar = object : okhttp3.CookieJar {
            override fun saveFromResponse(url: okhttp3.HttpUrl, cookies: List<okhttp3.Cookie>) = Unit

            override fun loadForRequest(url: okhttp3.HttpUrl): List<okhttp3.Cookie> =
                listOfNotNull(okhttp3.Cookie.parse(url, "session=s1; Domain=shop.example"))
        }
        val origins = Origins("https://shop.example", pictureHosts = listOf("images.shop.example"))
        val ours = pictureRequest("https://shop.example/media/lota.jpg", origins, jar)!!
        assertEquals("session=s1", ours.header("Cookie"))
        assertEquals("image/*", ours.header("Accept"))
        val host = pictureRequest("https://images.shop.example/lota.jpg", origins, jar)!!
        assertEquals("not even a cookie of the parent domain", null, host.header("Cookie"))
        assertEquals("image/*", host.header("Accept"))
        assertEquals("no session mark: nothing the answer sets is kept", null, host.tag(Session::class.java))
        assertEquals(null, ours.tag(Session::class.java))
    }

    @Test
    fun a_picture_whose_size_does_not_read_is_not_decoded() {
        assertEquals(0, sampleSize(-1, -1))
        assertEquals(0, sampleSize(0, 600))
    }
}
