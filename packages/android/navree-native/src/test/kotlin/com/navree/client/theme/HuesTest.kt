package com.navree.client.theme

import com.navree.protocol.Navree
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import kotlin.math.abs

class HuesTest {
    private fun distance(a: Double, b: Double): Double = abs(((a - b + 540.0) % 360.0) - 180.0)

    @Test
    fun the_hues_material_has_roles_for_take_those_roles() {
        assertEquals(Hues.Source.Primary, Hues.source(Navree.Hue.ACCENT))
        assertEquals(Hues.Source.Error, Hues.source(Navree.Hue.DANGER))
        assertEquals(Hues.Source.Outline, Hues.source(Navree.Hue.NEUTRAL))
        assertNull(Hues.palette(Navree.Hue.ACCENT, 260.0))
        for (hue in Navree.Hue.entries - setOf(Navree.Hue.ACCENT, Navree.Hue.DANGER, Navree.Hue.NEUTRAL)) {
            assertEquals("$hue is made as Material makes a custom colour", Hues.Source.Palette, Hues.source(hue))
            assertNotNull(Hues.palette(hue, 260.0))
        }
    }

    @Test
    fun a_status_and_the_hue_it_is_named_after_share_a_colour() {
        assertEquals(Hues.angle(Navree.Hue.GREEN), Hues.angle(Navree.Hue.SUCCESS))
        assertEquals(Hues.angle(Navree.Hue.AMBER), Hues.angle(Navree.Hue.WARNING))
        assertEquals(Hues.angle(Navree.Hue.BLUE), Hues.angle(Navree.Hue.INFO))
    }

    @Test
    fun a_hue_turns_towards_the_theme_but_stays_itself() {
        for (hue in Navree.Hue.entries) {
            val angle = Hues.angle(hue) ?: continue
            for (primary in listOf(0.0, 90.0, 180.0, 270.0)) {
                val palette = Hues.palette(hue, primary)!!
                assertTrue("$hue moves at most 15 degrees towards $primary", distance(palette.hue, angle) <= 15.0 + 1e-9)
            }
        }
    }

    @Test
    fun the_vivid_hues_are_far_enough_apart_to_tell_from_each_other() {
        val vivid = listOf(Navree.Hue.BLUE, Navree.Hue.GREEN, Navree.Hue.AMBER, Navree.Hue.ORANGE, Navree.Hue.RED, Navree.Hue.ROSE, Navree.Hue.VIOLET, Navree.Hue.CYAN)
        for (a in vivid) for (b in vivid) {
            if (a == b || setOf(a, b) == setOf(Navree.Hue.ORANGE, Navree.Hue.RED)) continue
            assertTrue("$a and $b", distance(Hues.angle(a)!!, Hues.angle(b)!!) >= 25.0)
        }
        assertTrue("slate is a grey with a trace of blue", Hues.chroma(Navree.Hue.SLATE) < Hues.chroma(Navree.Hue.BLUE) / 2)
    }

    @Test
    fun marks_stand_out_from_the_surface_in_light_and_in_dark() {
        // A mark (an arc, a bar) needs 3:1 against the surface it is drawn on, as Material asks of graphics.
        for (dark in listOf(false, true)) {
            val surface = SeedScheme.make(0xFF3F51B5.toInt(), dark).surfaceContainerLow
            for (hue in Navree.Hue.entries) {
                val palette = Hues.palette(hue, 260.0) ?: continue
                val mark = palette.tone(Hues.markTone(dark))
                assertTrue("$hue in ${if (dark) "dark" else "light"}: ${Hct.contrast(mark, surface)}", Hct.contrast(mark, surface) >= 3.0)
            }
        }
    }

    @Test
    fun a_track_is_the_hues_faintest_tone_and_a_mark_its_bright_one() {
        assertEquals(90, Hues.trackTone(dark = false))
        assertEquals(25, Hues.trackTone(dark = true))
        assertTrue(Hues.markTone(dark = false) < Hues.trackTone(dark = false))
        assertTrue(Hues.markTone(dark = true) > Hues.trackTone(dark = true))
        assertEquals("four shades, faint to strong", 4, Hues.shadeTones(false).size)
        assertTrue(Hues.shadeTones(false).zipWithNext().all { (a, b) -> a > b })
        assertTrue(Hues.shadeTones(true).zipWithNext().all { (a, b) -> a < b })
    }
}
