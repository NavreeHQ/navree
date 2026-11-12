@file:OptIn(ExperimentalFoundationApi::class, ExperimentalLayoutApi::class)

package com.navree.client.ui

import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.ListItem
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.LocalContentColor
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.SwipeToDismissBox
import androidx.compose.material3.SwipeToDismissBoxValue
import androidx.compose.material3.Text
import androidx.compose.material3.rememberSwipeToDismissBoxState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.CustomAccessibilityAction
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.customActions
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.navree.client.R
import com.navree.client.core.Events
import com.navree.client.design.ContentRules
import com.navree.client.design.ListRules
import com.navree.client.design.ScreenRules
import com.navree.client.design.childNodes
import com.navree.protocol.Navree
import kotlinx.coroutines.launch

/** The shape of an item's container by where it sits in its group: the group reads as one rounded block. */
@Composable
internal fun groupShape(group: Group): Shape {
    val outer = 16.dp
    val inner = 4.dp
    return when (group) {
        Group.None -> RoundedCornerShape(0.dp)
        Group.Only -> RoundedCornerShape(outer)
        Group.First -> RoundedCornerShape(topStart = outer, topEnd = outer, bottomStart = inner, bottomEnd = inner)
        Group.Middle -> RoundedCornerShape(inner)
        Group.Last -> RoundedCornerShape(topStart = inner, topEnd = inner, bottomStart = outer, bottomEnd = outer)
    }
}

/** The frame of an item in a group: the group's tonal container, the screen's margins, a hair of space between items. */
@Composable
internal fun Grouped(group: Group, content: @Composable () -> Unit) {
    if (group == Group.None) {
        content()
        return
    }
    val margin = ContentRules.marginDp(LocalUi.current.window).dp
    Surface(
        color = MaterialTheme.colorScheme.surfaceContainer,
        shape = groupShape(group),
        modifier = Modifier
            .readable()
            .padding(horizontal = margin)
            .padding(bottom = if (group == Group.Last || group == Group.Only) 0.dp else 2.dp),
        content = content,
    )
}

/** What leads a row: an avatar or an image as it is, an icon in the muted colour. */
@Composable
private fun Leading(node: Navree.Node, place: Place) {
    when (node) {
        is Navree.Icon -> IconView(node, Modifier.size(24.dp), tint = MaterialTheme.colorScheme.onSurfaceVariant)
        is Navree.Avatar -> AvatarView(node)
        is Navree.Image -> NodeView(node, place, Modifier.size(56.dp))
        else -> NodeView(node, place)
    }
}

/** The mark of a step in a sequence: done, the one in hand, or still to come. */
@Composable
private fun StateMark(state: Navree.RowState) {
    val scheme = MaterialTheme.colorScheme
    when (state) {
        Navree.RowState.DONE -> Box(Modifier.size(24.dp).clip(CircleShape).background(scheme.primary), contentAlignment = Alignment.Center) {
            Icon(Glyphs.Check, contentDescription = null, modifier = Modifier.size(16.dp), tint = scheme.onPrimary)
        }
        Navree.RowState.CURRENT -> Box(Modifier.size(24.dp), contentAlignment = Alignment.Center) {
            Box(Modifier.size(14.dp).clip(CircleShape).background(scheme.primary))
        }
        Navree.RowState.UPCOMING -> Box(Modifier.size(24.dp), contentAlignment = Alignment.Center) {
            Box(Modifier.size(14.dp).border(2.dp, scheme.outline, CircleShape))
        }
    }
}

/** The menu at a row's end, opened by its button or a long press. */
@Composable
private fun RowMenu(items: List<Navree.Node>, place: Place, open: Boolean, setOpen: (Boolean) -> Unit) {
    Box {
        IconButton(onClick = { setOpen(true) }) {
            Icon(Glyphs.More, contentDescription = stringResource(R.string.navree_more_options))
        }
        DropdownMenu(expanded = open, onDismissRequest = { setOpen(false) }) {
            MenuItems(items, place) { setOpen(false) }
        }
    }
}

