package com.navree.client.theme

import com.navree.protocol.Navree

/**
 * The page's hues in the Material theme. Three of them are roles Material
 * already has: `accent` is the theme's primary, `danger` its error, `neutral`
 * its outline. Every other hue has no role in Material, so it is made the way
 * Material makes a custom colour: a tonal palette at a fixed hue, turned part
 * of the way towards the theme's primary (harmonised), read at the tones
 * Material reads its own roles at. The server's `light` and `dark` colours
 * are for clients that do not know a hue; every hue is known here.
 */
object Hues {
    /** Where a hue's colours come from. */
    enum class Source { Primary, Error, Outline, Palette }

    fun source(hue: Navree.Hue): Source = when (hue) {
        Navree.Hue.ACCENT -> Source.Primary
        Navree.Hue.DANGER -> Source.Error
        Navree.Hue.NEUTRAL -> Source.Outline
        else -> Source.Palette
    }

    /**
     * The hue's angle in Material's colour space (HCT), before it is turned
     * towards the theme. A status and the plain hue it is named after share
     * one: a chart that colours by meaning and one that colours by identity
     * may show the same green.
     */
    fun angle(hue: Navree.Hue): Double? = when (hue) {
        Navree.Hue.SUCCESS, Navree.Hue.GREEN -> 145.0
        Navree.Hue.WARNING, Navree.Hue.AMBER -> 80.0
        Navree.Hue.INFO, Navree.Hue.BLUE -> 255.0
        Navree.Hue.ORANGE -> 50.0
        Navree.Hue.RED -> 27.0
        Navree.Hue.ROSE -> 0.0
        Navree.Hue.VIOLET -> 300.0
        Navree.Hue.CYAN -> 205.0
        Navree.Hue.SLATE -> 250.0
        Navree.Hue.ACCENT, Navree.Hue.DANGER, Navree.Hue.NEUTRAL -> null
    }

    /** How colourful a hue's palette is: slate is a grey with a trace of blue; the rest are vivid enough to tell apart. */
    fun chroma(hue: Navree.Hue): Double = if (hue == Navree.Hue.SLATE) 16.0 else 48.0

    /** The tone a mark (an arc, a bar, a point) is drawn at: bright on a light surface, lighter on a dark one. */
    fun markTone(dark: Boolean): Int = if (dark) 70 else 50

    /** The tone of the track a mark runs along: the hue's lightest container in light, a deep one in dark. */
    fun trackTone(dark: Boolean): Int = if (dark) 25 else 90

    /** A container in the hue and the colour of what sits on it, at Material's container tones. */
    fun containerTone(dark: Boolean): Int = if (dark) 30 else 90

    fun onContainerTone(dark: Boolean): Int = if (dark) 90 else 30

    /** A hue's palette in a theme whose primary has this hue. Null for the hues Material has roles for. */
    fun palette(hue: Navree.Hue, primaryHue: Double): TonalPalette? {
        val angle = angle(hue) ?: return null
        return TonalPalette(SeedScheme.harmonised(angle, primaryHue), chroma(hue))
    }

    /** The shades of a heat map: four steps of one hue, from faint to strong, each a tone of its palette. */
    fun shadeTones(dark: Boolean): List<Int> = if (dark) listOf(25, 40, 55, 70) else listOf(90, 75, 60, 45)
}
