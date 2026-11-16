package com.navree.client.design

import kotlin.math.abs
import kotlin.math.max
import kotlin.math.min
import kotlin.math.pow
import kotlin.math.roundToInt

/**
 * A colour field's colours. The value the page holds and the client sends is
 * `#rrggbb` in lower case; a colour here is its 24 bits as an `Int`
 * (`0xRRGGBB`). The person chooses among a palette, moves hue, saturation and
 * lightness, or types the hex value; each is checked here.
 */
object ColourRules {
    /** Hue in degrees (0 up to 360), saturation and lightness in percent (0 to 100). */
    data class Hsl(val hue: Float, val saturation: Float, val lightness: Float)

    /** The colours offered as swatches, five to a row: a spread of hues, a brown, a slate and black. */
    val PALETTE: List<Int> = listOf(
        0xdc2626, 0xea580c, 0xd97706, 0xeab308, 0x65a30d,
        0x16a34a, 0x0d9488, 0x0891b2, 0x2563eb, 0x4f46e5,
        0x7c3aed, 0xdb2777, 0x92400e, 0x475569, 0x000000,
    )

    /** How many swatches stand in a row. */
    const val SWATCHES_PER_ROW = 5

    /**
     * The colour a typed or held value stands for: `#rrggbb`, `rrggbb` or the
     * short `#rgb` (each digit doubled), in either case, with spaces round
     * it. Null for anything else, an empty value too.
     */
    fun parse(text: String?): Int? {
        val digits = text.orEmpty().trim().removePrefix("#")
        if (digits.length != 6 && digits.length != 3) return null
        if (!digits.all { it in '0'..'9' || it in 'a'..'f' || it in 'A'..'F' }) return null
        val full = if (digits.length == 3) digits.map { "$it$it" }.joinToString("") else digits
        return full.toInt(16)
    }

    /** The value a form sends: `#rrggbb`, lower case. */
    fun hex(rgb: Int): String = "#" + Integer.toHexString((rgb and 0xFFFFFF) or 0x1000000).substring(1)

    /** Hue, saturation and lightness of a colour. A grey has no hue of its own: it keeps [keepHue]. */
    fun toHsl(rgb: Int, keepHue: Float = 0f): Hsl {
        val r = ((rgb shr 16) and 0xFF) / 255.0
        val g = ((rgb shr 8) and 0xFF) / 255.0
        val b = (rgb and 0xFF) / 255.0
        val high = max(r, max(g, b))
        val low = min(r, min(g, b))
        val l = (high + low) / 2
        val spread = high - low
        if (spread == 0.0) return Hsl(keepHue, 0f, (l * 100).toFloat())
        val s = spread / (1 - abs(2 * l - 1))
        val h = when (high) {
            r -> ((g - b) / spread).mod(6.0)
            g -> (b - r) / spread + 2
            else -> (r - g) / spread + 4
        } * 60
        return Hsl(h.toFloat(), (s * 100).toFloat(), (l * 100).toFloat())
    }

    /** The colour of a hue, saturation and lightness, each held to its range. */
    fun fromHsl(hsl: Hsl): Int {
        val h = hsl.hue.toDouble().mod(360.0)
        val s = hsl.saturation.coerceIn(0f, 100f) / 100.0
        val l = hsl.lightness.coerceIn(0f, 100f) / 100.0
        val chroma = (1 - abs(2 * l - 1)) * s
        val x = chroma * (1 - abs((h / 60).mod(2.0) - 1))
        val m = l - chroma / 2
        val (r, g, b) = when ((h / 60).toInt()) {
            0 -> Triple(chroma, x, 0.0)
            1 -> Triple(x, chroma, 0.0)
            2 -> Triple(0.0, chroma, x)
            3 -> Triple(0.0, x, chroma)
            4 -> Triple(x, 0.0, chroma)
            else -> Triple(chroma, 0.0, x)
        }
        fun byte(v: Double) = ((v + m) * 255).roundToInt().coerceIn(0, 255)
        return (byte(r) shl 16) or (byte(g) shl 8) or byte(b)
    }

    /** The families a screen reader names a colour by. */
    enum class Family { Black, White, Grey, Red, Orange, Brown, Yellow, Green, Teal, Cyan, Blue, Purple, Pink }

    /** The family a colour is closest to, by its hue, saturation and lightness. */
    fun family(rgb: Int): Family {
        val (h, s, l) = toHsl(rgb)
        return when {
            l <= 8f -> Family.Black
            l >= 94f -> Family.White
            s <= 20f -> Family.Grey
            h < 15f || h >= 345f -> Family.Red
            h < 45f -> if (l < 40f) Family.Brown else Family.Orange
            h < 70f -> Family.Yellow
            h < 160f -> Family.Green
            h < 185f -> Family.Teal
            h < 200f -> Family.Cyan
            h < 255f -> Family.Blue
            h < 290f -> Family.Purple
            else -> Family.Pink
        }
    }

    /** Relative luminance of a colour, 0 (black) to 1 (white), as the contrast guidelines define it. */
    fun luminance(rgb: Int): Double {
        fun channel(shift: Int): Double {
            val c = ((rgb shr shift) and 0xFF) / 255.0
            return if (c <= 0.03928) c / 12.92 else ((c + 0.055) / 1.055).pow(2.4)
        }
        return 0.2126 * channel(16) + 0.7152 * channel(8) + 0.0722 * channel(0)
    }

    /** Black or white, whichever a mark drawn on this colour reads better in. */
    fun onColour(rgb: Int): Int {
        val colour = luminance(rgb)
        val withBlack = (colour + 0.05) / 0.05
        val withWhite = 1.05 / (colour + 0.05)
        return if (withBlack >= withWhite) 0x000000 else 0xFFFFFF
    }
}
