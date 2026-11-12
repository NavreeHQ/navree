package com.navree.client.theme

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import kotlin.math.abs

class TonalTest {
    private fun rgb(argb: Int) = Triple(argb shr 16 and 0xFF, argb shr 8 and 0xFF, argb and 0xFF)

    private fun near(expected: Int, actual: Int, within: Int): Boolean {
        val (r1, g1, b1) = rgb(expected)
        val (r2, g2, b2) = rgb(actual)
        return abs(r1 - r2) <= within && abs(g1 - g2) <= within && abs(b1 - b2) <= within
    }

    private fun hex(argb: Int) = "#%06X".format(argb and 0xFFFFFF)

    private fun hueDistance(a: Double, b: Double): Double = abs(((a - b + 540.0) % 360.0) - 180.0)

    @Test
    fun the_primaries_have_their_known_hue_chroma_and_tone() {
        // The values the colour science gives for pure red, green and blue.
        val known = listOf(
            0xFFFF0000.toInt() to Triple(27.4, 113.4, 53.2),
            0xFF00FF00.toInt() to Triple(142.1, 108.4, 87.7),
            0xFF0000FF.toInt() to Triple(282.8, 87.2, 32.3),
        )
        for ((colour, expected) in known) {
            val (hue, chroma) = Hct.hueChroma(colour)
            assertEquals("hue of ${hex(colour)}", expected.first, hue, 0.2)
            assertEquals("chroma of ${hex(colour)}", expected.second, chroma, 0.3)
            assertEquals("tone of ${hex(colour)}", expected.third, Hct.tone(colour), 0.1)
        }
        assertEquals(0.0, Hct.tone(0xFF000000.toInt()), 0.01)
        assertEquals(100.0, Hct.tone(0xFFFFFFFF.toInt()), 0.01)
    }

    @Test
    fun a_colour_survives_the_trip_through_hue_chroma_and_tone() {
        for (colour in listOf(0xFF0F766E, 0xFF6750A4, 0xFFB3261E, 0xFF16A34A, 0xFFF59E0B, 0xFF1D4ED8, 0xFF8A8A8A, 0xFF22C55E)) {
            val argb = colour.toInt()
            val (hue, chroma) = Hct.hueChroma(argb)
            val back = Hct.argb(hue, chroma, Hct.tone(argb))
            assertTrue("${hex(argb)} came back as ${hex(back)}", near(argb, back, within = 3))
        }
    }

    @Test
    fun a_tone_is_kept_exactly_and_chroma_gives_way() {
        for (hue in listOf(0.0, 25.0, 90.0, 145.0, 180.0, 245.0, 300.0)) {
            for (tone in listOf(10, 20, 30, 40, 50, 60, 80, 90, 95, 98)) {
                val colour = Hct.argb(hue, 120.0, tone.toDouble())
                assertEquals("tone at hue $hue", tone.toDouble(), Hct.tone(colour), 0.6)
                val (gotHue, gotChroma) = Hct.hueChroma(colour)
                assertTrue("chroma never goes over what was asked", gotChroma <= 121.0)
                if (gotChroma > 12.0) assertTrue("hue $hue at tone $tone came back as $gotHue", hueDistance(hue, gotHue) < 4.0)
            }
        }
        assertEquals(0xFF000000.toInt(), Hct.argb(120.0, 40.0, 0.0))
        assertEquals(0xFFFFFFFF.toInt(), Hct.argb(120.0, 40.0, 100.0))
    }

    @Test
    fun the_scheme_from_materials_own_seed_is_materials_own_scheme() {
        // What Material's theme builder gives for the seed #6750A4.
        val light = SeedScheme.make(0xFF6750A4.toInt(), dark = false)
        val expected = mapOf(
            "primary" to (0xFF65558F.toInt() to light.primary),
            "secondary" to (0xFF625B71.toInt() to light.secondary),
            "tertiary" to (0xFF7D5260.toInt() to light.tertiary),
            "error" to (0xFFBA1A1A.toInt() to light.error),
            "onSurface" to (0xFF1D1B20.toInt() to light.onSurface),
            "primaryContainer" to (0xFFE9DDFF.toInt() to light.primaryContainer),
        )
        for ((role, pair) in expected) {
            assertTrue("$role should be ${hex(pair.first)} and is ${hex(pair.second)}", near(pair.first, pair.second, within = 6))
        }
        assertEquals(0xFFFFFFFF.toInt(), light.onPrimary)
    }

    @Test
    fun text_is_readable_on_every_pair_of_every_scheme() {
        val seeds = listOf(0xFF0F766E, 0xFF6750A4, 0xFF39FF14, 0xFF000000, 0xFFFFFFFF, 0xFFFF0000, 0xFFFFD600, 0xFF1D4ED8, 0xFF7C2D12, 0xFF808080)
        for (seed in seeds) {
            for (dark in listOf(false, true)) {
                val palette = SeedScheme.make(seed.toInt(), dark)
                for ((name, front, back) in palette.textPairs()) {
                    val contrast = Hct.contrast(front, back)
                    assertTrue("$name with seed ${hex(seed.toInt())}, dark=$dark, is $contrast to 1", contrast >= 4.5)
                }
                assertTrue("an outline can be told from the surface", Hct.contrast(palette.outline, palette.surface) >= 3.0)
            }
        }
    }

    @Test
    fun the_seed_gives_the_hue_and_nothing_else() {
        val teal = SeedScheme.make(0xFF0F766E.toInt(), dark = false)
        val seedHue = Hct.hueChroma(0xFF0F766E.toInt()).first
        assertTrue(hueDistance(seedHue, Hct.hueChroma(teal.primary).first) < 4.0)
        assertEquals(40.0, Hct.tone(teal.primary), 0.6)
        assertEquals(80.0, Hct.tone(SeedScheme.make(0xFF0F766E.toInt(), dark = true).primary), 0.6)
        // A neon seed does not make a neon app: primary is the hue at a moderate chroma.
        val neon = SeedScheme.make(0xFF39FF14.toInt(), dark = false)
        assertTrue(Hct.hueChroma(neon.primary).second < 42.0)
        assertEquals(98.0, Hct.tone(neon.surface), 0.6)
    }

    @Test
    fun the_extra_tones_lean_towards_the_seed_but_stay_what_they_are() {
        assertEquals(145.0, SeedScheme.harmonised(145.0, 145.0), 0.001)
        assertEquals(150.0, SeedScheme.harmonised(145.0, 155.0), 0.001)
        assertEquals(160.0, SeedScheme.harmonised(145.0, 300.0), 0.001)
        assertEquals(130.0, SeedScheme.harmonised(145.0, 20.0), 0.001)
        assertEquals(355.0, SeedScheme.harmonised(5.0, 345.0), 0.001)
        val palette = SeedScheme.make(0xFF6750A4.toInt(), dark = false)
        val green = Hct.hueChroma(palette.onSuccessContainer).first
        assertTrue("success is still a green: $green", green in 120.0..175.0)
    }

    @Test
    fun colours_are_read_from_hex() {
        assertEquals(0xFF0F766E.toInt(), Hct.parse("#0f766e"))
        assertEquals(0xFF0F766E.toInt(), Hct.parse(" 0F766E "))
        assertEquals(0xFFFFAA00.toInt(), Hct.parse("#fa0"))
        assertNull(Hct.parse("teal"))
        assertNull(Hct.parse("#12345"))
        assertNull(Hct.parse(null))
    }
}
