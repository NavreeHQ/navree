package com.navree.client.ui

import android.view.View
import android.view.ViewGroup
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.WindowInsetsSides
import androidx.compose.foundation.layout.asPaddingValues
import androidx.compose.foundation.layout.consumeWindowInsets
import androidx.compose.foundation.layout.displayCutout
import androidx.compose.foundation.layout.exclude
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.onConsumedWindowInsetsChanged
import androidx.compose.foundation.layout.only
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.foundation.layout.systemBars
import androidx.compose.foundation.layout.union
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.unit.Density
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.LayoutDirection
import androidx.compose.ui.unit.dp
import androidx.compose.ui.zIndex

/*
 * How the parts of a window are laid out.
 *
 * A keyboard's Tab goes through the focus targets of a window in the order their
 * layout nodes are placed (`FocusableChildrenComparator` in compose-ui's
 * `OneDimensionalFocusSearch.kt`), and a `Column` or a `Row` places its children
 * in the order they are written. So each window is a `Column` or a `Row` whose
 * children are written in the order a person reads them: on a phone the top bar,
 * the content, the bottom navigation; on a tablet and wider the navigation at
 * the start edge, then the top bar, then the content. Material's `Scaffold`
 * places its content before its bars (to draw them over it), which is why it is
 * not used for the order.
 *
 * What a `Scaffold` also did is kept here: each bar consumes the window insets
 * it covers so the content gets the rest (`consumeWindowInsets`, the foundation
 * layout's documented way: what a layout consumes is not padded again by the
 * layouts below it), the passing messages and the floating action button stand
 * over the content and are written after it, and the screen's scroll behaviour
 * is put on the frame's modifier by the caller.
 *
 * That is the order Tab goes on in. Where a keyboard comes into a window that
 * holds the focus nowhere is not a search in that order, and is settled apart
 * ([KeyboardEntry]).
 */

/** Insets whose value can change without the instance changing, so a change is a new layout and not a new composition. */
internal class HeldInsets(first: WindowInsets = WindowInsets(0, 0, 0, 0)) : WindowInsets {
    var insets: WindowInsets by mutableStateOf(first)

    override fun getLeft(density: Density, layoutDirection: LayoutDirection): Int = insets.getLeft(density, layoutDirection)

    override fun getTop(density: Density): Int = insets.getTop(density)

    override fun getRight(density: Density, layoutDirection: LayoutDirection): Int = insets.getRight(density, layoutDirection)

    override fun getBottom(density: Density): Int = insets.getBottom(density)
}

/** The window of a phone: what is in front, then the bar of destinations along the bottom. */
@Composable
internal fun BottomNavigationWindow(bottomBar: @Composable () -> Unit, content: @Composable () -> Unit) {
    // What the bar covers is what the content is not given: the keyboard's space is counted from the bar's top.
    val covered = remember { HeldInsets() }
    Surface(Modifier.fillMaxSize(), color = MaterialTheme.colorScheme.background) {
        Column(Modifier.fillMaxSize()) {
            Box(Modifier.weight(1f).fillMaxWidth().consumeWindowInsets(covered)) { content() }
            Box(Modifier.onSizeChanged { covered.insets = WindowInsets(0, 0, 0, it.height) }) { bottomBar() }
        }
    }
}

/** The window of a tablet and wider: the rail or drawer at the start edge, then what is in front. */
@Composable
internal fun SideNavigationWindow(rail: @Composable () -> Unit, content: @Composable () -> Unit) {
    Row(Modifier.fillMaxSize()) {
        rail()
        Box(Modifier.weight(1f).fillMaxHeight().consumeWindowInsets(WindowInsets.safeDrawing.only(WindowInsetsSides.Start))) { content() }
    }
}

/** The room the system's bars leave, as the padding a layout gives its content; the top is left out when a bar above covers it. */
private class RoomLeft(private val room: PaddingValues, private val top: Boolean) : PaddingValues {
    override fun calculateLeftPadding(layoutDirection: LayoutDirection): Dp = room.calculateLeftPadding(layoutDirection)

    override fun calculateTopPadding(): Dp = if (top) room.calculateTopPadding() else 0.dp

    override fun calculateRightPadding(layoutDirection: LayoutDirection): Dp = room.calculateRightPadding(layoutDirection)

