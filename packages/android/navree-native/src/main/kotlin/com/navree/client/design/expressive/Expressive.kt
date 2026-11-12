@file:OptIn(ExperimentalMaterial3ExpressiveApi::class)

package com.navree.client.design.expressive

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.material3.ButtonGroupDefaults
import androidx.compose.material3.ColorScheme
import androidx.compose.material3.ContainedLoadingIndicator
import androidx.compose.material3.ExperimentalMaterial3ExpressiveApi
import androidx.compose.material3.HorizontalFloatingToolbar
import androidx.compose.material3.LoadingIndicator
import androidx.compose.material3.MaterialExpressiveTheme
import androidx.compose.material3.MotionScheme
import androidx.compose.material3.Shapes
import androidx.compose.material3.Text
import androidx.compose.material3.ToggleButton
import androidx.compose.material3.Typography
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow

/*
 * Every call into Material 3's Expressive components, which are still a
 * pre-release in the library this client builds against. Nothing else in the
 * client names them, so when the library renames one, this file is the only
 * one that changes. Each function takes the client's own words, not the
 * library's types.
 */

/**
 * The theme, with Material 3 Expressive's motion: springs that overshoot a
 * little for the screens' own movements. When the person has turned
 * animations off, the standard motion scheme is used instead.
 */
@Composable
fun ExpressiveTheme(
    colorScheme: ColorScheme,
    shapes: Shapes,
    typography: Typography,
    reducedMotion: Boolean,
    content: @Composable () -> Unit,
) {
    MaterialExpressiveTheme(
        colorScheme = colorScheme,
        motionScheme = if (reducedMotion) MotionScheme.standard() else MotionScheme.expressive(),
        shapes = shapes,
        typography = typography,
        content = content,
    )
}

/** The indicator for a wait of unknown length, between a moment and a few seconds. */
@Composable
fun Waiting(modifier: Modifier = Modifier, contained: Boolean = false) {
    if (contained) ContainedLoadingIndicator(modifier) else LoadingIndicator(modifier)
}

/**
 * A floating toolbar: the actions of a detail page, over its content at the
 * bottom, where the thumb is. Shown expanded; it never shares the screen with
 * a navigation bar (a pushed screen has none).
 */
@Composable
fun FloatingActions(modifier: Modifier = Modifier, content: @Composable RowScope.() -> Unit) {
    HorizontalFloatingToolbar(expanded = true, modifier = modifier, content = content)
}

/**
 * One choice of a few as a connected button group, Material 3's replacement
 * for segmented buttons: the options side by side, the chosen one filled.
 */
@Composable
fun ConnectedChoice(
    labels: List<String>,
    selected: Int,
    enabled: (Int) -> Boolean,
    onSelect: (Int) -> Unit,
    modifier: Modifier = Modifier,
) {
    Row(
        modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.spacedBy(ButtonGroupDefaults.ConnectedSpaceBetween),
    ) {
        labels.forEachIndexed { index, label ->
            ToggleButton(
                checked = index == selected,
                onCheckedChange = { onSelect(index) },
                enabled = enabled(index),
                modifier = Modifier.weight(1f).semantics { role = Role.RadioButton },
                shapes = when (index) {
                    0 -> ButtonGroupDefaults.connectedLeadingButtonShapes()
                    labels.lastIndex -> ButtonGroupDefaults.connectedTrailingButtonShapes()
                    else -> ButtonGroupDefaults.connectedMiddleButtonShapes()
                },
            ) {
                Text(label, maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
        }
    }
}