/** The menu's buttons offered to TalkBack as actions of the row itself. */
@Composable
private fun rowActions(items: List<Navree.Node>, place: Place): List<CustomAccessibilityAction> {
    val ui = LocalUi.current
    return items.filterIsInstance<Navree.Button>().map { item -> CustomAccessibilityAction(item.label) { ui.press(place, item); true } }
}

/**
 * A row as a Material list item. A row that leads somewhere is one touch
 * target; its menu opens from the button at its end and from a long press;
 * its swipe action is also offered to TalkBack as an action of the row. At
 * the largest text sizes the value at its end moves under its lines, so
 * neither is squeezed.
 */
@Composable
internal fun RowView(row: Navree.Row, place: Place, group: Group, modifier: Modifier = Modifier) {
    val ui = LocalUi.current
    val window = ui.window
    val action = row.action
    val menu = row.menu
    val swipe = row.swipe.firstOrNull()
    var menuOpen by remember { mutableStateOf(false) }
    val badge = row.badge
    val below = ListRules.trailingBelow(window)
    val selected = isSelected(row.action, place) || place.screen.has(row.id, Navree.State.SELECTED)
    val state = row.state
    val extra = (if (swipe != null) listOf(CustomAccessibilityAction(swipe.label) { ui.press(place, swipe); true }) else emptyList()) + rowActions(menu, place)
    val under = row.subtitle != null || row.detail.isNotEmpty() || (below && (badge != null || row.trailing != null))

    val item: @Composable () -> Unit = {
        ListItem(
            headlineContent = { Text(row.title, maxLines = 2, overflow = TextOverflow.Ellipsis) },
            supportingContent = if (under) {
                {
                    Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
                        row.subtitle?.let { Text(it, maxLines = 3, overflow = TextOverflow.Ellipsis) }
                        for (part in row.detail) NodeView(part, place)
                        if (below) {
                            row.trailing?.let { NodeView(it, place) }
                            badge?.let { StatusView(it) }
                        }
                    }
                }
            } else {
                null
            },
            leadingContent = when {
                row.leading != null -> {
                    { Leading(row.leading!!, place) }
                }
                state != null -> {
                    { StateMark(state) }
                }
                else -> null
            },
            trailingContent = if ((!below && (row.trailing != null || badge != null)) || menu.isNotEmpty()) {
                {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        if (!below) {
                            Column(horizontalAlignment = Alignment.End, verticalArrangement = Arrangement.spacedBy(6.dp)) {
                                row.trailing?.let { trailing ->
                                    CompositionLocalProvider(LocalContentColor provides MaterialTheme.colorScheme.onSurface) { NodeView(trailing, place) }
                                }
                                badge?.let { StatusView(it) }
                            }
                        }
                        if (menu.isNotEmpty()) RowMenu(menu, place, menuOpen) { menuOpen = it }
                    }
                }
            } else {
                null
            },
            colors = ListItemDefaults.colors(containerColor = if (selected) MaterialTheme.colorScheme.secondaryContainer else Color.Transparent),
            modifier = modifier
                .then(
                    when {
                        action != null && menu.isNotEmpty() -> Modifier.combinedClickable(
                            role = Role.Button,
                            onLongClick = { menuOpen = true },
                            onClick = { ui.act(place, action) },
                        )
                        action != null -> Modifier.clickable(role = Role.Button) { ui.act(place, action) }
                        else -> Modifier
                    },
                )
                .semantics {
                    if (extra.isNotEmpty()) customActions = extra
                    if (selected) this.selected = true
                },
        )
    }

    Grouped(group) {
        if (swipe != null) Swipeable(swipe, place, item) else item()
    }
}

/** Whether a row leads to the screen shown beside the list. */
private fun isSelected(action: Navree.Action?, place: Place): Boolean {
    val shown = place.selectedUrl ?: return false
    val href = (action as? Navree.Action.Navigate)?.href ?: return false
    return shown == href || shown.endsWith(href)
}

/**
 * Swiping a row from its end runs the row's first swipe action and lets the
 * row settle back: what the action did arrives from the server.
 */
