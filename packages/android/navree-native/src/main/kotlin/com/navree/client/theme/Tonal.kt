package com.navree.client.theme

import kotlin.math.abs
import kotlin.math.atan2
import kotlin.math.cbrt
import kotlin.math.cos
import kotlin.math.exp
import kotlin.math.hypot
import kotlin.math.max
import kotlin.math.min
import kotlin.math.pow
import kotlin.math.roundToInt
import kotlin.math.sign
import kotlin.math.sin
import kotlin.math.sqrt

/**
 * Hue, chroma and tone: the colour space Material 3 builds its palettes in.
 * Hue and chroma come from the CAM16 colour appearance model under standard
 * viewing conditions; tone is CIE L*. Two colours with tones 50 apart have a
 * contrast of at least 4.5 to 1, whatever their hue, which is what lets a
 * scheme be made from any seed and still be readable.
 */
object Hct {
    private val SRGB_TO_XYZ = arrayOf(
        doubleArrayOf(0.41233895, 0.35762064, 0.18051042),
        doubleArrayOf(0.2126, 0.7152, 0.0722),
        doubleArrayOf(0.01932141, 0.11916382, 0.95034478),
    )
    private val XYZ_TO_SRGB = arrayOf(
        doubleArrayOf(3.2413774792388685, -1.5376652402851851, -0.49885366846268053),
        doubleArrayOf(-0.9691452513005321, 1.8758853451067872, 0.04156585616912061),
        doubleArrayOf(0.05562093689691305, -0.20395524564742123, 1.0571799111220335),
    )
    private val XYZ_TO_CAM16 = arrayOf(
        doubleArrayOf(0.401288, 0.650173, -0.051461),
        doubleArrayOf(-0.250268, 1.204414, 0.045854),
        doubleArrayOf(-0.002079, 0.048952, 0.953127),
    )
    private val CAM16_TO_XYZ = arrayOf(
        doubleArrayOf(1.8620678, -1.0112547, 0.14918678),
        doubleArrayOf(0.38752654, 0.62144744, -0.00897398),
        doubleArrayOf(-0.01584150, -0.03412294, 1.0499644),
    )
    private val WHITE = doubleArrayOf(95.047, 100.0, 108.883)

    // Standard viewing conditions: D65, a mid-grey background, an average surround.
    private val adaptingLuminance = 200.0 / Math.PI * yFromTone(50.0) / 100.0
    private val n = yFromTone(50.0) / WHITE[1]
    private val z = 1.48 + sqrt(n)
    private val nbb = 0.725 / n.pow(0.2)
    private const val SURROUND_C = 0.69
    private const val NC = 1.0
    private val rgbD: DoubleArray
    private val fl: Double
    private val aw: Double

    init {
        val white = times(XYZ_TO_CAM16, WHITE)
        val d = (1.0 - (1.0 / 3.6) * exp((-adaptingLuminance - 42.0) / 92.0)).coerceIn(0.0, 1.0)
        rgbD = DoubleArray(3) { d * (100.0 / white[it]) + 1.0 - d }
        val k = 1.0 / (5.0 * adaptingLuminance + 1.0)
        val k4 = k * k * k * k
        fl = k4 * adaptingLuminance + 0.1 * (1.0 - k4) * (1.0 - k4) * cbrt(5.0 * adaptingLuminance)
        val adapted = DoubleArray(3) {
            val factor = (fl * rgbD[it] * white[it] / 100.0).pow(0.42)
            400.0 * factor / (factor + 27.13)
        }
        aw = (2.0 * adapted[0] + adapted[1] + 0.05 * adapted[2]) * nbb
    }

    private fun times(matrix: Array<DoubleArray>, v: DoubleArray): DoubleArray = DoubleArray(3) {
        matrix[it][0] * v[0] + matrix[it][1] * v[1] + matrix[it][2] * v[2]
    }

    private fun linear(channel: Int): Double {
        val normal = channel / 255.0
        return if (normal <= 0.040449936) normal / 12.92 * 100.0 else ((normal + 0.055) / 1.055).pow(2.4) * 100.0
    }

