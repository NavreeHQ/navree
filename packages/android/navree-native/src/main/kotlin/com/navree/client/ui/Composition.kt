@file:OptIn(ExperimentalFoundationApi::class)

package com.navree.client.ui

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.core.MutableTransitionState
import androidx.compose.animation.fadeIn
import androidx.compose.animation.scaleIn
import androidx.compose.animation.slideInVertically
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.gestures.snapping.rememberSnapFlingBehavior
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxScope
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.pager.HorizontalPager
import androidx.compose.foundation.pager.rememberPagerState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.SideEffect
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.RectangleShape
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.CustomAccessibilityAction
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.collectionInfo
import androidx.compose.ui.semantics.CollectionInfo
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.customActions
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.navree.client.design.CompositionRules
import com.navree.client.theme.LocalReducedMotion
import com.navree.protocol.Navree

/*
 * Free composition: cards, parts over each other, shapes, parts that scroll
 * across, and a part for each room. Drawn with Material's own pieces; no
 * glass, no shadows of the page's choosing, nothing composed into a bar.
 */

/** The ids a screen has drawn, and whether it is still at its first drawing. */
internal class Shown {
    var first = true
    val ids = HashSet<String>()
}

internal val LocalShown = staticCompositionLocalOf<Shown?> { null }

/**
 * A part that comes in as the page says (`appear`) when an op adds it to a
 * screen already drawn: a fade, a rise or a scale with the system's own
 * timing. Never at a screen's first drawing, and never when the person
 * turned animations off.
 */
@Composable
internal fun Appearing(id: String?, appear: Navree.Appear?, content: @Composable () -> Unit) {
    val shown = LocalShown.current
    val fresh = remember(id) { appear != null && id != null && shown != null && !shown.first && id !in shown.ids }
    if (id != null) SideEffect { shown?.ids?.add(id) }
    if (!fresh || LocalReducedMotion.current) {
        content()
        return
    }
    val state = remember { MutableTransitionState(false).apply { targetState = true } }
    val enter = when (appear) {
        Navree.Appear.RISE -> fadeIn() + slideInVertically { it / 4 }
        Navree.Appear.SCALE -> fadeIn() + scaleIn(initialScale = 0.92f)
        else -> fadeIn()
    }
    AnimatedVisibility(visibleState = state, enter = enter) { content() }
}

/**
 * A card: Material's filled card, one variant for the whole client, in the
 * hue's container where the page gives a hue. A card with an action is one
 * control, its words read together; its menu opens on a long press and is
 * offered to TalkBack as actions.
 */
