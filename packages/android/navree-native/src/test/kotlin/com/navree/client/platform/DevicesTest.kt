package com.navree.client.platform

import kotlinx.serialization.json.Json
import kotlinx.serialization.json.jsonObject
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class DevicesTest {
    private fun key(text: String) = Json.parseToJsonElement(text).jsonObject

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