    private fun channel(linear: Double): Int {
        val normal = linear / 100.0
        val gamma = if (normal <= 0.0031308) normal * 12.92 else 1.055 * normal.pow(1.0 / 2.4) - 0.055
        return (gamma * 255.0).roundToInt().coerceIn(0, 255)
    }

    private fun xyz(argb: Int): DoubleArray = times(
        SRGB_TO_XYZ,
        doubleArrayOf(linear(argb shr 16 and 0xFF), linear(argb shr 8 and 0xFF), linear(argb and 0xFF)),
    )

    private const val E = 216.0 / 24389.0
    private const val KAPPA = 24389.0 / 27.0

    /** The luminance Y, from 0 to 100, of a tone. */
    fun yFromTone(tone: Double): Double {
        val ft = (tone + 16.0) / 116.0
        val cube = ft * ft * ft
        return 100.0 * if (cube > E) cube else (116.0 * ft - 16.0) / KAPPA
    }

    private fun toneFromY(y: Double): Double {
        val t = y / 100.0
        return 116.0 * (if (t > E) cbrt(t) else (KAPPA * t + 16.0) / 116.0) - 16.0
    }

    /** The tone (L*) of a colour, from 0 for black to 100 for white. */
    fun tone(argb: Int): Double = toneFromY(xyz(argb)[1])

    /** The hue, in degrees, and the chroma of a colour. */
    fun hueChroma(argb: Int): Pair<Double, Double> {
        val cone = times(XYZ_TO_CAM16, xyz(argb))
        val adapted = DoubleArray(3) {
            val discounted = rgbD[it] * cone[it]
            val factor = (fl * abs(discounted) / 100.0).pow(0.42)
            sign(discounted) * 400.0 * factor / (factor + 27.13)
        }
        val a = (11.0 * adapted[0] - 12.0 * adapted[1] + adapted[2]) / 11.0
        val b = (adapted[0] + adapted[1] - 2.0 * adapted[2]) / 9.0
        val u = (20.0 * adapted[0] + 20.0 * adapted[1] + 21.0 * adapted[2]) / 20.0
        val p2 = (40.0 * adapted[0] + 20.0 * adapted[1] + adapted[2]) / 20.0
        val degrees = Math.toDegrees(atan2(b, a))
        val hue = if (degrees < 0) degrees + 360.0 else if (degrees >= 360.0) degrees - 360.0 else degrees
        val j = 100.0 * (p2 * nbb / aw).pow(SURROUND_C * z)
        val huePrime = if (hue < 20.14) hue + 360.0 else hue
        val eHue = 0.25 * (cos(Math.toRadians(huePrime) + 2.0) + 3.8)
        val p1 = 50000.0 / 13.0 * eHue * NC * nbb
        val t = p1 * hypot(a, b) / (u + 0.305)
        val alpha = (1.64 - 0.29.pow(n)).pow(0.73) * t.pow(0.9)
        return hue to alpha * sqrt(j / 100.0)
    }

    /** XYZ of a colour given by CAM16 lightness J, chroma and hue. */
    private fun xyzFromJch(j: Double, chroma: Double, hue: Double): DoubleArray {
        val alpha = if (chroma == 0.0 || j == 0.0) 0.0 else chroma / sqrt(j / 100.0)
        val t = (alpha / (1.64 - 0.29.pow(n)).pow(0.73)).pow(1.0 / 0.9)
        val radians = Math.toRadians(hue)
        val eHue = 0.25 * (cos(radians + 2.0) + 3.8)
        val ac = aw * (j / 100.0).pow(1.0 / SURROUND_C / z)
        val p1 = eHue * (50000.0 / 13.0) * NC * nbb
        val p2 = ac / nbb
        val hSin = sin(radians)
        val hCos = cos(radians)
        val gamma = 23.0 * (p2 + 0.305) * t / (23.0 * p1 + 11.0 * t * hCos + 108.0 * t * hSin)
        val a = gamma * hCos
        val b = gamma * hSin
        val adapted = doubleArrayOf(
            (460.0 * p2 + 451.0 * a + 288.0 * b) / 1403.0,
            (460.0 * p2 - 891.0 * a - 261.0 * b) / 1403.0,
            (460.0 * p2 - 220.0 * a - 6300.0 * b) / 1403.0,
        )
        val cone = DoubleArray(3) {
            val base = max(0.0, 27.13 * abs(adapted[it]) / (400.0 - abs(adapted[it])))
            sign(adapted[it]) * (100.0 / fl) * base.pow(1.0 / 0.42) / rgbD[it]
        }
        return times(CAM16_TO_XYZ, cone)
    }