@Composable
internal fun CardView(node: Navree.Card, place: Place, modifier: Modifier = Modifier) {
    val ui = LocalUi.current
    val ink = node.colour?.let { inkOf(it, Navree.Hue.ACCENT) }
    val colours = if (ink != null) {
        CardDefaults.cardColors(containerColor = ink.container, contentColor = ink.onContainer)
    } else {
        CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceContainerLow)
    }
    var menuOpen by remember { mutableStateOf(false) }
    val action = node.action
    val menuActions = node.menu.filterIsInstance<Navree.Button>().map { item -> CustomAccessibilityAction(item.label) { ui.press(place, item); true } }
    val described = Modifier.semantics(mergeDescendants = action != null) {
        if (action != null) {
            role = Role.Button
            node.label?.let { contentDescription = it }
        }
        if (menuActions.isNotEmpty()) customActions = menuActions
    }
    val pad = CompositionRules.cardPadDp(node.pad).dp
    val room = LocalRoom.current
    val content: @Composable () -> Unit = {
        Column(Modifier.fillMaxWidth().padding(pad), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            // What the card holds has the card's width less its padding.
            CompositionLocalProvider(LocalInLine provides false) {
                Roomed(if (room > 0f) (room - 2 * pad.value).coerceAtLeast(1f) else 0f) {
                    for (child in node.children) NodeView(child, place)
                }
            }
        }
    }
    // A card that zooms into what it opens shares its bounds with that screen.
    val zoom = LocalZoom.current
    val cardId = node.id
    val zooms = node.transition == Navree.Transition.ZOOM && cardId != null && zoom != null
    if (zooms) SideEffect { ui.zoomCards.add(cardId!!) }
    val bounds = if (zooms) zoomBounds(zoom!!, cardId!!) else Modifier
    Appearing(node.id, node.appear) {
    Box(modifier.then(bounds).fillMaxWidth()) {
        when {
            node.menu.isNotEmpty() -> Card(
                modifier = Modifier
                    .fillMaxWidth()
                    .fillMaxHeight()
                    .clip(CardDefaults.shape)
                    .combinedClickable(
                        role = Role.Button,
                        onLongClick = { menuOpen = true },
                        onClick = { if (action != null) ui.act(place, action, from = node.id) else menuOpen = true },
                    )
                    .then(described),
                colors = colours,
            ) { content() }
            action != null -> Card(onClick = { ui.act(place, action, from = node.id) }, modifier = Modifier.fillMaxWidth().fillMaxHeight().then(described), colors = colours) { content() }
            // In a grid's row, cards are as high as the tallest.
            else -> Card(modifier = Modifier.fillMaxWidth().fillMaxHeight().then(described), colors = colours) { content() }
        }
        if (node.menu.isNotEmpty()) {
            DropdownMenu(expanded = menuOpen, onDismissRequest = { menuOpen = false }) {
                MenuItems(node.menu, place) { menuOpen = false }
            }
        }
    }
    }
}

private fun alignment(place: Navree.Place): Alignment = when (place) {
    Navree.Place.TOP_START -> Alignment.TopStart
    Navree.Place.TOP -> Alignment.TopCenter
    Navree.Place.TOP_END -> Alignment.TopEnd
    Navree.Place.START -> Alignment.CenterStart
    Navree.Place.CENTER -> Alignment.Center
    Navree.Place.END -> Alignment.CenterEnd
    Navree.Place.BOTTOM_START -> Alignment.BottomStart
    Navree.Place.BOTTOM -> Alignment.BottomCenter
    Navree.Place.BOTTOM_END -> Alignment.BottomEnd
}

/** A length of the page: points as dp, or, when `scaled`, following the person's text size. */
@Composable
private fun length(points: Int, scaled: Boolean): Dp = if (scaled) with(LocalDensity.current) { points.sp.toDp() } else points.dp

/** The size the page gives a part, or none. */
@Composable
private fun Modifier.sized(size: Navree.Size?): Modifier {
    if (size == null) return this
    var out = this
    size.width?.let { out = out.width(length(it, size.scaled)) }
    size.height?.let { out = out.height(length(it, size.scaled)) }
    return out
}

/** Parts over each other, the first at the back, all at the page's place; a shape with no size fills the box. */
@Composable
internal fun LayersView(node: Navree.Layers, place: Place, modifier: Modifier = Modifier) {
    val pad = (node.pad * CompositionRules.STEP_DP).dp
    val room = LocalRoom.current
    Box(modifier.sized(node.size).padding(pad), contentAlignment = alignment(node.place)) {
        Roomed(if (room > 0f) (room - 2 * pad.value).coerceAtLeast(1f) else 0f) {
            for (child in node.children) {
                if (child is Navree.Shape && child.size == null) {
                    ShapeFill(child)
                } else {
                    NodeView(child, place)
                }
            }
        }
    }
}

@Composable
private fun BoxScope.ShapeFill(node: Navree.Shape) = ShapeView(node, Modifier.matchParentSize())

private fun shapeOf(form: Navree.ShapeForm): Shape = when (form) {
    Navree.ShapeForm.CIRCLE -> CircleShape
    Navree.ShapeForm.CAPSULE -> RoundedCornerShape(percent = 50)
    Navree.ShapeForm.RECT -> RectangleShape
    Navree.ShapeForm.ROUNDED -> RoundedCornerShape(12.dp)
}