@Composable
private fun Swipeable(action: Navree.Button, place: Place, content: @Composable () -> Unit) {
    val ui = LocalUi.current
    val scope = rememberCoroutineScope()
    val state = rememberSwipeToDismissBoxState()
    val destructive = ScreenRules.destructive(action)
    val container = if (destructive) MaterialTheme.colorScheme.errorContainer else MaterialTheme.colorScheme.primaryContainer
    val onContainer = if (destructive) MaterialTheme.colorScheme.onErrorContainer else MaterialTheme.colorScheme.onPrimaryContainer
    SwipeToDismissBox(
        state = state,
        enableDismissFromStartToEnd = false,
        backgroundContent = {
            Surface(color = container, contentColor = onContainer, modifier = Modifier.fillMaxSize()) {
                Row(Modifier.fillMaxSize().padding(horizontal = 24.dp), horizontalArrangement = Arrangement.End, verticalAlignment = Alignment.CenterVertically) {
                    val vector = rememberIcon(action.icon, action.standard)
                    if (vector != null) Icon(vector, contentDescription = null) else Text(action.label, style = MaterialTheme.typography.labelLarge)
                }
            }
        },
        onDismiss = { value ->
            if (value == SwipeToDismissBoxValue.EndToStart) ui.press(place, action)
            scope.launch { state.reset() }
        },
    ) {
        Surface(color = MaterialTheme.colorScheme.surface) { content() }
    }
}

/**
 * A table's row as a list row: its leading cell first, the record's name as
 * its first line, the columns a narrow screen keeps under it, its statuses on
 * a line of their own, its number at the end and its actions in a menu. The
 * whole row is the touch target and opens what the server says it opens.
 */
@Composable
internal fun TableItemView(table: Navree.Table, row: Navree.TableRow, place: Place) {
    val ui = LocalUi.current
    val folded = remember(table.columns, row) { ListRules.asListRow(table.columns, row) }
    val action = folded.action
    val words = folded.lines.filterIsInstance<Navree.Text>().map { it.value }.filter { it.isNotBlank() }
    val parts = folded.lines.filter { it !is Navree.Text }
    val below = ListRules.trailingBelow(ui.window)
    var menuOpen by remember { mutableStateOf(false) }
    val extra = rowActions(folded.menu, place)
    val selected = isSelected(action, place)
    ListItem(
        headlineContent = { Text(folded.headline ?: "", maxLines = 2, overflow = TextOverflow.Ellipsis) },
        supportingContent = if (words.isNotEmpty() || parts.isNotEmpty() || folded.statuses.isNotEmpty() || (below && folded.trailing != null)) {
            {
                Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
                    if (words.isNotEmpty()) Text(words.joinToString(" · "), maxLines = 2, overflow = TextOverflow.Ellipsis)
                    for (part in parts) NodeView(part, place)
                    if (folded.statuses.isNotEmpty()) {
                        FlowRow(horizontalArrangement = Arrangement.spacedBy(12.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                            for (status in folded.statuses) {
                                if (status is Navree.Badge) StatusView(status) else NodeView(status, place)
                            }
                        }
                    }
                    if (below) folded.trailing?.let { NodeView(it, place) }
                }
            }
        } else {
            null
        },
        leadingContent = folded.leading?.let { leading -> { Leading(leading, place) } },
        trailingContent = if ((!below && folded.trailing != null) || folded.menu.isNotEmpty()) {
            {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    if (!below) {
                        folded.trailing?.let { trailing ->
                            CompositionLocalProvider(LocalContentColor provides MaterialTheme.colorScheme.onSurface) { NodeView(trailing, place) }
                        }
                    }
                    if (folded.menu.isNotEmpty()) RowMenu(folded.menu, place, menuOpen) { menuOpen = it }
                }
            }
        } else {
            null
        },
        colors = ListItemDefaults.colors(containerColor = if (selected) MaterialTheme.colorScheme.secondaryContainer else Color.Transparent),
        modifier = Modifier
            .then(
                when {
                    action != null && folded.menu.isNotEmpty() -> Modifier.combinedClickable(role = Role.Button, onLongClick = { menuOpen = true }) { ui.act(place, action) }
                    action != null -> Modifier.clickable(role = Role.Button) { ui.act(place, action) }
                    else -> Modifier
                },
            )
            .semantics {
                if (extra.isNotEmpty()) customActions = extra
                if (selected) this.selected = true
            },
    )
}

