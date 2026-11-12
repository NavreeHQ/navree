@file:OptIn(ExperimentalLayoutApi::class)

package com.navree.client.ui

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.IntrinsicSize
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.ListItem
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.LocalContentColor
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.SecondaryTabRow
import androidx.compose.material3.Surface
import androidx.compose.material3.Tab
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.rotate
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.TextLinkStyles
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.LinkAnnotation
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.text.withLink
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.dp
import com.navree.client.R
import com.navree.client.design.ContentRules
import com.navree.client.design.ListRules
import com.navree.client.design.ToneRole
import com.navree.client.design.TypeRole
import com.navree.client.protocol.Align
import com.navree.client.protocol.AvatarNode
import com.navree.client.protocol.Axis
import com.navree.client.protocol.BadgeNode
import com.navree.client.protocol.BannerNode
import com.navree.client.protocol.ButtonNode
import com.navree.client.protocol.ChartNode
import com.navree.client.protocol.CodeNode
import com.navree.client.protocol.DialogNode
import com.navree.client.protocol.DisclosureNode
import com.navree.client.protocol.DividerNode
import com.navree.client.protocol.EmptyNode
import com.navree.client.protocol.FactsNode
import com.navree.client.protocol.FieldNode
import com.navree.client.protocol.FormNode
import com.navree.client.protocol.Fit
import com.navree.client.protocol.GridNode
import com.navree.client.protocol.IconNode
import com.navree.client.protocol.ImageNode
import com.navree.client.protocol.LinkNode
import com.navree.client.protocol.ListNode
import com.navree.client.protocol.MenuNode
import com.navree.client.protocol.NativeNode
import com.navree.client.protocol.Justify
import com.navree.client.protocol.Navigate
import com.navree.client.protocol.Present
import com.navree.client.protocol.Node
import com.navree.client.protocol.PopoverNode
import com.navree.client.protocol.ProgressNode
import com.navree.client.protocol.RegionNode
import com.navree.client.protocol.RichNode
import com.navree.client.protocol.RowNode
import com.navree.client.protocol.SectionNode
import com.navree.client.protocol.SectionStyle
import com.navree.client.protocol.SheetNode
import com.navree.client.protocol.SpacerNode
import com.navree.client.protocol.SplitNode
import com.navree.client.protocol.StackNode
import com.navree.client.protocol.TableNode
import com.navree.client.protocol.TabsNode
import com.navree.client.protocol.TextLevel
import com.navree.client.protocol.TextNode
import com.navree.client.protocol.Tone
import com.navree.client.protocol.UnknownNode
import com.navree.client.protocol.WebNode
import com.navree.client.runtime.Event
import com.navree.client.theme.toneColours

/** The Material type style a text level is set in. */
@Composable
internal fun typeOf(level: TextLevel): TextStyle = when (ContentRules.type(level)) {
    TypeRole.HeadlineMedium -> MaterialTheme.typography.headlineMedium
    TypeRole.TitleLarge -> MaterialTheme.typography.titleLarge
    TypeRole.TitleMedium -> MaterialTheme.typography.titleMedium
    TypeRole.BodyLarge -> MaterialTheme.typography.bodyLarge
    TypeRole.BodyMedium -> MaterialTheme.typography.bodyMedium
    TypeRole.BodySmall -> MaterialTheme.typography.bodySmall
    TypeRole.Mono -> MaterialTheme.typography.bodyMedium.copy(fontFamily = FontFamily.Monospace)
}

/** The colour text of a tone is set in, on a plain surface. */
@Composable
internal fun textColour(tone: Tone, level: TextLevel, muted: Boolean = false): Color = when (tone) {
    Tone.Neutral -> if (muted || level == TextLevel.Caption) MaterialTheme.colorScheme.onSurfaceVariant else LocalContentColor.current
    Tone.Accent -> MaterialTheme.colorScheme.primary
    Tone.Danger -> MaterialTheme.colorScheme.error
    // On a plain surface the container's own content colour is the readable one.
    else -> toneColours(ContentRules.tone(tone)).content.let { colour ->
        if (colour == Color.Unspecified) LocalContentColor.current else colour
    }
}