/**
 * A shape: a rounded rectangle, a circle, a capsule or a rectangle, filled in
 * its hue (its container tone when soft, a gradient to `to` when given), with
 * a hairline in its stroke's hue. Decoration unless it has a label.
 */
@Composable
internal fun ShapeView(node: Navree.Shape, modifier: Modifier = Modifier) {
    val shape = if (node.form == Navree.ShapeForm.ROUNDED) MaterialTheme.shapes.medium else shapeOf(node.form)
    val fill = node.fill?.let { inkOf(it, Navree.Hue.ACCENT) }
    val to = node.fill?.to?.let { ink(it) }
    val stroke = node.stroke?.let { inkOf(it, Navree.Hue.NEUTRAL) }
    var drawn = modifier.sized(node.size)
    if (fill != null) {
        val start = if (node.soft) fill.container else fill.mark
        drawn = if (to != null) {
            drawn.background(Brush.linearGradient(listOf(start, if (node.soft) to.container else to.mark)), shape)
        } else {
            drawn.background(start, shape)
        }
    }
    if (stroke != null) drawn = drawn.border(1.dp, stroke.mark, shape)
    val label = node.label
    Box(drawn.then(if (label != null) Modifier.semantics { contentDescription = label } else Modifier.clearAndSetSemantics {}))
}

/**
 * Parts that scroll across. With a least width, as many as fit show at once,
 * each the same width, and a fling stops at a part; paged, one at a time
 * with marks for the pages. Every part is reachable by TalkBack's steps.
 */
@Composable
internal fun CarouselView(node: Navree.Carousel, place: Place, modifier: Modifier = Modifier) {
    val gap = (node.gap * CompositionRules.STEP_DP).dp
    val label = node.label
    val named = if (label != null) Modifier.semantics { contentDescription = label } else Modifier
    if (node.paged) {
        val state = rememberPagerState { node.children.size }
        Column(modifier.fillMaxWidth().then(named), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            HorizontalPager(state = state, pageSpacing = gap, modifier = Modifier.fillMaxWidth()) { page ->
                NodeView(node.children[page], place, Modifier.fillMaxWidth())
            }
            if (node.children.size > 1) {
                Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(6.dp, Alignment.CenterHorizontally)) {
                    repeat(node.children.size) { index ->
                        val colour = if (index == state.currentPage) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.outlineVariant
                        Box(Modifier.size(8.dp).clip(CircleShape).background(colour).clearAndSetSemantics {})
                    }
                }
            }
        }
        return
    }
    BoxWithConstraints(modifier.fillMaxWidth().then(named)) {
        val room = maxWidth.value
        val count = CompositionRules.carouselCount(room, node.min, node.gap)
        val list = rememberLazyListState()
        LazyRow(
            state = list,
            horizontalArrangement = Arrangement.spacedBy(gap),
            flingBehavior = rememberSnapFlingBehavior(list),
            modifier = Modifier.fillMaxWidth().semantics { collectionInfo = CollectionInfo(rowCount = 1, columnCount = node.children.size) },
        ) {
            items(node.children.size) { index ->
                val width = count?.let { CompositionRules.carouselItemDp(room, it, node.gap).dp }
                Box(if (width != null) Modifier.width(width) else Modifier) {
                    Roomed(width?.value ?: 0f) {
                        NodeView(node.children[index], place, if (width != null) Modifier.fillMaxWidth() else Modifier)
                    }
                }
            }
        }
    }
}

/** A part for each room: the wide one where the place the content is drawn in is at least 600 dp wide. */
@Composable
internal fun AdaptiveView(node: Navree.Adaptive, place: Place, modifier: Modifier = Modifier) {
    BoxWithConstraints(modifier.fillMaxWidth()) {
        val room = CompositionRules.room(node, maxWidth.value)
        Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
            Roomed(maxWidth.value) { for (child in room) NodeView(child, place) }
        }
    }
}
