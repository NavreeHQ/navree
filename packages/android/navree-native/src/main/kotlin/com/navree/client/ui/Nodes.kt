@file:OptIn(ExperimentalLayoutApi::class)

package com.navree.client.ui

import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.SizeTransform
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.slideInVertically
import androidx.compose.animation.slideOutVertically
import androidx.compose.animation.togetherWith
import androidx.compose.animation.using
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
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.ListItem
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.LocalContentColor
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedCard
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
import androidx.compose.ui.text.LinkAnnotation
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.TextLinkStyles
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.text.withLink
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.dp
import com.navree.client.R
import com.navree.client.core.Events
import com.navree.client.design.ContentRules
import com.navree.client.design.ListRules
import com.navree.client.design.ToneRole
import com.navree.client.design.TypeRole
import com.navree.client.design.expressive.Waiting
import com.navree.client.theme.toneColours
import com.navree.client.design.CompositionRules
import com.navree.protocol.Navree

/** The Material type style a text style is set in. */
@Composable
internal fun typeOf(style: Navree.TextStyle): TextStyle = when (ContentRules.type(style)) {
    TypeRole.DisplayMedium -> MaterialTheme.typography.displayMedium
    TypeRole.HeadlineMedium -> MaterialTheme.typography.headlineMedium
    TypeRole.HeadlineSmall -> MaterialTheme.typography.headlineSmall
    TypeRole.TitleLarge -> MaterialTheme.typography.titleLarge
    TypeRole.TitleMedium -> MaterialTheme.typography.titleMedium
    TypeRole.BodyLarge -> MaterialTheme.typography.bodyLarge
    TypeRole.BodyMedium -> MaterialTheme.typography.bodyMedium
    TypeRole.BodySmall -> MaterialTheme.typography.bodySmall
    TypeRole.Mono -> MaterialTheme.typography.bodyMedium.copy(fontFamily = FontFamily.Monospace)
}

/** The colour text of a tone is set in, on a plain surface. */
@Composable
internal fun textColour(tone: Navree.Tone?, style: Navree.TextStyle, muted: Boolean = false): Color = when (tone) {
    null, Navree.Tone.NEUTRAL -> if (muted || style == Navree.TextStyle.CAPTION) MaterialTheme.colorScheme.onSurfaceVariant else LocalContentColor.current
    Navree.Tone.ACCENT -> MaterialTheme.colorScheme.primary
    Navree.Tone.DANGER -> MaterialTheme.colorScheme.error
    // On a plain surface the container's own content colour is the readable one.
    else -> toneColours(ContentRules.tone(tone)).content.let { colour ->
        if (colour == Color.Unspecified) LocalContentColor.current else colour
    }
}

/** The colour that marks a tone on a plain surface: a status dot, a trend. */
@Composable
internal fun markColour(tone: Navree.Tone?): Color = when (tone) {
    null, Navree.Tone.NEUTRAL -> MaterialTheme.colorScheme.outline
    Navree.Tone.ACCENT -> MaterialTheme.colorScheme.primary
    Navree.Tone.DANGER -> MaterialTheme.colorScheme.error
    else -> textColour(tone, Navree.TextStyle.BODY)
}

/**
 * Words in the type scale: the page's style, its weight, its hue (over its
 * tone), its alignment, figures of one width where the page asks, and a
 * count that rolls when an op changes it.
 */