/** A table where there is room for one: a header row, then rows of cells with a line between. */
@Composable
internal fun TableView(node: Navree.Table, place: Place, modifier: Modifier = Modifier) {
    val ui = LocalUi.current
    val ascending = stringResource(R.string.navree_sorted_ascending)
    val descending = stringResource(R.string.navree_sorted_descending)
    fun atEnd(index: Int): Boolean {
        val column = node.columns.getOrNull(index) ?: return false
        return column.align == Navree.Align.END || column.role == Navree.ColumnRole.TRAILING || (column.align == null && column.numeric)
    }
    // A table's sort goes to another address of the table's own region.
    val region = place.screen.screen?.body?.let { body -> regionOf(body, node) }
    Column(modifier.fillMaxWidth()) {
        Row(Modifier.fillMaxWidth().heightIn(min = 56.dp).padding(horizontal = 16.dp), verticalAlignment = Alignment.CenterVertically) {
            node.columns.forEachIndexed { index, column ->
                val sort = column.sort
                Row(
                    modifier = Modifier
                        .weight(if (column.role == Navree.ColumnRole.ACTIONS) 0.5f else 1f)
                        .then(
                            if (sort != null) {
                                Modifier.clickable(role = Role.Button) {
                                    if (region != null) {
                                        ui.send(Events.regionGo(place.id, region, sort.href))
                                    } else {
                                        ui.act(place, Navree.Action.Navigate(sort.href, Navree.Present.REPLACE))
                                    }
                                }
                            } else {
                                Modifier
                            },
                        )
                        .padding(vertical = 8.dp)
                        .semantics {
                            heading()
                            when (sort?.dir) {
                                Navree.SortDir.ASC -> stateDescription = ascending
                                Navree.SortDir.DESC -> stateDescription = descending
                                null -> Unit
                            }
                        },
                    horizontalArrangement = if (atEnd(index)) Arrangement.End else Arrangement.Start,
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Text(column.label, style = MaterialTheme.typography.titleSmall, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 1, overflow = TextOverflow.Ellipsis)
                    when (sort?.dir) {
                        Navree.SortDir.ASC -> Icon(Glyphs.ArrowUp, null, Modifier.padding(start = 4.dp).size(16.dp))
                        Navree.SortDir.DESC -> Icon(Glyphs.ArrowDown, null, Modifier.padding(start = 4.dp).size(16.dp))
                        null -> Unit
                    }
                }
            }
        }
        HorizontalDivider()
        for (row in node.rows) {
            val action = row.action
            Row(
                Modifier
                    .fillMaxWidth()
                    .then(if (action != null) Modifier.clickable(role = Role.Button) { ui.act(place, action) } else Modifier)
                    .heightIn(min = 52.dp)
                    .padding(horizontal = 16.dp, vertical = 8.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                row.cells.forEachIndexed { index, cell ->
                    val end = atEnd(index)
                    val column = node.columns.getOrNull(index)
                    Box(Modifier.weight(if (column?.role == Navree.ColumnRole.ACTIONS) 0.5f else 1f), contentAlignment = if (end) Alignment.CenterEnd else Alignment.CenterStart) {
                        when (cell) {
                            is Navree.Text -> Text(
                                cell.value,
                                style = MaterialTheme.typography.bodyMedium,
                                color = textColour(cell.tone, Navree.TextStyle.CALLOUT, cell.muted),
                                textAlign = if (end) TextAlign.End else TextAlign.Start,
                                maxLines = 2,
                                overflow = TextOverflow.Ellipsis,
                            )
                            is Navree.Badge -> StatusView(cell)
                            else -> NodeView(cell, place)
                        }
                    }
                }
            }
            HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant)
        }
        node.summary?.let {
            Text(it, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.padding(16.dp))
        }
    }
}

/** The id of the region a node stands in, when it stands in one. */
private fun regionOf(root: Navree.Node, target: Navree.Node): String? {
    fun visit(node: Navree.Node, region: String?): String? {
        if (node === target) return region
        val inner = (node as? Navree.Region)?.id ?: region
        for (child in node.childNodes()) visit(child, inner)?.let { return it }
        return null
    }
    return visit(root, null)
}