    /** Linear sRGB of the colour with this hue and chroma whose luminance is `y`, or null when sRGB has no such colour. */
    private fun linearRgb(hue: Double, chroma: Double, y: Double): DoubleArray? {
        var low = 0.0
        var high = 100.0
        repeat(48) {
            val mid = (low + high) / 2.0
            if (xyzFromJch(mid, chroma, hue)[1] < y) low = mid else high = mid
        }
        val xyz = xyzFromJch((low + high) / 2.0, chroma, hue)
        if (xyz.any { it.isNaN() } || abs(xyz[1] - y) > 0.05) return null
        val rgb = times(XYZ_TO_SRGB, xyz)
        return if (rgb.all { it >= -0.03 && it <= 100.03 }) rgb else null
    }

    private fun argb(linear: DoubleArray): Int =
        (0xFF shl 24) or (channel(linear[0]) shl 16) or (channel(linear[1]) shl 8) or channel(linear[2])

    private fun grey(tone: Double): Int {
        val value = channel(yFromTone(tone))
        return (0xFF shl 24) or (value shl 16) or (value shl 8) or value
    }

    /**
     * The sRGB colour with this hue and tone and as much of this chroma as
     * sRGB can show at that tone. The tone is kept exactly; chroma gives way.
     */
    fun argb(hue: Double, chroma: Double, tone: Double): Int {
        if (tone <= 0.01) return 0xFF000000.toInt()
        if (tone >= 99.99) return 0xFFFFFFFF.toInt()
        if (chroma < 0.5) return grey(tone)
        val y = yFromTone(tone)
        val wrapped = ((hue % 360.0) + 360.0) % 360.0
        linearRgb(wrapped, chroma, y)?.let { return argb(it) }
        var low = 0.0
        var high = chroma
        var best: DoubleArray? = null
        repeat(24) {
            val mid = (low + high) / 2.0
            val rgb = linearRgb(wrapped, mid, y)
            if (rgb != null) {
                best = rgb
                low = mid
            } else {
                high = mid
            }
        }
        return best?.let { argb(it) } ?: grey(tone)
    }

    /** The contrast ratio between two colours, from 1 to 21. */
    fun contrast(first: Int, second: Int): Double {
        val a = xyz(first)[1]
        val b = xyz(second)[1]
        return (max(a, b) + 5.0) / (min(a, b) + 5.0)
    }

    /** Parses `#rrggbb` or `#rgb`. */
    fun parse(text: String?): Int? {
        val hex = text?.trim()?.removePrefix("#") ?: return null
        val full = when (hex.length) {
            3 -> hex.map { "$it$it" }.joinToString("")
            6 -> hex
            else -> return null
        }
        return full.toLongOrNull(16)?.let { (0xFF000000 or it).toInt() }
    }
}

/** One hue at one chroma, at any tone. */
class TonalPalette(val hue: Double, val chroma: Double) {
    private val tones = HashMap<Int, Int>()

    fun tone(tone: Int): Int = tones.getOrPut(tone) { Hct.argb(hue, chroma, tone.toDouble()) }
}

/**
 * A Material 3 colour scheme as plain numbers, so it can be made and checked
 * without a device. Each value is ARGB.
 */