/** Draws any node. What has its own place on the screen's frame (sheets, dialogs) draws nothing here. */
@Composable
internal fun NodeView(node: Node, place: Place, modifier: Modifier = Modifier) {
    val local = place.entry.local
    if (node === place.omit || !inFlow(node, local)) return
    when (node) {
        is TextNode -> Text(
            text = node.id?.let { local.texts[it] } ?: node.value,
            modifier = if (node.heading != null || node.style == TextLevel.LargeTitle || node.style == TextLevel.Title) {
                modifier.semantics { heading() }
            } else {
                modifier
            },
            style = typeOf(node.style),
            color = textColour(node.tone, node.style, node.muted),
            maxLines = node.lines ?: Int.MAX_VALUE,
            overflow = TextOverflow.Ellipsis,
        )
        is RichNode -> RichView(node, place, modifier)
        is ImageNode -> {
            val width = node.width
            val height = node.height
            val shaped = if (width != null && height != null && height > 0) modifier.aspectRatio(width.toFloat() / height) else modifier
            RemoteImage(
                url = node.src,
                description = node.alt.ifEmpty { null },
                cover = node.fit != Fit.Contain,
                modifier = shaped.clip(MaterialTheme.shapes.medium),
            )
        }
        is IconNode -> IconView(node, modifier)
        is AvatarNode -> AvatarView(node, modifier)
        is BadgeNode -> BadgeView(node, place, modifier)
        is ProgressNode -> ProgressView(node, modifier)
        is BannerNode -> BannerView(node, place, modifier)
        is EmptyNode -> EmptyView(node, place, modifier)
        is FactsNode -> FactsView(node, place, modifier)
        is TableNode -> TableView(node, place, modifier)
        is CodeNode -> CodeView(node, modifier)
        is ButtonNode -> ButtonView(node, place, modifier)
        is LinkNode -> LinkView(node, place, modifier)
        is MenuNode -> MenuButtonView(node, place, modifier)
        is FormNode -> FormView(node, place, modifier)
        is FieldNode -> FieldView(node, place, modifier)
        is SectionNode -> SectionView(node, place, modifier)
        is GridNode -> GridView(node, place, modifier)
        is StackNode -> StackView(node, place, modifier)
        is RegionNode -> Column(modifier, verticalArrangement = Arrangement.spacedBy(12.dp)) {
            for (child in node.children) NodeView(child, place)
        }
        is ListNode -> Column(modifier) {
            for (child in node.children) {
                if (child is RowNode) RowView(child, place, Group.None) else NodeView(child, place)
            }
        }
        is RowNode -> RowView(node, place, Group.None, modifier)
        is SplitNode -> SplitView(node, place, modifier)
        is TabsNode -> TabsView(node, place, modifier)
        is DisclosureNode -> DisclosureView(node, place, modifier)
        is DividerNode -> HorizontalDivider(modifier.padding(vertical = 8.dp))
        is SpacerNode -> Spacer(modifier.height(16.dp))
        is NativeNode -> node.alt?.let { NodeView(it, place, modifier) }
        is UnknownNode -> node.alt?.let { NodeView(it, place, modifier) }
        // A screen that holds one of these is shown in the web view, so none reaches here.
        is ChartNode, is WebNode -> Unit
        is SheetNode, is DialogNode, is PopoverNode -> Unit
    }
}

@Composable
private fun RichView(node: RichNode, place: Place, modifier: Modifier) {
    val ui = LocalUi.current
    val link = MaterialTheme.colorScheme.primary
    val code = MaterialTheme.colorScheme.surfaceContainerHighest
    val text = remember(node, link, code) {
        buildAnnotatedString {
            for (run in node.runs) {
                val style = SpanStyle(
                    fontWeight = if (run.bold) FontWeight.Bold else null,
                    fontStyle = if (run.italic) FontStyle.Italic else null,
                    fontFamily = if (run.code) FontFamily.Monospace else null,
                    background = if (run.code) code else Color.Unspecified,
                )
                val href = run.href
                if (href != null) {
                    val styles = TextLinkStyles(style.copy(color = link, textDecoration = TextDecoration.Underline))
                    withLink(LinkAnnotation.Clickable(tag = href, styles = styles) { ui.act(place, Navigate(href)) }) {
                        append(run.text)
                    }
                } else {
                    withStyle(style) { append(run.text) }
                }
            }
        }
    }
    Text(
        text = text,
        modifier = modifier,
        style = typeOf(node.style),
        color = if (node.muted) MaterialTheme.colorScheme.onSurfaceVariant else LocalContentColor.current,
    )
}

@Composable
internal fun IconView(node: IconNode, modifier: Modifier = Modifier, tint: Color = LocalContentColor.current) {
    val vector = rememberVector(node) ?: return
    Icon(imageVector = vector, contentDescription = node.label, modifier = modifier, tint = tint)
}

