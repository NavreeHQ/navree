package com.navree.client.ui

import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.Spacer
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
import androidx.compose.material3.PlainTooltip
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TooltipBox
import androidx.compose.material3.TooltipDefaults
import androidx.compose.material3.rememberTooltipState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.toggleableState
import androidx.compose.ui.state.ToggleableState
import androidx.compose.ui.unit.dp
import com.navree.client.design.ScreenRules
import com.navree.protocol.Navree

/** Whether a button can be used now: not disabled, and not waiting for an answer. */
internal fun usable(button: Navree.Button, place: Place): Boolean =
    !button.disabled && !button.pending && !(button.action is Navree.Action.Submit && place.pending)

/** Whether the button's request is on its way. */
internal fun busy(button: Navree.Button, place: Place): Boolean =
    button.pending || (button.action is Navree.Action.Submit && place.pending)

@Composable
private fun RowScope.ButtonContent(node: Navree.Button, place: Place) {
    if (busy(node, place)) {
        CircularProgressIndicator(Modifier.size(18.dp), strokeWidth = 2.dp, color = LocalContentColor.current)
        Spacer(Modifier.size(ButtonDefaults.IconSpacing))
    } else {
        val vector = rememberIcon(node.icon, node.standard)
        if (vector != null) {
            Icon(vector, contentDescription = null, modifier = Modifier.size(ButtonDefaults.IconSize))
            Spacer(Modifier.size(ButtonDefaults.IconSpacing))
        }
    }
    Text(node.label)
}

/** An icon button with its name as a tooltip on a long press or a hover, as Material asks of icon-only buttons. */
@Composable
internal fun TipIconButton(
    label: String,
    icon: ImageVector,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
    tint: Color = Color.Unspecified,
) {
    TooltipBox(
        positionProvider = TooltipDefaults.rememberPlainTooltipPositionProvider(),
        tooltip = { PlainTooltip { Text(label) } },
        state = rememberTooltipState(),
    ) {
        IconButton(onClick = onClick, modifier = modifier, enabled = enabled) {
            Icon(icon, contentDescription = label, tint = if (tint == Color.Unspecified) LocalContentColor.current else tint)
        }
    }
}

/**
 * A button in the content. Its role and prominence pick the Material button:
 * primary and confirm are filled, creation is tonal, secondary is outlined,
 * cancel and the quiet ones are text buttons. A destructive action is never
 * prominent: it is a text button in the error colour, and asks before it runs.
 */
@Composable
internal fun ButtonView(node: Navree.Button, place: Place, modifier: Modifier = Modifier) {
    val ui = LocalUi.current
    val enabled = usable(node, place)
    val onClick = { ui.press(place, node) }
    val content: @Composable RowScope.() -> Unit = { ButtonContent(node, place) }
    val vector = rememberIcon(node.icon, node.standard)
    if (node.iconOnly && vector != null) {
        // The page drew the icon alone: an icon button, named for TalkBack and its tooltip by its label.
        TipIconButton(node.label, vector, onClick, modifier, enabled, tint = if (ScreenRules.destructive(node)) MaterialTheme.colorScheme.error else Color.Unspecified)
        return
    }
    when {
        ScreenRules.destructive(node) -> TextButton(
            onClick = onClick,
            modifier = modifier,
            enabled = enabled,
            colors = ButtonDefaults.textButtonColors(contentColor = MaterialTheme.colorScheme.error),
            content = content,
        )
        node.role == Navree.Role.PRIMARY || node.role == Navree.Role.CONFIRM || node.prominence == Navree.Prominence.HIGH ->
            Button(onClick = onClick, modifier = modifier, enabled = enabled, content = content)
        node.role == Navree.Role.CREATE -> FilledTonalButton(onClick = onClick, modifier = modifier, enabled = enabled, content = content)
        node.role == Navree.Role.CANCEL || node.prominence == Navree.Prominence.LOW ->
            TextButton(onClick = onClick, modifier = modifier, enabled = enabled, content = content)
        node.role == Navree.Role.SECONDARY -> OutlinedButton(onClick = onClick, modifier = modifier, enabled = enabled, content = content)
        else -> FilledTonalButton(onClick = onClick, modifier = modifier, enabled = enabled, content = content)
    }
}

