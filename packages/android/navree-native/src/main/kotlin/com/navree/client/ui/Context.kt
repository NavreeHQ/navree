package com.navree.client.ui

import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.layout.wrapContentWidth
import androidx.compose.material3.SnackbarHostState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalWindowInfo
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import com.navree.client.R
import com.navree.client.design.ContentRules
import com.navree.client.design.WindowInfo
import com.navree.client.protocol.Action
import com.navree.client.protocol.ButtonNode
import com.navree.client.protocol.Node
import com.navree.client.protocol.Submit
import com.navree.client.runtime.Entry
import com.navree.client.runtime.Event
import com.navree.client.runtime.Failure
import com.navree.client.runtime.Origin
import kotlin.math.roundToInt

/** What every part of the interface can reach: where events go, the window, the origin and how images are fetched. */
internal class Ui(
    val dispatch: (Event) -> Unit,
    val window: WindowInfo,
    val origin: Origin,
    val image: suspend (String) -> ByteArray?,
    val snackbars: SnackbarHostState,
)

internal val LocalUi = staticCompositionLocalOf<Ui> { error("No Navree client above this composable") }

/** Where a node is drawn: the screen it belongs to, the form around it, and whether a dialog holds it. */
internal data class Place(
    val entry: Entry,
    val form: String? = null,
    /** A dialog's own buttons are the confirmation; they are not asked about again. */
    val inDialog: Boolean = false,
    /** The address of the screen shown beside this one, when a list and its detail share the window. */
    val selectedUrl: String? = null,
    /** A node that is drawn elsewhere: a form's button that the app bar or a dialog's foot holds. */
    val omit: Node? = null,
) {
    val key: Long get() = entry.key
}

/** A submit that names no form means the form it stands in. */
private fun inForm(action: Action?, place: Place): Action? =
    if (action is Submit && action.form == null && place.form != null) action.copy(form = place.form) else action

internal fun Ui.act(place: Place, action: Action?, source: String? = null) {
    val meant = inForm(action, place)
    if (meant != null) dispatch(Event.Acted(place.key, meant, source))
}

internal fun Ui.press(place: Place, button: ButtonNode) {
    val meant = inForm(button.action, place)
    dispatch(Event.Pressed(place.key, if (meant === button.action) button else button.copy(action = meant), confirmed = place.inDialog))
}

/** The window's size in dp and the user's text size, as the design rules take them. */
@Composable
internal fun rememberWindow(): WindowInfo {
    val size = LocalWindowInfo.current.containerDpSize
    val scale = LocalDensity.current.fontScale
    return WindowInfo(size.width.value.roundToInt(), size.height.value.roundToInt(), scale)
}

/** Content in a wide window is one column of readable width, in the middle. */
internal fun Modifier.readable(mostDp: Int = ContentRules.CONTENT_MOST_DP): Modifier =
    fillMaxWidth().wrapContentWidth(Alignment.CenterHorizontally).widthIn(max = mostDp.dp)

@Composable
internal fun failureTitle(failure: Failure): String = stringResource(
    when (failure) {
        Failure.Offline -> R.string.navree_offline_title
        Failure.Server -> R.string.navree_server_title
        Failure.Protocol -> R.string.navree_protocol_title
    },
)

@Composable
internal fun failureText(failure: Failure): String = stringResource(
    when (failure) {
        Failure.Offline -> R.string.navree_offline_text
        Failure.Server -> R.string.navree_server_text
        Failure.Protocol -> R.string.navree_protocol_text
    },
)

@Composable
internal fun failureShort(failure: Failure): String = stringResource(
    when (failure) {
        Failure.Offline -> R.string.navree_offline_short
        Failure.Server -> R.string.navree_server_short
        Failure.Protocol -> R.string.navree_protocol_short
    },
)