@Composable
private fun ProgressView(node: ProgressNode, modifier: Modifier) {
    val described = modifier.fillMaxWidth().semantics { node.label?.let { contentDescription = it } }
    val value = node.value
    if (value != null) {
        LinearProgressIndicator(progress = { (value / 1000f).coerceIn(0f, 1f) }, modifier = described)
    } else {
        LinearProgressIndicator(modifier = described)
    }
}

@Composable
private fun BannerView(node: BannerNode, place: Place, modifier: Modifier) {
    val colours = toneColours(ContentRules.tone(node.tone))
    val urgent = node.tone == Tone.Danger || node.tone == Tone.Warning
    Card(
        modifier = modifier.fillMaxWidth().semantics {
            liveRegion = if (urgent) LiveRegionMode.Assertive else LiveRegionMode.Polite
        },
        colors = CardDefaults.cardColors(containerColor = colours.container, contentColor = colours.content),
    ) {
        Row(Modifier.padding(start = 16.dp, top = 16.dp, end = 16.dp, bottom = if (node.actions.isEmpty()) 16.dp else 4.dp)) {
            node.icon?.let { icon ->
                IconView(icon, Modifier.padding(end = 16.dp).size(24.dp))
            }
            Column(Modifier.weight(1f)) {
                node.title?.let { Text(it, style = MaterialTheme.typography.titleMedium) }
                node.text?.let {
                    Text(it, style = MaterialTheme.typography.bodyMedium, modifier = Modifier.padding(top = if (node.title != null) 4.dp else 0.dp))
                }
            }
        }
        if (node.actions.isNotEmpty()) {
            Row(
                Modifier.fillMaxWidth().padding(horizontal = 8.dp).padding(bottom = 8.dp),
                horizontalArrangement = Arrangement.End,
            ) {
                for (action in node.actions) {
                    if (action is ButtonNode) QuietButton(action, place, colours.content)
                }
            }
        }
    }
}