@Composable
private fun TextView(node: Navree.Text, modifier: Modifier) {
    val base = typeOf(node.style)
    val style = base
        .let { s -> node.weight?.let { s.copy(fontWeight = fontWeight(it)) } ?: s }
        .let { s -> if (node.tabular) s.copy(fontFeatureSettings = "tnum") else s }
    val hue = node.colour?.let { inkOf(it, Navree.Hue.ACCENT).mark }
    val colour = hue ?: textColour(node.tone, node.style, node.muted)
    val align = when (node.align) {
        Navree.Align.CENTER -> TextAlign.Center
        Navree.Align.END -> TextAlign.End
        Navree.Align.START -> TextAlign.Start
        else -> null
    }
    val heading = node.heading != null || node.style == Navree.TextStyle.LARGE_TITLE || node.style == Navree.TextStyle.TITLE || node.style == Navree.TextStyle.DISPLAY
    val shaped = (if (align != null) modifier.fillMaxWidth() else modifier).then(if (heading) Modifier.semantics { heading() } else Modifier)
    val draw: @Composable (String) -> Unit = { words ->
        Text(
            text = words,
            modifier = shaped,
            style = style,
            color = colour,
            textAlign = align,
            maxLines = node.lines ?: Int.MAX_VALUE,
            overflow = TextOverflow.Ellipsis,
        )
    }
    when (node.change) {
        null -> draw(node.value)
        // A number that changes rolls to its new value, as the animation guide's counter does.
        Navree.Change.COUNT -> AnimatedContent(
            targetState = node.value,
            transitionSpec = { (slideInVertically { it } + fadeIn()) togetherWith (slideOutVertically { -it } + fadeOut()) using SizeTransform(clip = false) },
            label = "count",
        ) { draw(it) }
        Navree.Change.FADE -> AnimatedContent(targetState = node.value, transitionSpec = { fadeIn() togetherWith fadeOut() }, label = "fade") { draw(it) }
    }
}

private fun fontWeight(weight: Navree.Weight): FontWeight = when (weight) {
    Navree.Weight.REGULAR -> FontWeight.Normal
    Navree.Weight.MEDIUM -> FontWeight.Medium
    Navree.Weight.SEMIBOLD -> FontWeight.SemiBold
    Navree.Weight.BOLD -> FontWeight.Bold
}

/** Draws any node. What has its own place on the screen's frame (sheets, dialogs, toolbars) draws nothing here. */
@Composable
internal fun NodeView(node: Navree.Node, place: Place, modifier: Modifier = Modifier) {
    if (node === place.omit || !inFlow(node, place.screen)) return
    when (node) {
        is Navree.Text -> TextView(node, modifier)
        is Navree.Rich -> RichView(node, place, modifier)
        is Navree.Image -> {
            val width = node.width
            val height = node.height
            val shaped = if (width != null && height != null && height > 0) modifier.aspectRatio(width.toFloat() / height) else modifier
            RemoteImage(
                url = node.src,
                description = node.alt.ifEmpty { null },
                cover = node.fit != Navree.Fit.CONTAIN,
                modifier = shaped.clip(MaterialTheme.shapes.medium),
            )
        }
        is Navree.Icon -> IconView(node, modifier)
        is Navree.Avatar -> AvatarView(node, modifier)
        is Navree.Badge -> StatusView(node, modifier)
        is Navree.Progress -> ProgressView(node, modifier)
        is Navree.Banner -> BannerView(node, place, modifier)
        is Navree.Empty -> EmptyView(node, place, modifier)
        is Navree.Facts -> FactsView(node, place, modifier)
        is Navree.Table -> TableView(node, place, modifier)
        is Navree.Code -> CodeView(node, modifier)
        is Navree.Button -> ButtonView(node, place, modifier)
        is Navree.Link -> LinkView(node, place, modifier)
        is Navree.Menu -> MenuButtonView(node, place, modifier)
        is Navree.Form -> FormView(node, place, modifier)
        is Navree.Field -> FieldView(node, place, modifier)
        is Navree.Section -> SectionView(node, place, modifier)
        is Navree.Grid -> GridView(node, place, modifier)
        is Navree.Stack -> StackView(node, place, modifier)
        is Navree.Region -> RegionView(node, place, modifier)
        is Navree.List -> if (node.reorder != null) ReorderList(node, place, modifier) else Column(modifier) {
            for (child in node.children) {
                when (child) {
                    is Navree.Row -> RowView(child, place, Group.None)
                    is Navree.FeedRow -> FeedRowView(child, place)
                    else -> NodeView(child, place)
                }
            }
        }
        is Navree.Row -> RowView(node, place, Group.None, modifier)
        is Navree.Split -> SplitView(node, place, modifier)
        is Navree.Pane -> PaneView(node, place, modifier)
        is Navree.Tabs -> TabsView(node, place, modifier)
        is Navree.Disclosure -> DisclosureView(node, place, modifier)
        is Navree.Divider -> HorizontalDivider(modifier.padding(vertical = 8.dp))
        is Navree.Spacer -> Spacer(modifier.height(16.dp))
        is Navree.Gauge -> HealthGaugeView(node, modifier)
        is Navree.Figure -> HealthFigureView(node, place, modifier)
        is Navree.Ring -> RingView(node, modifier)
        is Navree.Rings -> RingsView(node, modifier)
        is Navree.DayClock -> DayClockView(node, modifier)
        is Navree.DayPager -> DayPagerView(node, place, modifier)
        is Navree.Meter -> MeterView(node, modifier)
        is Navree.MiniColumns -> MiniColumnsView(node, modifier)
        is Navree.ColorStrip -> StripView(node, modifier)
        is Navree.RangeScale -> RangeView(node, modifier)
        is Navree.CalendarHeatmap -> CalendarHeatmapView(node, modifier)
        is Navree.MatrixHeatmap -> MatrixHeatmapView(node, modifier)
        is Navree.Scatter -> ScatterView(node, modifier)
        is Navree.Card -> CardView(node, place, modifier)
        is Navree.Layers -> LayersView(node, place, modifier)
        is Navree.Shape -> ShapeView(node, modifier)
        is Navree.Carousel -> CarouselView(node, place, modifier)
        is Navree.Adaptive -> AdaptiveView(node, place, modifier)
        is Navree.Timeline -> TimelineView(node, place, modifier)
        is Navree.FeedRow -> FeedRowView(node, place, modifier)
        is Navree.Message -> MessageView(node, place, modifier)
        is Navree.NativeView -> node.alt?.let { NodeView(it, place, modifier) }
        is Navree.Node.Unknown -> node.alt?.let { NodeView(it, place, modifier) }
        is Navree.Chart -> ChartView(node, modifier)
        // A screen that holds one of these is shown in the web view, so none reaches here.
        is Navree.WebRegion -> Unit
        // Drawn over the screen, not in it.
        is Navree.Sheet, is Navree.Dialog, is Navree.Popover, is Navree.Toolbar -> Unit
        else -> Unit
    }
}

