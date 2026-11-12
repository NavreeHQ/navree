@file:OptIn(ExperimentalFoundationApi::class, ExperimentalLayoutApi::class)

package com.navree.client.ui

import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.background
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
import com.navree.client.design.ContentRules
import com.navree.client.design.ListRules
import com.navree.client.protocol.ActionRole
import com.navree.client.protocol.Align
import com.navree.client.protocol.AvatarNode
import com.navree.client.protocol.ButtonNode
import com.navree.client.protocol.IconNode
import com.navree.client.protocol.ImageNode
import com.navree.client.protocol.Navigate
import com.navree.client.protocol.Node
import com.navree.client.protocol.Present
import com.navree.client.protocol.RowNode
import com.navree.client.protocol.RowState
import com.navree.client.protocol.SortDir
import com.navree.client.protocol.TableNode
import com.navree.client.protocol.TableRow
import com.navree.client.protocol.TextNode
import com.navree.client.runtime.Target
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
private fun Leading(node: Node, place: Place) {
    when (node) {
        is IconNode -> IconView(node, Modifier.size(24.dp), tint = MaterialTheme.colorScheme.onSurfaceVariant)
        is AvatarNode -> AvatarView(node)
        is ImageNode -> NodeView(node, place, Modifier.size(56.dp))
        else -> NodeView(node, place)
    }
}

/** The mark of a step in a sequence: done, the one in hand, or still to come. */
@Composable
private fun StateMark(state: RowState) {
    val scheme = MaterialTheme.colorScheme
    when (state) {
        RowState.Done -> Box(Modifier.size(24.dp).clip(CircleShape).background(scheme.primary), contentAlignment = Alignment.Center) {
            Icon(Glyphs.Check, contentDescription = null, modifier = Modifier.size(16.dp), tint = scheme.onPrimary)
        }
        RowState.Current -> Box(Modifier.size(24.dp), contentAlignment = Alignment.Center) {
            Box(Modifier.size(14.dp).clip(CircleShape).background(scheme.primary))
        }
        RowState.Upcoming -> Box(Modifier.size(24.dp), contentAlignment = Alignment.Center) {
            Box(Modifier.size(14.dp).border(2.dp, scheme.outline, CircleShape))
        }
    }
}

/**
 * A row as a Material list item. A row that leads somewhere is one touch
 * target; its menu opens from the button at its end and from a long press;
 * its swipe action is also offered to TalkBack as an action of the row.
 */