@Composable
internal fun EmptyView(node: EmptyNode, place: Place, modifier: Modifier = Modifier) {
    Column(
        modifier = modifier.fillMaxWidth().padding(horizontal = 32.dp, vertical = 32.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        node.icon?.let { icon ->
            IconView(icon, Modifier.size(48.dp), tint = MaterialTheme.colorScheme.onSurfaceVariant)
            Spacer(Modifier.height(16.dp))
        }
        Text(
            node.title,
            style = MaterialTheme.typography.titleLarge,
            textAlign = androidx.compose.ui.text.style.TextAlign.Center,
            modifier = Modifier.semantics { heading() },
        )
        node.text?.let {
            Text(
                it,
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                textAlign = androidx.compose.ui.text.style.TextAlign.Center,
                modifier = Modifier.padding(top = 8.dp),
            )
        }
        if (node.actions.isNotEmpty()) {
            Spacer(Modifier.height(24.dp))
            for (action in node.actions) NodeView(action, place)
        }
    }
}

/**
 * Facts: each a small label over its value, with an icon at the start when it
 * has one and its other lines under it. TalkBack reads a fact as one thing.
 */
@Composable
private fun FactsView(node: FactsNode, place: Place, modifier: Modifier) {
    Column(modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(16.dp)) {
        for (fact in node.items) {
            val icon = fact.icon
            Row(Modifier.fillMaxWidth().semantics(mergeDescendants = true) {}, horizontalArrangement = Arrangement.spacedBy(16.dp)) {
                if (icon != null) {
                    val colours = toneColours(ContentRules.tone(fact.tone))
                    Box(
                        Modifier.size(40.dp).clip(CircleShape).background(colours.container),
                        contentAlignment = Alignment.Center,
                    ) {
                        IconView(icon, Modifier.size(20.dp), tint = if (fact.tone == Tone.Neutral) MaterialTheme.colorScheme.onSurfaceVariant else colours.content)
                    }
                }
                Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                    Text(fact.label, style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                    for (value in fact.value) NodeView(value, place)
                    for (line in fact.details) {
                        Text(line, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                    }
                }
            }
        }
    }
}

@Composable
private fun CodeView(node: CodeNode, modifier: Modifier) {
    Surface(modifier = modifier.fillMaxWidth(), color = MaterialTheme.colorScheme.surfaceContainerHigh, shape = MaterialTheme.shapes.small) {
        SelectionContainer {
            Text(
                node.text,
                style = MaterialTheme.typography.bodyMedium.copy(fontFamily = FontFamily.Monospace),
                modifier = Modifier.horizontalScroll(rememberScrollState()).padding(12.dp),
            )
        }
    }
}

@Composable
private fun SectionView(node: SectionNode, place: Place, modifier: Modifier) {
    val content: @Composable () -> Unit = {
        Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
            val title = node.title
            if (title != null || node.actions.isNotEmpty()) {
                Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                    Column(Modifier.weight(1f)) {
                        title?.let { Text(it, style = MaterialTheme.typography.titleMedium, modifier = Modifier.semantics { heading() }) }
                        node.subtitle?.let {
                            Text(it, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                        }
                    }
                    for (action in node.actions) NodeView(action, place)
                }
            }
            for (child in node.children) NodeView(child, place)
            for (child in node.footer) NodeView(child, place)
        }
    }
    if (node.style == SectionStyle.Card) {
        Card(modifier.fillMaxWidth()) { Box(Modifier.padding(16.dp)) { content() } }
    } else {
        Box(modifier.fillMaxWidth()) { content() }
    }
}

@Composable
private fun GridView(node: GridNode, place: Place, modifier: Modifier) {
    val gap = ContentRules.gap(node.gap)
    BoxWithConstraints(modifier.fillMaxWidth()) {
        val columns = ListRules.gridColumns(maxWidth.value.toInt(), node.min, gap, node.cols)
        val shown = node.children.filter { inFlow(it, place.entry.local) }
        Column(verticalArrangement = Arrangement.spacedBy(gap.dp)) {
            for (line in shown.chunked(columns)) {
                // The parts of a line are as tall as the tallest of them.
                Row(Modifier.height(IntrinsicSize.Min), horizontalArrangement = Arrangement.spacedBy(gap.dp)) {
                    for (child in line) {
                        Box(Modifier.weight(1f).fillMaxHeight()) { NodeView(child, place, Modifier.fillMaxHeight()) }
                    }
                    // An unfinished last line keeps its columns the width of the others.
                    repeat(columns - line.size) { Spacer(Modifier.weight(1f)) }
                }
            }
        }
    }
}

/**
 * A stack inside other content. Across, its parts wrap when the page lets
 * them, and "between" puts the first at the start and the last at the end.
 */
@Composable
private fun StackView(node: StackNode, place: Place, modifier: Modifier) {
    val gap = ContentRules.gap(node.gap).dp
    val shown = node.children.filter { it !== place.omit && inFlow(it, place.entry.local) }
    if (shown.isEmpty()) return
    if (node.axis == Axis.Horizontal) {
        val across = when (node.justify) {
            Justify.Between -> if (shown.size > 1) Arrangement.SpaceBetween else Arrangement.Start
            Justify.Center -> Arrangement.spacedBy(gap, Alignment.CenterHorizontally)
            Justify.End -> Arrangement.spacedBy(gap, Alignment.End)
            else -> Arrangement.spacedBy(gap)
        }
        val upright = when (node.align) {
            Align.Start -> Alignment.Top
            Align.End -> Alignment.Bottom
            else -> Alignment.CenterVertically
        }
        if (node.wrap) {
            FlowRow(
                modifier = if (node.justify == Justify.Between || node.justify == Justify.End) modifier.fillMaxWidth() else modifier,
                horizontalArrangement = across,
                verticalArrangement = Arrangement.spacedBy(gap),
                itemVerticalAlignment = upright,
            ) {
                for (child in shown) NodeView(child, place)
            }
        } else {
            Row(
                modifier = if (node.justify == Justify.Between || node.justify == Justify.End) modifier.fillMaxWidth() else modifier,
                horizontalArrangement = across,
                verticalAlignment = upright,
            ) {
                for (child in shown) NodeView(child, place)
            }
        }
    } else {
        Column(
            modifier = modifier,
            verticalArrangement = Arrangement.spacedBy(gap),
            horizontalAlignment = when (node.align) {
                Align.Center -> Alignment.CenterHorizontally
                Align.End -> Alignment.End
                else -> Alignment.Start
            },
        ) {
            for (child in shown) NodeView(child, place)
        }
    }
}

@Composable
private fun SplitView(node: SplitNode, place: Place, modifier: Modifier) {
    val primary = node.primary
    val detail = node.detail
    BoxWithConstraints(modifier.fillMaxWidth()) {
        if (maxWidth >= 840.dp && primary != null && detail != null) {
            Row(horizontalArrangement = Arrangement.spacedBy(24.dp)) {
                Box(Modifier.width(360.dp)) { NodeView(primary, place) }
                Box(Modifier.weight(1f)) { NodeView(detail, place) }
            }
        } else {
            Column(verticalArrangement = Arrangement.spacedBy(16.dp)) {
                primary?.let { NodeView(it, place) }
                detail?.let { NodeView(it, place) }
            }
        }
    }
}

@Composable
private fun TabsView(node: TabsNode, place: Place, modifier: Modifier) {
    val ui = LocalUi.current
    val fromServer = node.items.indexOfFirst { it.selected }.coerceAtLeast(0)
    var chosen by rememberSaveable(node.id, fromServer) { mutableIntStateOf(fromServer) }
    if (node.items.isEmpty()) return
    Column(modifier.fillMaxWidth()) {
        SecondaryTabRow(selectedTabIndex = chosen.coerceIn(0, node.items.lastIndex)) {
            node.items.forEachIndexed { index, item ->
                Tab(
                    selected = index == chosen,
                    onClick = {
                        val href = item.href
                        // A tab with an address is another page of the same screen; one without shows its own children.
                        if (href != null && item.children.isEmpty()) {
                            ui.act(place, Navigate(href, Present.Replace))
                        } else {
                            chosen = index
                        }
                    },
                    text = { Text(item.label, maxLines = 1, overflow = TextOverflow.Ellipsis) },
                )
            }
        }
        val children = node.items.getOrNull(chosen)?.children.orEmpty()
        Column(Modifier.padding(top = 16.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            for (child in children) NodeView(child, place)
        }
    }
}

@Composable
private fun DisclosureView(node: DisclosureNode, place: Place, modifier: Modifier) {
    val ui = LocalUi.current
    val id = node.id
    var own by rememberSaveable(id) { androidx.compose.runtime.mutableStateOf(node.open) }
    val open = if (id != null) place.entry.local.isOpen(node) else own
    val expand = stringResource(R.string.navree_expand)
    val collapse = stringResource(R.string.navree_collapse)
    Column(modifier.fillMaxWidth()) {
        ListItem(
            headlineContent = { Text(node.label) },
            trailingContent = {
                Icon(Glyphs.ExpandMore, contentDescription = null, modifier = Modifier.rotate(if (open) 180f else 0f))
            },
            colors = ListItemDefaults.colors(containerColor = Color.Transparent),
            modifier = Modifier
                .clip(MaterialTheme.shapes.medium)
                .clickable(role = Role.Button, onClickLabel = if (open) collapse else expand) {
                    if (id != null) ui.dispatch(Event.OverlayToggled(place.key, id, !open)) else own = !open
                }
                .semantics { stateDescription = if (open) collapse else expand },
        )
        AnimatedVisibility(visible = open) {
            Column(Modifier.padding(horizontal = 16.dp, vertical = 8.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
                for (child in node.children) NodeView(child, place)
            }
        }
    }
}

@Composable
internal fun AvatarView(node: AvatarNode, modifier: Modifier = Modifier) {
    val initials = node.initials ?: node.name.split(' ').filter { it.isNotBlank() }.take(2).joinToString("") { it.take(1) }.uppercase()
    val src = node.src
    Box(
        modifier = modifier
            .size(40.dp)
            .clip(CircleShape)
            .background(MaterialTheme.colorScheme.primaryContainer)
            // The name is in the row beside it; said twice it is noise.
            .clearAndSetSemantics {},
        contentAlignment = Alignment.Center,
    ) {
        if (src != null) {
            RemoteImage(url = src, description = null, cover = true, modifier = Modifier.size(40.dp))
        } else if (initials.isNotEmpty()) {
            // The circle does not grow with the user's text size, so neither do the letters in it:
            // the name is written out beside it.
            val fixed = with(LocalDensity.current) { 16.dp.toSp() }
            Text(
                initials,
                style = MaterialTheme.typography.titleMedium.copy(fontSize = fixed),
                color = MaterialTheme.colorScheme.onPrimaryContainer,
                maxLines = 1,
            )
        }
    }
}

/** A status label: a word on its tone's container. */
@Composable
internal fun BadgeView(node: BadgeNode, place: Place, modifier: Modifier = Modifier) {
    val text = node.id?.let { place.entry.local.texts[it] } ?: node.text ?: return
    val role = ContentRules.tone(node.tone)
    val colours = toneColours(role)
    Surface(
        modifier = modifier,
        color = colours.container,
        contentColor = if (role == ToneRole.Surface) MaterialTheme.colorScheme.onSurface else colours.content,
        shape = MaterialTheme.shapes.small,
    ) {
        Text(
            text,
            style = MaterialTheme.typography.labelMedium,
            maxLines = 1,
            modifier = Modifier.padding(horizontal = 8.dp, vertical = 4.dp),
        )
    }
}
