package com.navree.client.ui

import androidx.compose.foundation.Image
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LocalContentColor
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.MenuDefaults
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import com.navree.client.protocol.ActionRole
import com.navree.client.protocol.ButtonNode
import com.navree.client.protocol.DividerNode
import com.navree.client.protocol.FormNode
import com.navree.client.protocol.LinkNode
import com.navree.client.protocol.MenuNode
import com.navree.client.protocol.Node
import com.navree.client.protocol.Prominence
import com.navree.client.protocol.SectionNode
import com.navree.client.protocol.Submit

/** Whether a button can be used now: not disabled by the server or a command, and not waiting for an answer. */
internal fun usable(button: ButtonNode, place: Place): Boolean {
    val local = place.entry.local
    val id = button.id
    if (button.disabled || button.pending) return false
    if (id != null && (id in local.busy || local.disabled[id] == true)) return false
    val action = button.action
    return !(action is Submit && (action.form ?: place.form) in local.busy)
}

/** Whether the button's request is on its way. */
internal fun busy(button: ButtonNode, place: Place): Boolean {
    val local = place.entry.local
    val action = button.action
    return button.pending || (button.id != null && button.id in local.busy) || (action is Submit && (action.form ?: place.form) in local.busy)
}

@Composable
private fun RowScope.ButtonContent(node: ButtonNode, place: Place) {
    if (busy(node, place)) {
        CircularProgressIndicator(Modifier.size(18.dp), strokeWidth = 2.dp, color = LocalContentColor.current)
        androidx.compose.foundation.layout.Spacer(Modifier.size(ButtonDefaults.IconSpacing))
    } else {
        val vector = rememberVector(node.icon)
        if (vector != null) {
            Icon(vector, contentDescription = null, modifier = Modifier.size(ButtonDefaults.IconSize))
            androidx.compose.foundation.layout.Spacer(Modifier.size(ButtonDefaults.IconSpacing))
        }
    }
    Text(node.label)
}

/**
 * A button in the content. Its role and prominence pick the Material button:
 * primary and confirm are filled, creation is tonal, secondary is outlined,
 * cancel and the quiet ones are text buttons. A destructive button is filled
 * in the error colours when it is prominent and a text button in the error
 * colour otherwise.
 */
@Composable
internal fun ButtonView(node: ButtonNode, place: Place, modifier: Modifier = Modifier) {
    val ui = LocalUi.current
    val enabled = usable(node, place)
    val onClick = { ui.press(place, node) }
    val content: @Composable RowScope.() -> Unit = { ButtonContent(node, place) }
    val vector = rememberVector(node.icon)
    if (node.iconOnly && vector != null) {
        // The page drew the icon alone: an icon button, named for TalkBack by its label.
        IconButton(onClick = onClick, modifier = modifier, enabled = enabled) { Icon(vector, contentDescription = node.label) }
        return
    }
    when {
        node.role == ActionRole.Destructive && node.prominence == Prominence.High -> Button(
            onClick = onClick,
            modifier = modifier,
            enabled = enabled,
            colors = ButtonDefaults.buttonColors(
                containerColor = MaterialTheme.colorScheme.error,
                contentColor = MaterialTheme.colorScheme.onError,
            ),
            content = content,
        )
        node.role == ActionRole.Destructive -> TextButton(
            onClick = onClick,
            modifier = modifier,
            enabled = enabled,
            colors = ButtonDefaults.textButtonColors(contentColor = MaterialTheme.colorScheme.error),
            content = content,
        )
        node.role == ActionRole.Primary || node.role == ActionRole.Confirm || node.prominence == Prominence.High ->
            Button(onClick = onClick, modifier = modifier, enabled = enabled, content = content)
        node.role == ActionRole.Create -> FilledTonalButton(onClick = onClick, modifier = modifier, enabled = enabled, content = content)
        node.role == ActionRole.Cancel || node.prominence == Prominence.Low ->
            TextButton(onClick = onClick, modifier = modifier, enabled = enabled, content = content)
        node.role == ActionRole.Secondary -> OutlinedButton(onClick = onClick, modifier = modifier, enabled = enabled, content = content)
        else -> FilledTonalButton(onClick = onClick, modifier = modifier, enabled = enabled, content = content)
    }
}

