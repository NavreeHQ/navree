package com.navree.client.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.paneTitle
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import com.navree.client.design.OverlayRules
import com.navree.client.protocol.ActionRole
import com.navree.client.protocol.ButtonNode
import com.navree.client.protocol.DialogNode
import com.navree.client.protocol.FormNode
import com.navree.client.protocol.Node
import com.navree.client.protocol.PopoverNode
import com.navree.client.protocol.Screen
import com.navree.client.protocol.SheetNode
import com.navree.client.protocol.SheetSize
import com.navree.client.runtime.Entry
import com.navree.client.runtime.Event
import com.navree.client.runtime.overlays
import com.navree.client.runtime.submitButton
import com.navree.client.runtime.walk

/**
 * The sheets and dialogs of a screen that are open now, and the question a
 * destructive button asks. A sheet of the tree is a modal bottom sheet; a
 * dialog of the tree is a Material alert dialog.
 */
@Composable
internal fun Overlays(entry: Entry, screen: Screen) {
    val ui = LocalUi.current
    val place = Place(entry)
    for (overlay in screen.overlays()) {
        val id = overlay.id ?: continue
        if (!entry.local.isOpen(overlay)) continue
        val close = { ui.dispatch(Event.OverlayToggled(entry.key, id, false)) }
        when (overlay) {
            is SheetNode -> Sheet(overlay.title, overlay.subtitle, overlay.size, overlay.children, overlay.actions, place, close)
            is PopoverNode -> Sheet(null, null, SheetSize.Medium, overlay.children, emptyList(), place, close)
            is DialogNode -> TreeDialog(overlay, place.copy(inDialog = true), close)
            else -> Unit
        }
    }
    entry.local.confirm?.let { waiting -> ConfirmDialog(waiting.button, entry) }
}

@Composable
private fun Sheet(
    title: String?,
    subtitle: String?,
    size: SheetSize,
    children: List<Node>,
    actions: List<Node>,
    place: Place,
    close: () -> Unit,
) {
    val plan = OverlayRules.sheet(size, LocalUi.current.window)
    val state = rememberModalBottomSheetState(skipPartiallyExpanded = !plan.partial)
    ModalBottomSheet(
        onDismissRequest = close,
        sheetState = state,
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
                    subtitle?.let {
                        Text(it, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                    }
                }
            }
            for (child in children) NodeView(child, place)
            if (actions.isNotEmpty()) {
                Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp, androidx.compose.ui.Alignment.End)) {
                    for (action in actions) {
                        if (action is ButtonNode) QuietButton(action, place)
                    }
                }
            }
        }
    }
}

/** Which of a dialog's buttons affirm and which back out. Affirming ones sit at the end. */
private fun affirms(button: ButtonNode): Boolean = button.role != ActionRole.Cancel

/**
 * A dialog of the tree as a Material alert dialog. A dialog that holds a
 * form is finished from the dialog's own foot: the form's button is drawn
 * there as the affirming button, beside a way to back out, and not a second
 * time among the fields.
 */
@Composable
private fun TreeDialog(dialog: DialogNode, place: Place, close: () -> Unit) {
    val ui = LocalUi.current
    val buttons = dialog.actions.filterIsInstance<ButtonNode>()
    val forward = buttons.filter(::affirms)
    val backward = buttons.filterNot(::affirms)
    val form = dialog.children.asSequence().flatMap { it.walk() }.filterIsInstance<FormNode>().firstOrNull()
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
            if (form != null && submit != null) QuietButton(submit, place.copy(form = form.key))
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
    )
}

/**
 * The question asked before a destructive action runs, when the page did not
 * bring a dialog of its own. The button's own label is the question and the
 * answer that goes ahead; the client adds no words of its own but "Cancel".
 */
@Composable
private fun ConfirmDialog(button: ButtonNode, entry: Entry) {
    val ui = LocalUi.current
    val answer: (Boolean) -> Unit = { ui.dispatch(Event.ConfirmAnswered(entry.key, it)) }
    AlertDialog(
        onDismissRequest = { answer(false) },
        title = { Text("${button.label}?") },
        confirmButton = {
            TextButton(onClick = { answer(true) }) { Text(button.label, color = MaterialTheme.colorScheme.error) }
        },
        dismissButton = {
            TextButton(onClick = { answer(false) }) { Text(stringResource(android.R.string.cancel)) }
        },
    )
}