@Composable
private fun RegionView(node: Navree.Region, place: Place, modifier: Modifier) {
    val ui = LocalUi.current
    val state = place.screen.regions[node.id]
    val shown = node.children.filter { inFlow(it, place.screen) }
    Column(modifier, verticalArrangement = Arrangement.spacedBy(12.dp)) {
        when {
            shown.isEmpty() && (node.lazy || state?.loading == true) && state?.failure == null ->
                Box(Modifier.fillMaxWidth().padding(24.dp), contentAlignment = Alignment.Center) { Waiting(Modifier.size(40.dp)) }
            else -> for (child in shown) NodeView(child, place)
        }
        val failure = state?.failure
        if (failure != null) {
            RetryLine(failureTitle(failure)) { ui.send(Events.regionRetry(place.id, node.id)) }
        }
    }
}

/** What a part that could not load says, with a way to try again. */
@Composable
internal fun RetryLine(text: String, retry: () -> Unit) {
    ListItem(
        headlineContent = { Text(text) },
        trailingContent = {
            androidx.compose.material3.TextButton(onClick = retry) { Text(stringResource(R.string.navree_retry)) }
        },
        colors = ListItemDefaults.colors(containerColor = Color.Transparent),
    )
}

@Composable
private fun RichView(node: Navree.Rich, place: Place, modifier: Modifier) {
    val ui = LocalUi.current
    val link = MaterialTheme.colorScheme.primary
    val code = MaterialTheme.colorScheme.surfaceContainerHighest
    fun runs(builder: androidx.compose.ui.text.AnnotatedString.Builder, list: List<Navree.Run>) {
        for (run in list) {
            val style = SpanStyle(
                fontWeight = if (run.bold) FontWeight.Bold else null,
                fontStyle = if (run.italic) FontStyle.Italic else null,
                fontFamily = if (run.code) FontFamily.Monospace else null,
                background = if (run.code) code else Color.Unspecified,
            )
            val href = run.href
            if (href != null) {
                val styles = TextLinkStyles(style.copy(color = link, textDecoration = TextDecoration.Underline))
                builder.withLink(LinkAnnotation.Clickable(tag = href, styles = styles) { ui.act(place, Navree.Action.Navigate(href)) }) {
                    append(run.text)
                }
            } else {
                builder.withStyle(style) { append(run.text) }
            }
        }
    }
    val colour = if (node.muted) MaterialTheme.colorScheme.onSurfaceVariant else LocalContentColor.current
    if (node.blocks.isEmpty()) {
        val text = remember(node, link, code) { buildAnnotatedString { runs(this, node.runs) } }
        Text(text = text, modifier = modifier, style = typeOf(node.style), color = colour)
        return
    }
    // Blocks: paragraphs, headings, quotes, lists and code, one under the other.
    Column(modifier, verticalArrangement = Arrangement.spacedBy(8.dp)) {
        var number = 0
        for (block in node.blocks) {
            val text = buildAnnotatedString { runs(this, block.runs) }
            when (block.kind) {
                Navree.BlockKind.HEADING -> Text(
                    text,
                    style = if (block.level <= 2) MaterialTheme.typography.titleLarge else MaterialTheme.typography.titleMedium,
                    modifier = Modifier.semantics { heading() },
                )
                Navree.BlockKind.QUOTE -> Row(Modifier.height(IntrinsicSize.Min)) {
                    Box(Modifier.width(4.dp).fillMaxHeight().background(MaterialTheme.colorScheme.outlineVariant))
                    Text(text, style = typeOf(node.style), color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.padding(start = 12.dp))
                }
                Navree.BlockKind.BULLET -> Row {
                    Text("•", style = typeOf(node.style), modifier = Modifier.width(20.dp))
                    Text(text, style = typeOf(node.style), color = colour)
                }
                Navree.BlockKind.NUMBERED -> Row {
                    number += 1
                    Text("$number.", style = typeOf(node.style), modifier = Modifier.width(28.dp))
                    Text(text, style = typeOf(node.style), color = colour)
                }
                Navree.BlockKind.CODE -> CodeView(Navree.Code(text = text.text), Modifier)
                Navree.BlockKind.PARAGRAPH -> Text(text, style = typeOf(node.style), color = colour)
            }
            if (block.kind != Navree.BlockKind.NUMBERED) number = 0
        }
    }
}

