package com.navree.client.platform

import kotlinx.serialization.json.Json
import kotlinx.serialization.json.jsonObject
import okhttp3.HttpUrl.Companion.toHttpUrl
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class DevicesTest {
    private fun key(text: String) = Json.parseToJsonElement(text).jsonObject

    /**
     * A server offers a passkey call only to a build that lists `passkey_options` beside
     * `passkey`: the name says the passkey's request is the one the core hands over.
     */
    @Test
    fun a_build_that_lists_passkey_says_it_takes_the_options_the_core_hands_over() {
        for (pushes in listOf(false, true)) {
            val features = DeviceCalls("https://shop.example/".toHttpUrl(), googleClientId = null, pushes = pushes) {}.features
            assertTrue("passkey" in features)
            assertTrue("passkey_options" in features)
            assertTrue(("push" in features) == pushes)
        }
    }

    @Test
    fun a_confirmation_is_shown_only_when_it_requires_the_person_to_be_verified() {
        assertTrue(confirmable(key("""{"challenge":"c1","rpId":"shop.example","userVerification":"required"}""")))
        assertFalse(confirmable(key("""{"challenge":"c1","rpId":"shop.example","userVerification":"preferred"}""")))
        assertFalse(confirmable(key("""{"challenge":"c1","rpId":"shop.example","userVerification":"discouraged"}""")))
        assertFalse("left out means preferred", confirmable(key("""{"challenge":"c1","rpId":"shop.example"}""")))
        assertFalse(confirmable(key("""{"rpId":"shop.example","userVerification":"required"}""")))
        assertFalse(confirmable(null))
    }
}