data class Palette(
    val dark: Boolean,
    val primary: Int,
    val onPrimary: Int,
    val primaryContainer: Int,
    val onPrimaryContainer: Int,
    val inversePrimary: Int,
    val secondary: Int,
    val onSecondary: Int,
    val secondaryContainer: Int,
    val onSecondaryContainer: Int,
    val tertiary: Int,
    val onTertiary: Int,
    val tertiaryContainer: Int,
    val onTertiaryContainer: Int,
    val error: Int,
    val onError: Int,
    val errorContainer: Int,
    val onErrorContainer: Int,
    val surface: Int,
    val onSurface: Int,
    val surfaceVariant: Int,
    val onSurfaceVariant: Int,
    val surfaceDim: Int,
    val surfaceBright: Int,
    val surfaceContainerLowest: Int,
    val surfaceContainerLow: Int,
    val surfaceContainer: Int,
    val surfaceContainerHigh: Int,
    val surfaceContainerHighest: Int,
    val inverseSurface: Int,
    val inverseOnSurface: Int,
    val outline: Int,
    val outlineVariant: Int,
    val scrim: Int,
    // The roles that keep one tone in light and in dark.
    val primaryFixed: Int,
    val primaryFixedDim: Int,
    val onPrimaryFixed: Int,
    val onPrimaryFixedVariant: Int,
    val secondaryFixed: Int,
    val secondaryFixedDim: Int,
    val onSecondaryFixed: Int,
    val onSecondaryFixedVariant: Int,
    val tertiaryFixed: Int,
    val tertiaryFixedDim: Int,
    val onTertiaryFixed: Int,
    val onTertiaryFixedVariant: Int,
    // Colours Material 3 has no role for. They are made the way it makes its own.
    val successContainer: Int,
    val onSuccessContainer: Int,
    val warningContainer: Int,
    val onWarningContainer: Int,
    val infoContainer: Int,
    val onInfoContainer: Int,
) {
    /** Every pair of a colour and what is drawn on it, by name. Text on each must be readable. */
    fun textPairs(): List<Triple<String, Int, Int>> = listOf(
        Triple("onPrimary/primary", onPrimary, primary),
        Triple("onPrimaryContainer/primaryContainer", onPrimaryContainer, primaryContainer),
        Triple("onSecondary/secondary", onSecondary, secondary),
        Triple("onSecondaryContainer/secondaryContainer", onSecondaryContainer, secondaryContainer),
        Triple("onTertiary/tertiary", onTertiary, tertiary),
        Triple("onTertiaryContainer/tertiaryContainer", onTertiaryContainer, tertiaryContainer),
        Triple("onError/error", onError, error),
        Triple("onErrorContainer/errorContainer", onErrorContainer, errorContainer),
        Triple("onSurface/surface", onSurface, surface),
        Triple("onSurface/surfaceContainerHighest", onSurface, surfaceContainerHighest),
        Triple("onSurfaceVariant/surface", onSurfaceVariant, surface),
        Triple("onSurfaceVariant/surfaceContainerHighest", onSurfaceVariant, surfaceContainerHighest),
        Triple("inverseOnSurface/inverseSurface", inverseOnSurface, inverseSurface),
        Triple("primary/surface", primary, surface),
        Triple("error/surface", error, surface),
        Triple("onSuccessContainer/successContainer", onSuccessContainer, successContainer),
        Triple("onWarningContainer/warningContainer", onWarningContainer, warningContainer),
        Triple("onInfoContainer/infoContainer", onInfoContainer, infoContainer),
    )
}

/**
 * A scheme from one seed colour, the way Material 3 makes its default one
 * (the "tonal spot" variant): the seed gives the hue; primary is that hue at
 * a moderate chroma, secondary the same hue muted, tertiary a hue 60 degrees
 * on, and the neutrals carry a trace of the hue. Roles are tones of those
 * palettes, the same tones for every seed.
 *
 * The brand tints the system. It does not pick each role: a seed that is
 * very dark, very light or very vivid still gives a scheme with the contrast
 * Material components expect.
 */
