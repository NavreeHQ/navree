package com.navree.client.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.systemBarsPadding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Surface
import androidx.compose.ui.graphics.RectangleShape
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import com.navree.client.R
import com.navree.client.design.Presentation
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
            is Navree.Sheet -> when (ScreenRules.sheet(overlay.task, LocalUi.current.window)) {
                // Drawn beside the content by the screen itself.
                Presentation.SideSheet -> Unit
                Presentation.FullScreenDialog -> TaskSheet(overlay, inner, full = true, close)
                Presentation.Dialog -> TaskSheet(overlay, inner, full = false, close)
                else -> Sheet(overlay.title, overlay.subtitle, overlay.size, overlay.children, overlay.actions, inner, close)
            }
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

/**
 * A sheet of the page that is a task: something to finish or cancel. On a
 * short window it is a full-screen dialog (an X, the title, the affirming
 * action at the bar's end); on a wider one a basic dialog.
 */
@Composable
private fun TaskSheet(sheet: Navree.Sheet, place: Place, full: Boolean, close: () -> Unit) {
    val buttons = sheet.actions.filterIsInstance<Navree.Button>()
    val forward = buttons.filter(::affirms)
    val form = sheet.children.asSequence().flatMap { it.walk() }.filterIsInstance<Navree.Form>().firstOrNull()
    val submit = form?.submitButton()
    val inner = place.copy(omit = submit)
    Dialog(onDismissRequest = close, properties = DialogProperties(usePlatformDefaultWidth = !full)) {
        Surface(
            shape = if (full) RectangleShape else MaterialTheme.shapes.extraLarge,
            color = if (full) MaterialTheme.colorScheme.surface else MaterialTheme.colorScheme.surfaceContainerHigh,
            modifier = if (full) Modifier.fillMaxSize() else Modifier.widthIn(max = 560.dp),
        ) {
            Column(Modifier.then(if (full) Modifier.fillMaxSize().systemBarsPadding().imePadding() else Modifier)) {
                Row(Modifier.fillMaxWidth().padding(horizontal = 4.dp, vertical = 8.dp), verticalAlignment = Alignment.CenterVertically) {
                    TipIconButton(stringResource(R.string.navree_close), Glyphs.Close, close)
                    Text(sheet.title ?: "", style = MaterialTheme.typography.titleLarge, maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f).padding(start = 8.dp).semantics { heading() })
                    for (button in forward) QuietButton(button, place)
                    if (form != null && submit != null) QuietButton(submit, place.copy(form = form.id))
                }
                Column(
                    Modifier.weight(1f, fill = false).verticalScroll(rememberScrollState()).padding(horizontal = 24.dp).padding(bottom = 24.dp),
                    verticalArrangement = Arrangement.spacedBy(16.dp),
                ) {
                    sheet.subtitle?.let { Text(it, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant) }
                    for (child in sheet.children) NodeView(child, inner)
                }
            }
        }
    }
}

/**
 * A sheet of the page that is a glance, beside the content on a wide window:
 * Material's standard side sheet, holding the detail of what was tapped
 * while the list stays in view.
 */
@Composable
internal fun SideSheet(sheet: Navree.Sheet, place: Place, modifier: Modifier = Modifier) {
    val ui = LocalUi.current
    val close = { ui.send(Events.overlay(place.id, sheet.id, false)) }
    Surface(
        color = MaterialTheme.colorScheme.surfaceContainerLow,
        shape = RoundedCornerShape(topStart = 28.dp, bottomStart = 28.dp),
        modifier = modifier.fillMaxHeight().semantics { sheet.title?.let { paneTitle = it } },
    ) {
        Column(Modifier.fillMaxSize().systemBarsPadding()) {
            Row(Modifier.fillMaxWidth().padding(start = 24.dp, end = 8.dp, top = 12.dp, bottom = 4.dp), verticalAlignment = Alignment.CenterVertically) {
                Column(Modifier.weight(1f)) {
                    sheet.title?.let { Text(it, style = MaterialTheme.typography.titleLarge, modifier = Modifier.semantics { heading() }) }
                    sheet.subtitle?.let { Text(it, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant) }
                }
                TipIconButton(stringResource(R.string.navree_close), Glyphs.Close, close)
            }
            Column(
                Modifier.weight(1f).verticalScroll(rememberScrollState()).padding(horizontal = 24.dp).padding(bottom = 24.dp),
                verticalArrangement = Arrangement.spacedBy(16.dp),
            ) {
                val inner = place.copy(overlay = sheet.id)
                for (child in sheet.children) NodeView(child, inner)
                if (sheet.actions.isNotEmpty()) {
                    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp, Alignment.End)) {
                        for (action in sheet.actions) if (action is Navree.Button) QuietButton(action, inner)
                    }
                }
            }
        }
    }
}

/** The glance sheet of a page that is open now, when there is one. */
internal fun openGlance(place: Place): Navree.Sheet? {
    val tree = place.screen.screen ?: return null
    return tree.overlays().filterIsInstance<Navree.Sheet>().lastOrNull { !it.task && (it.open || place.screen.open(it.id)) }
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
 * The core's question before an action runs, in the server's words: the
 * headline, a line of text, the confirming button at the end and the way back
 * before it (Android's own "Cancel" when the page names none). Every way out
 * of the dialog is an answer.
 */
@Composable
internal fun QuestionDialog(place: Place) {
    val ui = LocalUi.current
    val question = place.screen.question ?: return
    val no = { ui.send(Events.answer(place.id, question.id, false)) }
    AlertDialog(
        onDismissRequest = no,
        title = { Text(question.title) },
        text = question.text?.let { text -> { Text(text) } },
        confirmButton = { TextButton(onClick = { ui.send(Events.answer(place.id, question.id, true)) }) { Text(question.confirm) } },
        dismissButton = { TextButton(onClick = no) { Text(question.cancel ?: stringResource(android.R.string.cancel)) } },
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
