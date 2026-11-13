package com.navree.client.theme

import android.os.Build
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.ColorScheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Shapes
import androidx.compose.material3.dynamicDarkColorScheme
import androidx.compose.material3.dynamicLightColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.ReadOnlyComposable
import androidx.compose.runtime.remember
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.toArgb
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import com.navree.client.design.ToneRole
import com.navree.client.design.expressive.ExpressiveTheme

/** A container colour and the colour of what sits on it. */
@Immutable
data class TonePair(val container: Color, val content: Color)

/** The tones Material 3 has no roles for, made from the same seed as the rest. */
@Immutable
data class ToneColours(
    val success: TonePair,
    val warning: TonePair,
    val info: TonePair,
)

private val LocalToneColours = staticCompositionLocalOf {
    ToneColours(
        TonePair(Color.Unspecified, Color.Unspecified),
        TonePair(Color.Unspecified, Color.Unspecified),
        TonePair(Color.Unspecified, Color.Unspecified),
    )
}

/** Whether the person turned animations off: charts then appear at once, without growing. */
val LocalReducedMotion = staticCompositionLocalOf { false }

/** The container and content colours a tone is drawn with in the current theme. */
@Composable
@ReadOnlyComposable
fun toneColours(role: ToneRole): TonePair {
    val scheme = MaterialTheme.colorScheme
    val extra = LocalToneColours.current
    return when (role) {
        ToneRole.Surface -> TonePair(scheme.surfaceContainerHighest, scheme.onSurfaceVariant)
        ToneRole.Primary -> TonePair(scheme.primaryContainer, scheme.onPrimaryContainer)
        ToneRole.Error -> TonePair(scheme.errorContainer, scheme.onErrorContainer)
        ToneRole.Success -> extra.success
        ToneRole.Warning -> extra.warning
        ToneRole.Info -> extra.info
    }
}

/**
 * The palette as a Material colour scheme. Every role is given, so nothing
 * of Material's default purple is left in it, in light or in dark.
 */
internal fun Palette.toColorScheme(): ColorScheme = lightColorScheme(
    primary = Color(primary),
    onPrimary = Color(onPrimary),
    primaryContainer = Color(primaryContainer),
    onPrimaryContainer = Color(onPrimaryContainer),
    inversePrimary = Color(inversePrimary),
    secondary = Color(secondary),
    onSecondary = Color(onSecondary),
    secondaryContainer = Color(secondaryContainer),
    onSecondaryContainer = Color(onSecondaryContainer),
    tertiary = Color(tertiary),
    onTertiary = Color(onTertiary),
    tertiaryContainer = Color(tertiaryContainer),
    onTertiaryContainer = Color(onTertiaryContainer),
    background = Color(surface),
    onBackground = Color(onSurface),
    surface = Color(surface),
    onSurface = Color(onSurface),
    surfaceVariant = Color(surfaceVariant),
    onSurfaceVariant = Color(onSurfaceVariant),
    surfaceTint = Color(primary),
    inverseSurface = Color(inverseSurface),
    inverseOnSurface = Color(inverseOnSurface),
    error = Color(error),
    onError = Color(onError),
    errorContainer = Color(errorContainer),
    onErrorContainer = Color(onErrorContainer),
    outline = Color(outline),
    outlineVariant = Color(outlineVariant),
    scrim = Color(scrim),
    surfaceBright = Color(surfaceBright),
    surfaceContainer = Color(surfaceContainer),
    surfaceContainerHigh = Color(surfaceContainerHigh),
    surfaceContainerHighest = Color(surfaceContainerHighest),
    surfaceContainerLow = Color(surfaceContainerLow),
    surfaceContainerLowest = Color(surfaceContainerLowest),
    surfaceDim = Color(surfaceDim),
    primaryFixed = Color(primaryFixed),
    primaryFixedDim = Color(primaryFixedDim),
    onPrimaryFixed = Color(onPrimaryFixed),
    onPrimaryFixedVariant = Color(onPrimaryFixedVariant),
    secondaryFixed = Color(secondaryFixed),
    secondaryFixedDim = Color(secondaryFixedDim),
    onSecondaryFixed = Color(onSecondaryFixed),
    onSecondaryFixedVariant = Color(onSecondaryFixedVariant),
    tertiaryFixed = Color(tertiaryFixed),
    tertiaryFixedDim = Color(tertiaryFixedDim),
    onTertiaryFixed = Color(onTertiaryFixed),
    onTertiaryFixedVariant = Color(onTertiaryFixedVariant),
)

/** The shape scale for a corner style. Only `sharp` moves away from Material's own. */
internal fun shapesFor(corner: String?): Shapes = when (corner) {
    "sharp" -> Shapes(
        extraSmall = RoundedCornerShape(2.dp),
        small = RoundedCornerShape(4.dp),
        medium = RoundedCornerShape(6.dp),
        large = RoundedCornerShape(8.dp),
        extraLarge = RoundedCornerShape(12.dp),
    )
    else -> Shapes()
}

/**
 * The app's Material 3 theme. The brand gives one seed colour; the scheme is
 * made from it for light and for dark. With `dynamic` on Android 12 and
 * later, the user's wallpaper colours are used instead: when the app sets no
 * accent, or opts in. Type, shape (but for a sharp corner style) and
 * elevation stay the system's; motion is Material 3 Expressive's, or the
 * standard scheme when the person has turned animations off.
 */
@Composable
fun NavreeTheme(
    seed: Int,
    dynamic: Boolean = false,
    corner: String? = null,
    dark: Boolean = isSystemInDarkTheme(),
    content: @Composable () -> Unit,
) {
    val context = LocalContext.current
    val useDynamic = dynamic && Build.VERSION.SDK_INT >= Build.VERSION_CODES.S
    val palette = remember(seed, dark) { SeedScheme.make(seed, dark) }
    val scheme = when {
        useDynamic && dark -> dynamicDarkColorScheme(context)
        useDynamic -> dynamicLightColorScheme(context)
        else -> remember(palette) { palette.toColorScheme() }
    }
    // With the wallpaper's colours, the extra tones are turned towards its primary instead of the seed.
    val tones = remember(palette, scheme.primary, useDynamic) {
        val source = if (useDynamic) SeedScheme.make(scheme.primary.toArgb(), dark) else palette
        ToneColours(
            success = TonePair(Color(source.successContainer), Color(source.onSuccessContainer)),
            warning = TonePair(Color(source.warningContainer), Color(source.onWarningContainer)),
            info = TonePair(Color(source.infoContainer), Color(source.onInfoContainer)),
        )
    }
    val shapes = remember(corner) { shapesFor(corner) }
    val reduced = remember(context) {
        android.provider.Settings.Global.getFloat(context.contentResolver, android.provider.Settings.Global.ANIMATOR_DURATION_SCALE, 1f) == 0f
    }
    CompositionLocalProvider(LocalToneColours provides tones, LocalReducedMotion provides reduced) {
        ExpressiveTheme(colorScheme = scheme, shapes = shapes, typography = MaterialTheme.typography, reducedMotion = reduced, content = content)
    }
}