@Composable
internal fun IconView(node: Navree.Icon, modifier: Modifier = Modifier, tint: Color = LocalContentColor.current) {
    val vector = rememberIcon(node) ?: return
    // As large as the text style the page names; in the page's hue when it gives one.
    val sized = node.scale?.let { modifier.size(CompositionRules.iconDp(it).dp) } ?: modifier
    val colour = node.colour?.let { inkOf(it, Navree.Hue.ACCENT).mark } ?: tint
    Icon(imageVector = vector, contentDescription = node.label, modifier = sized, tint = colour)
}

@Composable
private fun ProgressView(node: Navree.Progress, modifier: Modifier) {
    val described = modifier.fillMaxWidth().semantics { node.label?.let { contentDescription = it } }
    val value = node.value
    if (value != null) {
        LinearProgressIndicator(progress = { (value / 1000f).coerceIn(0f, 1f) }, modifier = described)
    } else {
        LinearProgressIndicator(modifier = described)
    }
}

@Composable
private fun BannerView(node: Navree.Banner, place: Place, modifier: Modifier) {
    val colours = toneColours(ContentRules.tone(node.tone))
    val urgent = node.tone == Navree.Tone.DANGER || node.tone == Navree.Tone.WARNING
    Card(
        modifier = modifier.fillMaxWidth().semantics {
            liveRegion = if (urgent) LiveRegionMode.Assertive else LiveRegionMode.Polite
        },
        colors = CardDefaults.cardColors(containerColor = colours.container, contentColor = colours.content),
    ) {
        Row(Modifier.padding(start = 16.dp, top = 16.dp, end = 16.dp, bottom = if (node.actions.isEmpty()) 16.dp else 4.dp)) {
            node.icon?.let { icon -> IconView(icon, Modifier.padding(end = 16.dp).size(24.dp), tint = colours.content) }
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
                    if (action is Navree.Button) QuietButton(action, place, colours.content)
                }
            }
        }
    }
}