object SeedScheme {
    fun make(seed: Int, dark: Boolean): Palette {
        val hue = Hct.hueChroma(seed).first
        val primary = TonalPalette(hue, 36.0)
        val secondary = TonalPalette(hue, 16.0)
        val tertiary = TonalPalette(hue + 60.0, 24.0)
        val neutral = TonalPalette(hue, 6.0)
        val variant = TonalPalette(hue, 8.0)
        val error = TonalPalette(25.0, 84.0)
        val success = TonalPalette(harmonised(145.0, hue), 40.0)
        val warning = TonalPalette(harmonised(80.0, hue), 56.0)
        val info = TonalPalette(harmonised(245.0, hue), 40.0)
        fun pick(palette: TonalPalette, light: Int, night: Int): Int = palette.tone(if (dark) night else light)
        return Palette(
            dark = dark,
            primary = pick(primary, 40, 80),
            onPrimary = pick(primary, 100, 20),
            primaryContainer = pick(primary, 90, 30),
            onPrimaryContainer = pick(primary, 30, 90),
            inversePrimary = pick(primary, 80, 40),
            secondary = pick(secondary, 40, 80),
            onSecondary = pick(secondary, 100, 20),
            secondaryContainer = pick(secondary, 90, 30),
            onSecondaryContainer = pick(secondary, 30, 90),
            tertiary = pick(tertiary, 40, 80),
            onTertiary = pick(tertiary, 100, 20),
            tertiaryContainer = pick(tertiary, 90, 30),
            onTertiaryContainer = pick(tertiary, 30, 90),
            error = pick(error, 40, 80),
            onError = pick(error, 100, 20),
            errorContainer = pick(error, 90, 30),
            onErrorContainer = pick(error, 30, 90),
            surface = pick(neutral, 98, 6),
            onSurface = pick(neutral, 10, 90),
            surfaceVariant = pick(variant, 90, 30),
            onSurfaceVariant = pick(variant, 30, 80),
            surfaceDim = pick(neutral, 87, 6),
            surfaceBright = pick(neutral, 98, 24),
            surfaceContainerLowest = pick(neutral, 100, 4),
            surfaceContainerLow = pick(neutral, 96, 10),
            surfaceContainer = pick(neutral, 94, 12),
            surfaceContainerHigh = pick(neutral, 92, 17),
            surfaceContainerHighest = pick(neutral, 90, 22),
            inverseSurface = pick(neutral, 20, 90),
            inverseOnSurface = pick(neutral, 95, 20),
            outline = pick(variant, 50, 60),
            outlineVariant = pick(variant, 80, 30),
            scrim = neutral.tone(0),
            primaryFixed = primary.tone(90),
            primaryFixedDim = primary.tone(80),
            onPrimaryFixed = primary.tone(10),
            onPrimaryFixedVariant = primary.tone(30),
            secondaryFixed = secondary.tone(90),
            secondaryFixedDim = secondary.tone(80),
            onSecondaryFixed = secondary.tone(10),
            onSecondaryFixedVariant = secondary.tone(30),
            tertiaryFixed = tertiary.tone(90),
            tertiaryFixedDim = tertiary.tone(80),
            onTertiaryFixed = tertiary.tone(10),
            onTertiaryFixedVariant = tertiary.tone(30),
            successContainer = pick(success, 90, 30),
            onSuccessContainer = pick(success, 30, 90),
            warningContainer = pick(warning, 90, 30),
            onWarningContainer = pick(warning, 30, 90),
            infoContainer = pick(info, 90, 30),
            onInfoContainer = pick(info, 30, 90),
        )
    }

    /**
     * A fixed hue, such as the green of success, turned part of the way
     * towards the seed's hue (half the distance, at most 15 degrees), so it
     * sits with the brand's colours and still reads as what it is.
     */
    fun harmonised(hue: Double, towards: Double): Double {
        val difference = ((towards - hue + 540.0) % 360.0) - 180.0
        val turn = min(abs(difference) * 0.5, 15.0) * sign(difference)
        return ((hue + turn) % 360.0 + 360.0) % 360.0
    }
}