@Composable
internal fun RowView(row: RowNode, place: Place, group: Group, modifier: Modifier = Modifier) {
    val ui = LocalUi.current
    val window = ui.window
    val action = row.action
    val menu = row.menu
    val swipe = row.swipe.firstOrNull()
    var menuOpen by remember { mutableStateOf(false) }
    val badge = row.badge
    // With large text there is no room for the badge beside the title; it goes under the second line.
    val badgeBelow = window.largeText
    val selected = isSelected(row, place, ui)
    val state = row.state

    val extra = buildList {
        if (swipe != null) add(CustomAccessibilityAction(swipe.label) { ui.press(place, swipe); true })
        menu.filterIsInstance<ButtonNode>().forEach { item ->
            add(CustomAccessibilityAction(item.label) { ui.press(place, item); true })
        }
    }
    val under = row.subtitle != null || row.detail.isNotEmpty() || (badge != null && badgeBelow)

    val item: @Composable () -> Unit = {
        ListItem(
            headlineContent = { Text(row.title, maxLines = 2, overflow = TextOverflow.Ellipsis) },
            supportingContent = if (under) {
                {
                    Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
                        row.subtitle?.let { Text(it, maxLines = 3, overflow = TextOverflow.Ellipsis) }
                        for (part in row.detail) NodeView(part, place)
                        if (badge != null && badgeBelow) NodeView(badge, place)
                    }
                }
            } else {
                null
            },
            leadingContent = when {
                row.leading != null -> {
                    { Leading(row.leading, place) }
                }
                state != null -> {
                    { StateMark(state) }
                }
                else -> null
            },
            trailingContent = if (row.trailing != null || (badge != null && !badgeBelow) || menu.isNotEmpty()) {
                {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Column(horizontalAlignment = Alignment.End, verticalArrangement = Arrangement.spacedBy(6.dp)) {
                            row.trailing?.let { trailing ->
                                CompositionLocalProvider(LocalContentColor provides MaterialTheme.colorScheme.onSurface) {
                                    NodeView(trailing, place)
                                }
                            }
                            if (badge != null && !badgeBelow) NodeView(badge, place)
                        }
                        if (menu.isNotEmpty()) {
                            Box {
                                IconButton(onClick = { menuOpen = true }) {
                                    Icon(Glyphs.More, contentDescription = stringResource(R.string.navree_more_options))
                                }
                                DropdownMenu(expanded = menuOpen, onDismissRequest = { menuOpen = false }) {
                                    MenuItems(menu, place) { menuOpen = false }
                                }
                            }
                        }
                    }
                }
            } else {
                null
            },
            colors = ListItemDefaults.colors(
                containerColor = if (selected) MaterialTheme.colorScheme.secondaryContainer else Color.Transparent,
            ),
            modifier = modifier
                .then(
                    when {
                        action != null && menu.isNotEmpty() -> Modifier.combinedClickable(
                            role = Role.Button,
                            onLongClick = { menuOpen = true },
                            onClick = { ui.act(place, action, row.id) },
                        )
                        action != null -> Modifier.clickable(role = Role.Button) { ui.act(place, action, row.id) }
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

/** Whether the row leads to the screen shown beside the list. */
private fun isSelected(row: RowNode, place: Place, ui: Ui): Boolean {
    val shown = place.selectedUrl ?: return false
    val href = (row.action as? Navigate)?.href ?: return false
    return (ui.origin.resolve(href, place.entry.url) as? Target.Inside)?.url == shown
}

/**
 * Swiping a row from its end runs the row's first swipe action and lets the
 * row settle back: what the action did arrives from the server.
 */
@Composable
private fun Swipeable(action: ButtonNode, place: Place, content: @Composable () -> Unit) {
    val ui = LocalUi.current
    val scope = rememberCoroutineScope()
    val state = rememberSwipeToDismissBoxState()
    val destructive = action.role == ActionRole.Destructive
    val container = if (destructive) MaterialTheme.colorScheme.errorContainer else MaterialTheme.colorScheme.primaryContainer
    val onContainer = if (destructive) MaterialTheme.colorScheme.onErrorContainer else MaterialTheme.colorScheme.onPrimaryContainer
    SwipeToDismissBox(
        state = state,
        enableDismissFromStartToEnd = false,
        backgroundContent = {
            Surface(color = container, contentColor = onContainer, modifier = Modifier.fillMaxSize()) {
                Row(
                    Modifier.fillMaxSize().padding(horizontal = 24.dp),
                    horizontalArrangement = Arrangement.End,
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    val vector = rememberVector(action.icon)
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
 * A table's row as a list row: the record's name as its first line, the
 * columns a narrow screen keeps under it, its badges in a row, its number at
 * the end. The whole row is the touch target.
 */
@Composable
internal fun TableItemView(table: TableNode, row: TableRow, place: Place) {
    val ui = LocalUi.current
    val folded = remember(table.columns, row) { ListRules.asListRow(table.columns, row) }
    val action = folded.action
    val words = folded.lines.filterIsInstance<TextNode>().map { it.value }.filter { it.isNotBlank() }
    val parts = folded.lines.filter { it !is TextNode }
    ListItem(
        headlineContent = { Text(folded.headline ?: "", maxLines = 2, overflow = TextOverflow.Ellipsis) },
        supportingContent = if (words.isNotEmpty() || parts.isNotEmpty() || folded.badges.isNotEmpty()) {
            {
                Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
                    if (words.isNotEmpty()) Text(words.joinToString(" · "), maxLines = 2, overflow = TextOverflow.Ellipsis)
                    for (part in parts) NodeView(part, place)
                    if (folded.badges.isNotEmpty()) {
                        FlowRow(horizontalArrangement = Arrangement.spacedBy(6.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                            for (badge in folded.badges) NodeView(badge, place)
                        }
                    }
                }
            }
        } else {
            null
        },
        trailingContent = folded.trailing?.let { trailing ->
            {
                CompositionLocalProvider(LocalContentColor provides MaterialTheme.colorScheme.onSurface) {
                    NodeView(trailing, place)
                }
            }
        },
        colors = ListItemDefaults.colors(containerColor = Color.Transparent),
        modifier = if (action != null) Modifier.clickable(role = Role.Button) { ui.act(place, action, row.id) } else Modifier,
    )
}

/** A table where there is room for one: a header row, then rows of cells with a line between. */
@Composable
internal fun TableView(node: TableNode, place: Place, modifier: Modifier = Modifier) {
    val ui = LocalUi.current
    val ascending = stringResource(R.string.navree_sorted_ascending)
    val descending = stringResource(R.string.navree_sorted_descending)
    fun atEnd(index: Int): Boolean {
        val column = node.columns.getOrNull(index) ?: return false
        return column.align == Align.End || (column.align == null && column.numeric)
    }
    Column(modifier.fillMaxWidth()) {
        Row(Modifier.fillMaxWidth().heightIn(min = 56.dp).padding(horizontal = 16.dp), verticalAlignment = Alignment.CenterVertically) {
            node.columns.forEachIndexed { index, column ->
                val sort = column.sort
                Row(
                    modifier = Modifier
                        .weight(1f)
                        .then(if (sort != null) Modifier.clickable(role = Role.Button) { ui.act(place, Navigate(sort.href, Present.Replace)) } else Modifier)
                        .padding(vertical = 8.dp)
                        .semantics {
                            heading()
                            when (sort?.dir) {
                                SortDir.Ascending -> stateDescription = ascending
                                SortDir.Descending -> stateDescription = descending
                                null -> Unit
                            }
                        },
                    horizontalArrangement = if (atEnd(index)) Arrangement.End else Arrangement.Start,
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Text(
                        column.label,
                        style = MaterialTheme.typography.titleSmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                    when (sort?.dir) {
                        SortDir.Ascending -> Icon(Glyphs.ArrowUp, null, Modifier.padding(start = 4.dp).size(16.dp))
                        SortDir.Descending -> Icon(Glyphs.ArrowDown, null, Modifier.padding(start = 4.dp).size(16.dp))
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
                    .then(if (action != null) Modifier.clickable(role = Role.Button) { ui.act(place, action, row.id) } else Modifier)
                    .heightIn(min = 52.dp)
                    .padding(horizontal = 16.dp, vertical = 8.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                row.cells.forEachIndexed { index, cell ->
                    val end = atEnd(index)
                    Box(Modifier.weight(1f), contentAlignment = if (end) Alignment.CenterEnd else Alignment.CenterStart) {
                        if (cell is TextNode) {
                            Text(
                                cell.value,
                                style = MaterialTheme.typography.bodyMedium,
                                textAlign = if (end) TextAlign.End else TextAlign.Start,
                                maxLines = 2,
                                overflow = TextOverflow.Ellipsis,
                            )
                        } else {
                            NodeView(cell, place)
                        }
                    }
                }
            }
            HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant)
        }
        node.summary?.let {
            Text(
                it,
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(16.dp),
            )
        }
    }
}