/** A text button, as the buttons of a banner, a dialog or a sheet's foot are. */
@Composable
internal fun QuietButton(node: ButtonNode, place: Place, colour: Color = Color.Unspecified) {
    val ui = LocalUi.current
    val tint = when {
        node.role == ActionRole.Destructive -> MaterialTheme.colorScheme.error
        colour != Color.Unspecified -> colour
        else -> MaterialTheme.colorScheme.primary
    }
    TextButton(
        onClick = { ui.press(place, node) },
        enabled = usable(node, place),
        colors = ButtonDefaults.textButtonColors(contentColor = tint),
    ) { ButtonContent(node, place) }
}

@Composable
internal fun LinkView(node: LinkNode, place: Place, modifier: Modifier = Modifier) {
    val ui = LocalUi.current
    TextButton(onClick = { ui.act(place, node.action, node.id) }, modifier = modifier) { Text(node.label) }
}

/** A menu in the content: a button that opens its items under it. */
@Composable
internal fun MenuButtonView(node: MenuNode, place: Place, modifier: Modifier = Modifier) {
    var open by remember { mutableStateOf(false) }
    val vector = rememberVector(node.icon)
    Box(modifier) {
        if (vector == null) {
            OutlinedButton(onClick = { open = true }) { Text(node.label) }
        } else {
            IconButton(onClick = { open = true }) {
                Icon(vector ?: Glyphs.More, contentDescription = node.label)
            }
        }
        DropdownMenu(expanded = open, onDismissRequest = { open = false }) {
            MenuItems(node.items, place) { open = false }
        }
    }
}

/**
 * The items of a menu: buttons, sections with a heading, and dividers. A
 * destructive item is set in the error colour.
 */
@Composable
internal fun ColumnScope.MenuItems(items: List<Node>, place: Place, close: () -> Unit) {
    val ui = LocalUi.current
    for (item in items) {
        when (item) {
            is ButtonNode -> {
                val destructive = item.role == ActionRole.Destructive
                val vector = rememberVector(item.icon)
                DropdownMenuItem(
                    text = { Text(item.label) },
                    onClick = {
                        close()
                        ui.press(place, item)
                    },
                    enabled = usable(item, place),
                    leadingIcon = if (vector != null) {
                        { Icon(vector, contentDescription = null) }
                    } else {
                        null
                    },
                    colors = if (destructive) {
                        MenuDefaults.itemColors(
                            textColor = MaterialTheme.colorScheme.error,
                            leadingIconColor = MaterialTheme.colorScheme.error,
                        )
                    } else {
                        MenuDefaults.itemColors()
                    },
                )
            }
            is LinkNode -> DropdownMenuItem(
                text = { Text(item.label) },
                onClick = {
                    close()
                    ui.act(place, item.action, item.id)
                },
            )
            is DividerNode -> HorizontalDivider(Modifier.padding(vertical = 8.dp))
            is SectionNode -> {
                item.title?.let { MenuHeading(it) }
                MenuItems(item.children, place, close)
            }
            is MenuNode -> {
                MenuHeading(item.label)
                MenuItems(item.items, place, close)
            }
            else -> Unit
        }
    }
}

@Composable
internal fun MenuHeading(text: String) {
    Text(
        text,
        style = MaterialTheme.typography.labelMedium,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
        modifier = Modifier.padding(horizontal = 12.dp, vertical = 8.dp).semantics { heading() },
    )
}

/** A form that is part of other content: its fields and its button, in the order the page gave them. */
@Composable
internal fun FormView(node: FormNode, place: Place, modifier: Modifier = Modifier) {
    val inner = place.copy(form = node.key)
    Column(modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(8.dp)) {
        for (child in node.children) NodeView(child, inner)
    }
}

/** An image from the app's origin. Until it arrives, and if it never does, its place is kept. */
@Composable
internal fun RemoteImage(url: String, description: String?, cover: Boolean, modifier: Modifier = Modifier) {
    val ui = LocalUi.current
    var bitmap by remember(url) { mutableStateOf<ImageBitmap?>(null) }
    LaunchedEffect(url) {
        val bytes = ui.image(url) ?: return@LaunchedEffect
        bitmap = try {
            android.graphics.BitmapFactory.decodeByteArray(bytes, 0, bytes.size)?.asImageBitmap()
        } catch (_: RuntimeException) {
            null
        }
    }
    val loaded = bitmap
    if (loaded != null) {
        Image(
            bitmap = loaded,
            contentDescription = description,
            contentScale = if (cover) ContentScale.Crop else ContentScale.Fit,
            modifier = modifier,
        )
    } else {
        Box(modifier.semantics { })
    }
}