/** A text button, as the buttons of a banner, a dialog or a sheet's foot are. */
@Composable
internal fun QuietButton(node: Navree.Button, place: Place, colour: Color = Color.Unspecified) {
    val ui = LocalUi.current
    val tint = when {
        ScreenRules.destructive(node) -> MaterialTheme.colorScheme.error
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
internal fun LinkView(node: Navree.Link, place: Place, modifier: Modifier = Modifier) {
    val ui = LocalUi.current
    if (node.children.isEmpty()) {
        TextButton(onClick = { ui.act(place, node.action) }, modifier = modifier) { Text(node.label) }
        return
    }
    // A link that holds parts (a ring and its words) is one touch target with the shape of a card's item.
    Box(
        modifier
            .clip(MaterialTheme.shapes.medium)
            .clickable(role = androidx.compose.ui.semantics.Role.Button) { ui.act(place, node.action) }
            .semantics(mergeDescendants = true) {}
            .padding(vertical = 8.dp, horizontal = 4.dp),
        contentAlignment = Alignment.Center,
    ) {
        Column(horizontalAlignment = Alignment.CenterHorizontally) {
            for (child in node.children) NodeView(child, place)
        }
    }
}

/** A menu in the content: a button that opens its items under it. */
@Composable
internal fun MenuButtonView(node: Navree.Menu, place: Place, modifier: Modifier = Modifier) {
    var open by remember { mutableStateOf(false) }
    val vector = rememberIcon(node.icon)
    Box(modifier) {
        if (vector == null) {
            OutlinedButton(onClick = { open = true }) { Text(node.label) }
        } else {
            TipIconButton(node.label, vector, { open = true })
        }
        DropdownMenu(expanded = open, onDismissRequest = { open = false }) {
            MenuItems(node.items, place) { open = false }
        }
    }
}

/**
 * The items of a menu: buttons, sections with a heading, and dividers. A
 * destructive item is set in the error colour and comes after a divider.
 */
@Composable
internal fun ColumnScope.MenuItems(items: List<Navree.Node>, place: Place, close: () -> Unit) {
    val ui = LocalUi.current
    for (item in items) {
        when (item) {
            is Navree.Button -> {
                val destructive = ScreenRules.destructive(item)
                val vector = rememberIcon(item.icon, item.standard)
                val checked = item.checked
                // A ticked item: a check box, or a radio button among the items of one `one_of`.
                val mark: ImageVector? = when {
                    checked == null -> null
                    item.oneOf != null -> if (checked) Glyphs.RadioOn else Glyphs.RadioOff
                    else -> if (checked) Glyphs.CheckBox else Glyphs.CheckBoxOff
                }
                DropdownMenuItem(
                    text = { Text(item.label) },
                    onClick = {
                        close()
                        ui.press(place, item)
                    },
                    enabled = usable(item, place),
                    modifier = if (checked != null) {
                        Modifier.semantics {
                            role = if (item.oneOf != null) Role.RadioButton else Role.Checkbox
                            if (item.oneOf != null) selected = checked else toggleableState = ToggleableState(checked)
                        }
                    } else {
                        Modifier
                    },
                    leadingIcon = when {
                        mark != null -> {
                            { Icon(mark, contentDescription = null, tint = if (checked == true) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurfaceVariant) }
                        }
                        vector != null -> {
                            { Icon(vector, contentDescription = null) }
                        }
                        else -> null
                    },
                    colors = if (destructive) {
                        MenuDefaults.itemColors(textColor = MaterialTheme.colorScheme.error, leadingIconColor = MaterialTheme.colorScheme.error)
                    } else {
                        MenuDefaults.itemColors()
                    },
                )
            }
            is Navree.Link -> DropdownMenuItem(
                text = { Text(item.label) },
                onClick = {
                    close()
                    ui.act(place, item.action)
                },
            )
            is Navree.Divider -> HorizontalDivider(Modifier.padding(vertical = 8.dp))
            is Navree.Section -> {
                item.title?.let { MenuHeading(it) }
                MenuItems(item.children, place, close)
            }
            is Navree.Menu -> {
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
internal fun FormView(node: Navree.Form, place: Place, modifier: Modifier = Modifier) {
    val inner = place.copy(form = node.id)
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
        Image(bitmap = loaded, contentDescription = description, contentScale = if (cover) ContentScale.Crop else ContentScale.Fit, modifier = modifier)
    } else {
        // Its place is kept, tinted, so a picture still on its way reads as one.
        Box(modifier.background(MaterialTheme.colorScheme.surfaceContainerHighest).semantics { })
    }
}