@Composable
internal fun EmptyView(node: Navree.Empty, place: Place, modifier: Modifier = Modifier) {
    Column(
        modifier = modifier.fillMaxWidth().padding(horizontal = 32.dp, vertical = 32.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        node.icon?.let { icon ->
            IconView(icon, Modifier.size(48.dp), tint = MaterialTheme.colorScheme.onSurfaceVariant)
            Spacer(Modifier.height(16.dp))
        }
        Text(node.title, style = MaterialTheme.typography.titleLarge, textAlign = TextAlign.Center, modifier = Modifier.semantics { heading() })
        node.text?.let {
            Text(
                it,
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                textAlign = TextAlign.Center,
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
 * In a wide window they stand in two columns.
 */
@Composable
private fun FactsView(node: Navree.Facts, place: Place, modifier: Modifier) {
    val window = LocalUi.current.window
    val columns = if (window.width >= com.navree.client.design.WidthClass.Expanded && !window.largeText) 2 else 1
    Column(modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(16.dp)) {
        for (line in node.items.chunked(columns)) {
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(24.dp)) {
                for (fact in line) {
                    Row(Modifier.weight(1f).semantics(mergeDescendants = true) {}, horizontalArrangement = Arrangement.spacedBy(16.dp)) {
                        val icon = fact.icon
                        if (icon != null) {
                            val colours = toneColours(ContentRules.tone(fact.tone))
                            Box(Modifier.size(40.dp).clip(CircleShape).background(colours.container), contentAlignment = Alignment.Center) {
                                IconView(
                                    icon,
                                    Modifier.size(20.dp),
                                    tint = if (fact.tone == null || fact.tone == Navree.Tone.NEUTRAL) MaterialTheme.colorScheme.onSurfaceVariant else colours.content,
                                )
                            }
                        }
                        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                            Text(fact.label, style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                            for (value in fact.value) NodeView(value, place)
                            for (detail in fact.details) {
                                Text(detail, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                            }
                        }
                    }
                }
                repeat(columns - line.size) { Spacer(Modifier.weight(1f)) }
            }
        }
    }
}

@Composable
internal fun CodeView(node: Navree.Code, modifier: Modifier) {
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
private fun SectionView(node: Navree.Section, place: Place, modifier: Modifier) {
    val content: @Composable () -> Unit = {
        Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
            val title = node.title
            if (title != null || node.actions.isNotEmpty() || node.hint != null) {
                Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                    Column(Modifier.weight(1f)) {
                        title?.let { Text(it, style = MaterialTheme.typography.titleMedium, modifier = Modifier.semantics { heading() }) }
                        node.subtitle?.let {
                            Text(it, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                        }
                    }
                    node.hint?.let { HintMark(it) }
                    for (action in node.actions) NodeView(action, place)
                }
            }
            for (child in node.children) NodeView(child, place)
            for (child in node.footer) NodeView(child, place)
        }
    }
    if (node.style == Navree.SectionStyle.CARD) {
        // One card variant on every screen: Material's filled card, a tonal step above the surface.
        Card(
            modifier.fillMaxWidth(),
            colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceContainerLow),
        ) { Box(Modifier.padding(16.dp)) { content() } }
    } else {
        Box(modifier.fillMaxWidth()) { content() }
    }
}

@Composable
private fun GridView(node: Navree.Grid, place: Place, modifier: Modifier) {
    val gap = ContentRules.gap(node.gap)
    BoxWithConstraints(modifier.fillMaxWidth()) {
        val columns = ListRules.gridColumns(maxWidth.value.toInt(), node.min, gap, node.cols, LocalUi.current.window.fontScale)
        val shown = node.children.filter { inFlow(it, place.screen) }
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
 * A stack with a wide axis turns in a wide room, its parts keeping their
 * state; a part that grows takes the room left; `pad` is steps all round.
 */
@Composable
private fun StackView(node: Navree.Stack, place: Place, modifier: Modifier) {
    val shown = node.children.filter { it !== place.omit && inFlow(it, place.screen) }
    if (shown.isEmpty()) return
    val padded = if (node.pad > 0) modifier.padding((node.pad * CompositionRules.STEP_DP).dp) else modifier
    if (node.wide == null || node.wide == node.axis) {
        StackLaid(node, node.axis, shown, place, padded)
        return
    }
    BoxWithConstraints(padded) {
        val axis = CompositionRules.axis(node, maxWidth.value)
        StackLaid(node, axis, shown, place, Modifier)
    }
}

@Composable
private fun StackLaid(node: Navree.Stack, axis: Navree.Axis, shown: List<Navree.Node>, place: Place, modifier: Modifier) {
    val gap = ContentRules.gap(node.gap).dp
    if (axis == Navree.Axis.HORIZONTAL) {
        val across = when (node.justify) {
            Navree.Justify.BETWEEN -> if (shown.size > 1) Arrangement.SpaceBetween else Arrangement.Start
            Navree.Justify.CENTER -> Arrangement.spacedBy(gap, Alignment.CenterHorizontally)
            Navree.Justify.END -> Arrangement.spacedBy(gap, Alignment.End)
            else -> Arrangement.spacedBy(gap)
        }
        val upright = when (node.align) {
            Navree.Align.START -> Alignment.Top
            Navree.Align.END -> Alignment.Bottom
            else -> Alignment.CenterVertically
        }
        val growing = shown.any(CompositionRules::grows)
        val wide = node.justify == Navree.Justify.BETWEEN || node.justify == Navree.Justify.END || growing
        if (node.wrap) {
            FlowRow(
                modifier = if (wide) modifier.fillMaxWidth() else modifier,
                horizontalArrangement = across,
                verticalArrangement = Arrangement.spacedBy(gap),
                itemVerticalAlignment = upright,
            ) {
                for (child in shown) NodeView(child, place)
            }
        } else {
            Row(modifier = if (wide) modifier.fillMaxWidth() else modifier, horizontalArrangement = across, verticalAlignment = upright) {
                for (child in shown) NodeView(child, place, if (CompositionRules.grows(child)) Modifier.weight(1f) else Modifier)
            }
        }
    } else {
        Column(
            modifier = modifier,
            verticalArrangement = Arrangement.spacedBy(gap),
            horizontalAlignment = when (node.align) {
                Navree.Align.CENTER -> Alignment.CenterHorizontally
                Navree.Align.END -> Alignment.End
                else -> Alignment.Start
            },
        ) {
            for (child in shown) NodeView(child, place)
        }
    }
}

/**
 * Two panes. In a wide window they stand side by side, the detail in a
 * rounded container of its own on the surface tone; otherwise the pane the
 * server says is in hand comes first, the other under it.
 */
@Composable
private fun SplitView(node: Navree.Split, place: Place, modifier: Modifier) {
    BoxWithConstraints(modifier.fillMaxWidth()) {
        if (maxWidth >= 840.dp) {
            Row(horizontalArrangement = Arrangement.spacedBy(24.dp)) {
                Box(Modifier.width(360.dp)) { NodeView(node.primary, place) }
                Surface(
                    color = MaterialTheme.colorScheme.surfaceContainerLow,
                    shape = MaterialTheme.shapes.extraLarge,
                    modifier = Modifier.weight(1f),
                ) {
                    Box(Modifier.padding(24.dp)) { NodeView(node.detail, place) }
                }
            }
        } else {
            val detailFirst = (node.detail as? Navree.Pane)?.current == true
            Column(verticalArrangement = Arrangement.spacedBy(16.dp)) {
                NodeView(if (detailFirst) node.detail else node.primary, place)
                NodeView(if (detailFirst) node.primary else node.detail, place)
            }
        }
    }
}

@Composable
private fun PaneView(node: Navree.Pane, place: Place, modifier: Modifier) {
    Column(modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(12.dp)) {
        node.title?.let { Text(it, style = MaterialTheme.typography.titleMedium, modifier = Modifier.semantics { heading() }) }
        for (child in node.children) NodeView(child, place)
    }
}

@Composable
private fun TabsView(node: Navree.Tabs, place: Place, modifier: Modifier) {
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
                            ui.act(place, Navree.Action.Navigate(href, Navree.Present.REPLACE))
                        } else {
                            chosen = index
                            // Tabs with an id are the core's to keep: it fetches what the tab shows, and the tree says which is chosen.
                            node.id?.let { ui.send(Events.tab(place.id, it, index)) }
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
private fun DisclosureView(node: Navree.Disclosure, place: Place, modifier: Modifier) {
    val ui = LocalUi.current
    val id = node.id
    var own by rememberSaveable(id) { androidx.compose.runtime.mutableStateOf(node.open) }
    // The core keeps the state of a disclosure with an id; one without is the client's to hold.
    val open = if (id != null) node.open || place.screen.open(id) else own
    val expand = stringResource(R.string.navree_expand)
    val collapse = stringResource(R.string.navree_collapse)
    Column(modifier.fillMaxWidth()) {
        ListItem(
            headlineContent = { Text(node.label) },
            trailingContent = { Icon(Glyphs.ExpandMore, contentDescription = null, modifier = Modifier.rotate(if (open) 180f else 0f)) },
            colors = ListItemDefaults.colors(containerColor = Color.Transparent),
            modifier = Modifier
                .clip(MaterialTheme.shapes.medium)
                .clickable(role = Role.Button, onClickLabel = if (open) collapse else expand) {
                    if (id != null) ui.send(Events.overlay(place.id, id, !open)) else own = !open
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
internal fun AvatarView(node: Navree.Avatar, modifier: Modifier = Modifier, size: Int = 40) {
    val initials = node.initials ?: node.name.split(' ').filter { it.isNotBlank() }.take(2).joinToString("") { it.take(1) }.uppercase()
    val src = node.src
    Box(
        modifier = modifier
            .size(size.dp)
            .clip(CircleShape)
            .background(MaterialTheme.colorScheme.primaryContainer)
            // The name is in the row beside it; said twice it is noise.
            .clearAndSetSemantics {},
        contentAlignment = Alignment.Center,
    ) {
        if (src != null) {
            RemoteImage(url = src, description = null, cover = true, modifier = Modifier.size(size.dp))
        } else if (initials.isNotEmpty()) {
            // The circle does not grow with the user's text size, so neither do the letters in it:
            // the name is written out beside it.
            val fixed = with(LocalDensity.current) { (size * 0.4f).dp.toSp() }
            Text(initials, style = MaterialTheme.typography.titleMedium.copy(fontSize = fixed, lineHeight = fixed), color = MaterialTheme.colorScheme.onPrimaryContainer, maxLines = 1)
        }
    }
}

/**
 * A status. As plain as the lines around it: a small dot in its tone's colour
 * and the word, at the size of supporting text. Google's own lists say a
 * status this way; a filled label would shout louder than the amount beside it.
 */
@Composable
internal fun StatusView(node: Navree.Badge, modifier: Modifier = Modifier) {
    Row(modifier, verticalAlignment = Alignment.CenterVertically) {
        Box(Modifier.size(8.dp).clip(CircleShape).background(markColour(node.tone)))
        Text(
            node.text,
            style = MaterialTheme.typography.bodyMedium,
            color = if (node.tone == Navree.Tone.DANGER) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.onSurfaceVariant,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
            modifier = Modifier.padding(start = 6.dp),
        )
    }
}

/** A timeline: moments one under the other, joined by a line, each with its mark, time and what happened. */
@Composable
private fun TimelineView(node: Navree.Timeline, place: Place, modifier: Modifier) {
    val line = MaterialTheme.colorScheme.outlineVariant
    Column(modifier.fillMaxWidth()) {
        node.items.forEachIndexed { index, moment ->
            Row(Modifier.fillMaxWidth().height(IntrinsicSize.Min).semantics(mergeDescendants = true) {}) {
                Column(Modifier.width(40.dp).fillMaxHeight(), horizontalAlignment = Alignment.CenterHorizontally) {
                    val icon = moment.icon
                    val colour = markColour(moment.tone)
                    if (icon != null) {
                        Box(Modifier.size(28.dp).clip(CircleShape).background(MaterialTheme.colorScheme.surfaceContainerHigh), contentAlignment = Alignment.Center) {
                            IconView(icon, Modifier.size(16.dp), tint = colour)
                        }
                    } else {
                        Box(Modifier.padding(top = 6.dp).size(12.dp).clip(CircleShape).background(if (moment.state == Navree.RowState.UPCOMING) line else colour))
                    }
                    if (index < node.items.lastIndex) Box(Modifier.width(2.dp).weight(1f).background(line))
                }
                Column(Modifier.weight(1f).padding(start = 8.dp, bottom = 16.dp), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                    Text(moment.title, style = MaterialTheme.typography.bodyLarge)
                    moment.subtitle?.let { Text(it, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant) }
                    moment.time?.let { Text(it, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant) }
                    for (part in moment.detail) NodeView(part, place)
                }
            }
        }
    }
}

/** A feed row: who did what, as a list item with the actor's avatar, the words with their links, and when. */
@Composable
internal fun FeedRowView(node: Navree.FeedRow, place: Place, modifier: Modifier = Modifier) {
    val ui = LocalUi.current
    val action = node.action
    ListItem(
        headlineContent = {
            NodeView(Navree.Rich(runs = node.runs), place)
        },
        supportingContent = if (node.time != null || node.detail.isNotEmpty()) {
            {
                Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
                    node.time?.let { Text(it) }
                    for (part in node.detail) NodeView(part, place)
                }
            }
        } else {
            null
        },
        leadingContent = when {
            node.actor != null -> {
                { AvatarView(node.actor!!) }
            }
            node.icon != null -> {
                { IconView(node.icon!!, Modifier.size(24.dp), tint = markColour(node.tone)) }
            }
            else -> null
        },
        colors = ListItemDefaults.colors(containerColor = Color.Transparent),
        modifier = modifier.then(if (action != null) Modifier.clickable(role = Role.Button) { ui.act(place, action) } else Modifier),
    )
}

/**
 * A message in a conversation: the person's own on the end side in the
 * primary container, others' on the start side with their avatar; a note
 * across the middle. The time and whether it arrived sit under the bubble.
 */
@Composable
private fun MessageView(node: Navree.Message, place: Place, modifier: Modifier) {
    val scheme = MaterialTheme.colorScheme
    if (node.note) {
        Column(modifier.fillMaxWidth(), horizontalAlignment = Alignment.CenterHorizontally) {
            Surface(color = scheme.surfaceContainerHigh, shape = MaterialTheme.shapes.medium) {
                Column(Modifier.padding(horizontal = 12.dp, vertical = 8.dp)) { for (part in node.body) NodeView(part, place) }
            }
        }
        return
    }
    val mine = node.mine
    Row(
        modifier.fillMaxWidth().semantics(mergeDescendants = true) {},
        horizontalArrangement = if (mine) Arrangement.End else Arrangement.Start,
        verticalAlignment = Alignment.Bottom,
    ) {
        if (!mine) {
            AvatarView(node.avatar ?: Navree.Avatar(name = node.author), size = 32)
            Spacer(Modifier.width(8.dp))
        }
        Column(horizontalAlignment = if (mine) Alignment.End else Alignment.Start, modifier = Modifier.widthIn(max = 480.dp)) {
            if (!mine) Text(node.author, style = MaterialTheme.typography.labelMedium, color = scheme.onSurfaceVariant)
            Surface(
                color = if (mine) scheme.primaryContainer else scheme.surfaceContainerHigh,
                contentColor = if (mine) scheme.onPrimaryContainer else scheme.onSurface,
                shape = RoundedCornerShape(20.dp),
            ) {
                Column(Modifier.padding(horizontal = 16.dp, vertical = 10.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                    for (part in node.body) NodeView(part, place)
                }
            }
            val under = listOfNotNull(node.time, node.delivery?.let { deliveryWord(it) }).joinToString(" · ")
            if (under.isNotEmpty()) {
                Text(
                    under,
                    style = MaterialTheme.typography.labelSmall,
                    color = if (node.delivery == Navree.Delivery.FAILED) scheme.error else scheme.onSurfaceVariant,
                    modifier = Modifier.padding(top = 2.dp, start = 4.dp, end = 4.dp),
                )
            }
        }
    }
}

@Composable
private fun deliveryWord(delivery: Navree.Delivery): String = stringResource(
    when (delivery) {
        Navree.Delivery.SENDING -> R.string.navree_sending
        Navree.Delivery.SENT -> R.string.navree_sent
        Navree.Delivery.DELIVERED -> R.string.navree_delivered
        Navree.Delivery.READ -> R.string.navree_read
        Navree.Delivery.FAILED -> R.string.navree_not_sent
    },
)

/** The colours a tone's container is drawn with, for code that has a tone role in hand. */
@Composable
internal fun toneRoleColours(role: ToneRole) = toneColours(role)