    override fun calculateBottomPadding(): Dp = room.calculateBottomPadding()
}

/**
 * One screen: its top bar, its content, then what floats over the content (the
 * passing messages and the floating action button).
 *
 * The content is given the room the system's bars leave, minus what the bars
 * above it have consumed: at the top only where no top bar is shown (the top bar
 * covers the status bar itself, and the content starts under it); at the sides
 * and the bottom always.
 */
@Composable
internal fun ScreenFrame(
    modifier: Modifier = Modifier,
    topBarShown: Boolean,
    topBar: @Composable () -> Unit,
    snackbarHost: @Composable () -> Unit,
    floatingActionButton: (@Composable () -> Unit)? = null,
    content: @Composable (PaddingValues) -> Unit,
) {
    val bars = WindowInsets.systemBars.union(WindowInsets.displayCutout)
    val safe = remember(bars) { HeldInsets(bars) }
    val room = safe.asPaddingValues()
    Surface(
        modifier = modifier.onConsumedWindowInsetsChanged { consumed -> safe.insets = bars.exclude(consumed) },
        color = MaterialTheme.colorScheme.background,
    ) {
        Box(Modifier.fillMaxSize()) {
            Column(Modifier.fillMaxSize()) {
                // Drawn over the content, as a bar is, though it is laid out before it.
                Box(Modifier.zIndex(1f)) { topBar() }
                Box(
                    Modifier
                        .weight(1f)
                        .fillMaxWidth()
                        .then(if (topBarShown) Modifier.consumeWindowInsets(bars.only(WindowInsetsSides.Top)) else Modifier),
                ) { content(RoomLeft(room, top = !topBarShown)) }
            }
            // After the content: the messages, then the button, above the room the bars leave at the bottom.
            Column(Modifier.align(Alignment.BottomCenter).fillMaxWidth().padding(RoomLeft(room, top = false))) {
                Box(Modifier.align(Alignment.CenterHorizontally)) { snackbarHost() }
                if (floatingActionButton != null) {
                    Box(Modifier.align(Alignment.End).padding(end = 16.dp, bottom = 16.dp)) { floatingActionButton() }
                }
            }
        }
    }
}

/**
 * Where a keyboard comes into a window that holds the focus nowhere: at the
 * window's first stop in the order Tab then goes on in, whichever way the
 * window is laid out.
 *
 * With the focus nowhere the system does not search in Tab's direction. It
 * gives the window its default focus (`ViewRootImpl.performFocusNavigation`,
 * and `leaveTouchMode` for the key that ends touch mode, both call
 * `View.restoreDefaultFocus()`, which asks for the focus going down), and
 * Compose answers a request going down from no place with the control nearest
 * the window's top left corner (`findChildCorrespondingToFocusEnter` in
 * compose-ui's `TwoDimensionalFocusSearch.kt`). Laid out left to right that is
 * the control a person reads first; laid out right to left it is not, since
 * reading starts at the top right. The order between the parts is not changed
 * by the direction: a `Row` places its children as they are written whichever
 * way it lays them out.
 *
 * A view a window names as its default focus is asked before anything else
 * (`ViewGroup.restoreDefaultFocus`). So the window is given one, beside the
 * composition and of no size, that never takes the focus itself and asks the
 * composition for it going forward, which Compose answers with the first stop
 * in the order the parts are placed in. Where that finds nothing the system
 * goes on as it would have. A dialog, a menu and a sheet are windows of their
 * own and keep the system's default.
 */
@Composable
internal fun KeyboardEntry() {
    val composition = LocalView.current
    DisposableEffect(composition) {
        val frame: ViewGroup? = composition.rootView.findViewById(android.R.id.content)
        val entry = if (frame == null) null else FirstStop(composition).also { frame.addView(it, ViewGroup.LayoutParams(0, 0)) }
        onDispose {
            if (frame != null && entry != null) frame.removeView(entry)
        }
    }
}

/** A window's default focus: it never has the focus, and hands a request for it to the [composition], going forward. */
private class FirstStop(private val composition: View) : View(composition.context) {
    init {
        isFocusable = false
        importantForAccessibility = IMPORTANT_FOR_ACCESSIBILITY_NO
        isFocusedByDefault = true
    }

    override fun restoreDefaultFocus(): Boolean = composition.requestFocus(FOCUS_FORWARD)
}
