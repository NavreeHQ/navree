package com.navree.client.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.paneTitle
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import com.navree.client.core.Events
import com.navree.client.design.OverlayRules
import com.navree.client.design.ScreenRules
import com.navree.client.design.overlays
import com.navree.client.design.submitButton
import com.navree.client.design.walk
import com.navree.protocol.Navree

/**
 * The sheets, dialogs and popovers of a screen's tree that are open now, and
 * the question a destructive action asks. A sheet is a modal bottom sheet
 * (with a most width on wide windows); a dialog is a Material alert dialog.
 */
@Composable
internal fun Overlays(place: Place) {
    val ui = LocalUi.current
    val tree = place.screen.screen ?: return
    for (overlay in tree.overlays()) {
        val id = when (overlay) {
            is Navree.Sheet -> overlay.id
            is Navree.Dialog -> overlay.id
            is Navree.Popover -> overlay.id
            else -> continue
        }
        val open = when (overlay) {
            is Navree.Sheet -> overlay.open
            is Navree.Dialog -> overlay.open
            else -> false
        } || place.screen.open(id)
        if (!open) continue
        val close = { ui.send(Events.overlay(place.id, id, false)) }
        val inner = place.copy(overlay = id)
        when (overlay) {
            is Navree.Sheet -> Sheet(overlay.title, overlay.subtitle, overlay.size, overlay.children, overlay.actions, inner, close)
            is Navree.Popover -> Sheet(null, null, Navree.SheetSize.MEDIUM, overlay.children, emptyList(), inner, close)
            is Navree.Dialog -> TreeDialog(overlay, inner.copy(inDialog = true), close)
            else -> Unit
        }
    }
}

@Composable
private fun Sheet(
    title: String?,
    subtitle: String?,
    size: Navree.SheetSize,
    children: List<Navree.Node>,
    actions: List<Navree.Node>,
    place: Place,
    close: () -> Unit,
) {
    val plan = OverlayRules.sheet(size, LocalUi.current.window)
    val state = rememberModalBottomSheetState(skipPartiallyExpanded = !plan.partial)
    ModalBottomSheet(
        onDismissRequest = close,
        sheetState = state,
        sheetMaxWidth = OverlayRules.SHEET_MOST_DP.dp,
        modifier = if (title != null) Modifier.semantics { paneTitle = title } else Modifier,
    ) {
        Column(
            Modifier
                .fillMaxWidth()
                .then(if (plan.fullHeight) Modifier.fillMaxHeight() else Modifier)
                .verticalScroll(rememberScrollState())
                .padding(horizontal = 24.dp)
                .padding(bottom = 24.dp),
            verticalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            if (title != null || subtitle != null) {
                Column {
                    title?.let { Text(it, style = MaterialTheme.typography.titleLarge, modifier = Modifier.semantics { heading() }) }
                    subtitle?.let { Text(it, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant) }
                }
            }
            for (child in children) NodeView(child, place)
            if (actions.isNotEmpty()) {
                Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp, Alignment.End)) {
                    for (action in actions) {
                        if (action is Navree.Button) QuietButton(action, place)
                    }
                }
            }
        }
    }
}

/** Which of a dialog's buttons affirm and which back out. Affirming ones sit at the end. */
private fun affirms(button: Navree.Button): Boolean = button.role != Navree.Role.CANCEL && button.standard != Navree.Standard.CANCEL

/**
 * A dialog of the tree as a Material alert dialog. A dialog that holds a
 * form is finished from the dialog's own foot: the form's button is drawn
 * there as the affirming button, beside a way to back out, and not a second
 * time among the fields.
 */
@Composable
private fun TreeDialog(dialog: Navree.Dialog, place: Place, close: () -> Unit) {
    val buttons = dialog.actions.filterIsInstance<Navree.Button>()
    val forward = buttons.filter(::affirms)
    val backward = buttons.filterNot(::affirms)
    val form = dialog.children.asSequence().flatMap { it.walk() }.filterIsInstance<Navree.Form>().firstOrNull()
    val submit = form?.submitButton()
    val inner = place.copy(omit = submit)
    AlertDialog(
        onDismissRequest = close,
        title = dialog.title?.let { title -> { Text(title) } },
        text = if (dialog.text != null || dialog.children.isNotEmpty()) {
            {
                Column(Modifier.verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(12.dp)) {
                    dialog.text?.let { Text(it) }
                    for (child in dialog.children) NodeView(child, inner)
                }
            }
        } else {
            null
        },
        confirmButton = {
            for (button in forward) QuietButton(button, place)
            if (form != null && submit != null) QuietButton(submit, place.copy(form = form.id))
        },
        dismissButton = when {
            backward.isNotEmpty() -> {
                { for (button in backward) QuietButton(button, place) }
            }
            submit != null -> {
                { TextButton(onClick = close) { Text(stringResource(android.R.string.cancel)) } }
            }
            else -> null
        },
        modifier = Modifier.widthIn(max = 560.dp),
    )
}

/**
 * The question asked before a destructive action runs, when the page did not
 * bring a dialog of its own. Its headline is the action's own words as a
 * question ("Cancel order?"); the confirming button repeats the action's verb
 * in the error colour, and "Cancel" backs out.
 */
@Composable
internal fun ConfirmDialog() {
    val ui = LocalUi.current
    val (place, button) = ui.asking.value ?: return
    val done = { ui.asking.value = null }
    AlertDialog(
        onDismissRequest = done,
        title = { Text(stringResource(com.navree.client.R.string.navree_confirm_question, button.label)) },
        confirmButton = {
            TextButton(
                onClick = {
                    done()
                    ui.act(place, button.action)
                },
            ) { Text(button.label, color = if (ScreenRules.destructive(button)) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.primary) }
        },
        dismissButton = { TextButton(onClick = done) { Text(stringResource(android.R.string.cancel)) } },
    )
}
