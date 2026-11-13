package com.navree.client.design

/** Android's window width classes. The lower bounds, in dp: 0, 600, 840, 1200, 1600. */
enum class WidthClass {
    Compact,
    Medium,
    Expanded,
    Large,
    ExtraLarge,
    ;

    companion object {
        fun of(widthDp: Int): WidthClass = when {
            widthDp < 600 -> Compact
            widthDp < 840 -> Medium
            widthDp < 1200 -> Expanded
            widthDp < 1600 -> Large
            else -> ExtraLarge
        }
    }
}

/** Android's window height classes. The lower bounds, in dp: 0, 480, 900. */
enum class HeightClass {
    Compact,
    Medium,
    Expanded,
    ;

    companion object {
        fun of(heightDp: Int): HeightClass = when {
            heightDp < 480 -> Compact
            heightDp < 900 -> Medium
            else -> Expanded
        }
    }
}

/**
 * The window the app is drawn in, as the rules need it: its size in dp and the
 * user's text size. Every rule of the design layer is a function of this and
 * of what the tree says, so each can be checked without a device.
 */
data class WindowInfo(
    val widthDp: Int,
    val heightDp: Int,
    /** The user's text size setting: 1.0 is the default, 2.0 the largest. */
    val fontScale: Float = 1f,
) {
    val width: WidthClass get() = WidthClass.of(widthDp)
    val height: HeightClass get() = HeightClass.of(heightDp)

    /** Text is large enough that side-by-side text no longer fits and must stack. */
    val largeText: Boolean get() = fontScale >= 1.3f

    /** Text is at the largest sizes: the bar gives up actions so the title can be read. */
    val largestText: Boolean get() = fontScale >= 1.5f

    companion object {
        /** A common phone, upright. */
        val Phone = WindowInfo(412, 915)

        /** The same phone on its side. */
        val PhoneLandscape = WindowInfo(915, 412)

        /** A foldable opened, or a small tablet upright. */
        val Foldable = WindowInfo(673, 841)

        /** A tablet on its side. */
        val Tablet = WindowInfo(1280, 800)

        /** A desktop window. */
        val Desktop = WindowInfo(1680, 1000)
    }
}
