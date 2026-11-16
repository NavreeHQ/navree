package com.navree.client.design

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class ColourRulesTest {
    @Test
    fun a_value_is_read_with_or_without_its_hash_in_either_case_and_in_short_form() {
        assertEquals(0x16a34a, ColourRules.parse("#16a34a"))
        assertEquals(0x16a34a, ColourRules.parse("16A34A"))
        assertEquals(0x16a34a, ColourRules.parse("  #16a34a  "))
        assertEquals("a short value doubles each digit", 0xaabbcc, ColourRules.parse("#abc"))
        assertEquals(0x000000, ColourRules.parse("#000"))
        assertEquals(0xffffff, ColourRules.parse("FFF"))
    }

    @Test
    fun what_is_not_a_colour_is_not_read() {
        for (wrong in listOf(null, "", "#", "#12", "#1234", "#12345", "#1234567", "#12zz34", "+16a34a", "-16a34a", "16 a34a", "#16a34g", "rgb(1,2,3)", "0x16a34a")) {
            assertNull("\"$wrong\" is no colour", ColourRules.parse(wrong))
        }
    }

    @Test
    fun a_colour_is_sent_as_six_lower_case_digits_after_a_hash() {
        assertEquals("#16a34a", ColourRules.hex(0x16a34a))
        assertEquals("#000000", ColourRules.hex(0))
        assertEquals("#00000f", ColourRules.hex(0xf))
        assertEquals("#ffffff", ColourRules.hex(0xffffff))
        assertEquals("only the 24 bits count", "#123456", ColourRules.hex(0x7f123456))
        for (rgb in ColourRules.PALETTE) assertEquals(rgb, ColourRules.parse(ColourRules.hex(rgb)))
    }

    @Test
    fun hue_saturation_and_lightness_give_the_same_colour_back() {
        for (red in 0..255 step 17) for (green in 0..255 step 17) for (blue in 0..255 step 17) {
            val rgb = (red shl 16) or (green shl 8) or blue
            assertEquals(ColourRules.hex(rgb), ColourRules.hex(ColourRules.fromHsl(ColourRules.toHsl(rgb))))
        }
    }

    @Test
    fun the_sliders_ends_and_a_grey_are_what_a_person_expects() {
        assertEquals(0xff0000, ColourRules.fromHsl(ColourRules.Hsl(0f, 100f, 50f)))
        assertEquals(0x00ff00, ColourRules.fromHsl(ColourRules.Hsl(120f, 100f, 50f)))
        assertEquals(0x0000ff, ColourRules.fromHsl(ColourRules.Hsl(240f, 100f, 50f)))
        assertEquals(0xffffff, ColourRules.fromHsl(ColourRules.Hsl(200f, 40f, 100f)))
        assertEquals(0x000000, ColourRules.fromHsl(ColourRules.Hsl(200f, 40f, 0f)))
        assertEquals("a hue past the end wraps", ColourRules.fromHsl(ColourRules.Hsl(10f, 50f, 50f)), ColourRules.fromHsl(ColourRules.Hsl(370f, 50f, 50f)))
        assertEquals("values out of range are held to it", 0xffffff, ColourRules.fromHsl(ColourRules.Hsl(0f, 300f, 400f)))
        val grey = ColourRules.toHsl(0x808080, keepHue = 210f)
        assertEquals("a grey keeps the hue it had, so the hue slider stays where it was", 210f, grey.hue, 0f)
        assertEquals(0f, grey.saturation, 0f)
    }

    @Test
    fun the_palette_has_three_full_rows_of_different_colours_and_holds_the_examples_green() {
        assertEquals(15, ColourRules.PALETTE.size)
        assertEquals(0, ColourRules.PALETTE.size % ColourRules.SWATCHES_PER_ROW)
        assertEquals("no colour twice", ColourRules.PALETTE.size, ColourRules.PALETTE.toSet().size)
        assertTrue(0x16a34a in ColourRules.PALETTE)
    }

    @Test
    fun a_screen_reader_hears_a_family_for_each_colour_and_the_palette_is_told_apart() {
        val families = ColourRules.PALETTE.map { ColourRules.family(it) }
        assertEquals(
            listOf(
                ColourRules.Family.Red, ColourRules.Family.Orange, ColourRules.Family.Orange, ColourRules.Family.Yellow, ColourRules.Family.Green,
                ColourRules.Family.Green, ColourRules.Family.Teal, ColourRules.Family.Cyan, ColourRules.Family.Blue, ColourRules.Family.Blue,
                ColourRules.Family.Purple, ColourRules.Family.Pink, ColourRules.Family.Brown, ColourRules.Family.Grey, ColourRules.Family.Black,
            ),
            families,
        )
        assertEquals(ColourRules.Family.White, ColourRules.family(0xffffff))
        assertEquals(ColourRules.Family.Grey, ColourRules.family(0x808080))
        // The name a swatch is read by is its family and its value, which no two swatches share.
        val names = ColourRules.PALETTE.map { "${ColourRules.family(it)}, ${ColourRules.hex(it)}" }
        assertEquals(names.size, names.toSet().size)
    }

    @Test
    fun a_hex_value_is_one_run_left_to_right_in_a_right_to_left_line_and_as_it_is_in_a_left_to_right_one() {
        assertEquals("\u2066#16a34a\u2069", BidiRules.codes("#16a34a", rtl = true))
        assertEquals("a short value too", "\u2066#ABC\u2069", BidiRules.codes("#ABC", rtl = true))
        assertEquals("#16a34a", BidiRules.codes("#16a34a", rtl = false))
        assertEquals("what is not a colour is left alone", "#12zz", BidiRules.codes("#12zz", rtl = true))
        assertEquals("#1234", BidiRules.codes("#1234", rtl = true))
        assertEquals("a name that starts with a hash is no colour", "#festive", BidiRules.codes("#festive", rtl = true))
        // What a screen reader is told: the value stays one run, so it is read as it is written.
        assertEquals(0x16a34a, ColourRules.parse(BidiRules.codes("#16a34a", rtl = true).filter { it != '\u2066' && it != '\u2069' }))
    }

    @Test
    fun a_mark_on_a_colour_is_black_on_a_light_colour_and_white_on_a_dark_one() {
        assertEquals(0x000000, ColourRules.onColour(0xffffff))
        assertEquals(0x000000, ColourRules.onColour(0xeab308))
        assertEquals(0xffffff, ColourRules.onColour(0x000000))
        assertEquals(0xffffff, ColourRules.onColour(0x2563eb))
        assertEquals("the example's green reads better with black", 0x000000, ColourRules.onColour(0x16a34a))
        for (rgb in ColourRules.PALETTE) {
            val mark = ColourRules.onColour(rgb)
            val ratio = (maxOf(ColourRules.luminance(rgb), ColourRules.luminance(mark)) + 0.05) / (minOf(ColourRules.luminance(rgb), ColourRules.luminance(mark)) + 0.05)
            assertTrue("the mark on ${ColourRules.hex(rgb)} reads at $ratio to 1, below the 3 to 1 a symbol needs", ratio >= 3.0)
        }
        assertEquals(1.0, ColourRules.luminance(0xffffff), 1e-9)
        assertEquals(0.0, ColourRules.luminance(0x000000), 1e-9)
    }
}
