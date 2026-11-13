@file:OptIn(ExperimentalSharedTransitionApi::class)

package com.navree.client.ui

import androidx.compose.animation.AnimatedVisibilityScope
import androidx.compose.animation.ExperimentalSharedTransitionApi
import androidx.compose.animation.SharedTransitionLayout
import androidx.compose.animation.SharedTransitionScope
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.runtime.compositionLocalOf
import androidx.compose.ui.Modifier

/*
 * The zoom from a card to the screen it opens: the card and the screen it
 * opened share their bounds, so the screen grows out of the card on the way
 * in and shrinks back into it on the way out, the way Material's own
 * container transform moves. Only a card that asks for it (`transition:
 * zoom`) takes part, and the screen it opened says it was opened from it.
 */

/** The scope the shared bounds are drawn in. Held as one thing so only this file touches the transition API. */
internal class ZoomScope(internal val shared: SharedTransitionScope)

/** The scope of a screen coming or going, with the shared bounds' own. */
internal class Zoom(val scope: ZoomScope, val visibility: AnimatedVisibilityScope)

/** Where the screen being drawn takes part in a zoom, if it can. */
internal val LocalZoom = compositionLocalOf<Zoom?> { null }

/** The layer the zoom is drawn in; what is inside it may take part. */
@Composable
internal fun ZoomLayout(content: @Composable (ZoomScope) -> Unit) {
    SharedTransitionLayout(Modifier.fillMaxSize()) { content(ZoomScope(this)) }
}

/** The bounds that a card and the screen it opens share, under the card's id. */
@Composable
internal fun zoomBounds(zoom: Zoom, card: String): Modifier = with(zoom.scope.shared) {
    Modifier.sharedBounds(rememberSharedContentState(key = "zoom:$card"), animatedVisibilityScope = zoom.visibility)
}
