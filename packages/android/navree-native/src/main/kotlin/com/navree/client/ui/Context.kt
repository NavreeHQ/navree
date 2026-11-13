package com.navree.client.ui

import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.layout.wrapContentWidth
import androidx.compose.material3.SnackbarHostState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalWindowInfo
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import com.navree.client.R
import com.navree.client.core.Driver
import com.navree.client.core.Events
import com.navree.client.core.Failure
import com.navree.client.core.FieldValue
import com.navree.client.core.Notice
import com.navree.client.core.ScreenView
import com.navree.client.design.ConfirmRules
import com.navree.client.design.ContentRules
import com.navree.client.design.WindowInfo
import com.navree.protocol.Navree
import kotlin.math.roundToInt

/** What every part of the interface can reach: the driver, the window, how images are fetched, the snackbars. */
internal class Ui(
    val driver: Driver,
    val window: WindowInfo,
    val image: suspend (String) -> ByteArray?,
    val snackbars: SnackbarHostState,
) {
    /** A destructive action waiting for its answer: the client's one question, in the action's own words. */
    val asking = mutableStateOf<Pair<Place, Navree.Button>?>(null)

    fun send(event: kotlinx.serialization.json.JsonObject) = driver.send(event)
}

internal val LocalUi = staticCompositionLocalOf<Ui> { error("No Navree client above this composable") }

/**
 * Where a node is drawn: the screen it stands on, the ids of the form and of
 * the sheet, dialog or popover around it (what the core needs with a tap),
 * whether a dialog holds it, and what is drawn elsewhere.
 */
internal data class Place(
    val screen: ScreenView,
    val form: String? = null,
    val overlay: String? = null,
    /** A dialog's own buttons are the confirmation; they are not asked about again. */
    val inDialog: Boolean = false,
    /** The address of the screen shown beside this one, when a list and its detail share the window. */
    val selectedUrl: String? = null,
    /** A node that is drawn elsewhere: a form's button that the app bar or a dialog's foot holds. */
    val omit: Navree.Node? = null,
    /** Drawn in a cell of a wide table, whose column heading already names what is in it. */
    val inCell: Boolean = false,
) {
    val id: Long get() = screen.id

    /** What a field holds now, as the core keeps it. */
    fun value(field: Navree.Field): FieldValue? = screen.field(form, field.id)?.value

    /** What is wrong with a field now, in the server's words. */
    fun errors(field: Navree.Field): List<String> = screen.field(form, field.id)?.errors.orEmpty()

    /** Whether the form around the node is on its way. */
    val pending: Boolean get() = screen.forms[form ?: ""]?.pending == true
}

/** Something with an action was tapped. */
internal fun Ui.act(place: Place, action: Navree.Action?, text: String? = null) {
    if (action == null) return
    send(Events.act(place.id, action, place.form, place.overlay, text))
}

/**
 * A button was tapped. One whose page wrote a question (`ask`) has the core
 * ask it, in the server's words, unless it can be undone: Android's way then
 * is to do it and offer Undo. A destructive one that writes and brought no
 * question asks in the client's dialog, in the action's own words.
 */
internal fun Ui.press(place: Place, button: Navree.Button) {
    if (button.disabled || button.pending) return
    val action = button.action ?: return
    val ask = button.ask?.takeUnless { button.undoable || place.inDialog }
    when {
        ask != null -> send(Events.act(place.id, action, place.form, place.overlay, ask = ask))
        ConfirmRules.asks(button, place.inDialog) -> asking.value = place to button
        else -> act(place, action)
    }
}

/** A field changed. */
internal fun Ui.change(place: Place, field: Navree.Field, value: FieldValue) {
    val id = field.id ?: return
    send(Events.field(place.id, place.form, id, value))
}

/** The window's size in dp and the user's text size, as the design rules take them. */
@Composable
internal fun rememberWindow(): WindowInfo {
    val size = LocalWindowInfo.current.containerDpSize
    val scale = LocalDensity.current.fontScale
    return WindowInfo(size.width.value.roundToInt(), size.height.value.roundToInt(), scale)
}

/**
 * Content in a wide window is one column of readable width, in the middle.
 * What is in the column starts at the column's start, as text does.
 */
internal fun Modifier.readable(mostDp: Int = ContentRules.CONTENT_MOST_DP): Modifier =
    fillMaxWidth().wrapContentWidth(Alignment.CenterHorizontally).widthIn(max = mostDp.dp).fillMaxWidth()

@Composable
internal fun failureTitle(failure: Failure): String = stringResource(
    when (failure) {
        Failure.Offline -> R.string.navree_offline_title
        Failure.Timeout -> R.string.navree_timeout_title
        Failure.Server -> R.string.navree_server_title
        Failure.Protocol, Failure.Config -> R.string.navree_protocol_title
    },
)

@Composable
internal fun failureText(failure: Failure): String = stringResource(
    when (failure) {
        Failure.Offline -> R.string.navree_offline_text
        Failure.Timeout -> R.string.navree_timeout_text
        Failure.Server -> R.string.navree_server_text
        Failure.Protocol, Failure.Config -> R.string.navree_protocol_text
    },
)

/** The client's own words for a notice of the core. */
@Composable
internal fun noticeText(notice: Notice): String = stringResource(
    when (notice) {
        Notice.Offline -> R.string.navree_notice_offline
        Notice.Timeout -> R.string.navree_notice_timeout
        Notice.Failed -> R.string.navree_notice_failed
        Notice.Unsaved -> R.string.navree_notice_unsaved
        Notice.Loading -> R.string.navree_notice_loading
        Notice.Copied -> R.string.navree_copied
        Notice.FieldsNeedAttention -> R.string.navree_notice_fields
        Notice.Loaded -> R.string.navree_notice_loaded
    },
)
