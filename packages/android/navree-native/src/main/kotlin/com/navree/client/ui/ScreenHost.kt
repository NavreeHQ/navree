package com.navree.client.ui

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.IntrinsicSize
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.consumeWindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyItemScope
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExtendedFloatingActionButton
import androidx.compose.material3.FilterChip
import androidx.compose.material3.FloatingActionButton
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SearchBar
import androidx.compose.material3.SearchBarDefaults
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.material3.TopAppBarScrollBehavior
import androidx.compose.material3.pulltorefresh.PullToRefreshBox
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.nestedscroll.nestedScroll
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.navree.client.R
import com.navree.client.core.Content
import com.navree.client.core.Events
import com.navree.client.core.Failure
import com.navree.client.core.FieldValue
import com.navree.client.core.ScreenView
import com.navree.client.design.BarActionStyle
import com.navree.client.design.BarKind
import com.navree.client.design.BarScroll
import com.navree.client.design.ContentRules
import com.navree.client.design.FabPlace
import com.navree.client.design.FormRules
import com.navree.client.design.NavigationIcon
import com.navree.client.design.Presentation
import com.navree.client.design.ScreenContext
import com.navree.client.design.ScreenPlan
import com.navree.client.design.ScreenRules
import com.navree.client.design.PagerRules
import com.navree.client.design.SelectionRules
import com.navree.client.design.nodeId
import com.navree.client.design.walk
import com.navree.client.design.SearchPlace
import com.navree.client.design.SubmitPlace
import com.navree.client.design.expressive.FloatingActions
import com.navree.client.design.expressive.Waiting
import com.navree.client.design.FilterRules
import com.navree.client.design.fields
import com.navree.client.design.submitButton
import com.navree.protocol.Navree

/** Opens the full list of destinations over the content. Set by the frame that owns the navigation. */
internal val LocalOpenMenu = staticCompositionLocalOf<() -> Unit> { {} }

/** What a screen is drawn with, beyond its own view state. */
internal data class HostInfo(
    val context: ScreenContext,
    val user: Navree.User?,
    val name: String,
    /** This is the screen the person is looking at: it shows the passing messages. */
    val front: Boolean,
    val selectedUrl: String? = null,
)

/** One screen: its bar, its content and what floats over it, placed by the design rules. */
@Composable
internal fun ScreenHost(screen: ScreenView, info: HostInfo) {
    when (val content = screen.content) {
        Content.Blank -> BlankScreen(screen, info)
        is Content.Web -> WebScreen(screen, content, info)
        is Content.Native -> NativeScreen(screen, content.screen, info)
    }
}

@Composable
private fun NavigationIconView(icon: NavigationIcon, screen: ScreenView, back: Navree.Back? = null) {
    val ui = LocalUi.current
    val openMenu = LocalOpenMenu.current
    when (icon) {
        NavigationIcon.None -> Unit
        NavigationIcon.Back -> TipIconButton(
            stringResource(R.string.navree_back),
            Glyphs.Back,
            onClick = {
                if (ui.driver.nav.canGoBack || back == null) {
                    ui.send(Events.back)
                } else {
                    // Opened directly, with nothing under it: "up" is where the page says it is.
                    ui.send(Events.act(screen.id, Navree.Action.Navigate(back.href, Navree.Present.REPLACE)))
                }
            },
        )
        NavigationIcon.Close -> TipIconButton(stringResource(R.string.navree_close), Glyphs.Close, onClick = { ui.send(Events.back) })
        NavigationIcon.Menu -> TipIconButton(stringResource(R.string.navree_menu), Glyphs.Menu, onClick = openMenu)
    }
}

/** The icon a screen with nothing drawn yet still needs: a way out. */
private fun wayOut(info: HostInfo): NavigationIcon = when {
    !info.context.chrome -> NavigationIcon.None
    info.context.canGoBack -> NavigationIcon.Back
    info.context.presentation != Presentation.Stack -> NavigationIcon.Close
    else -> NavigationIcon.None
}

/** The passing messages, above the keyboard when it is up and above the navigation bar. */
@Composable
private fun Snackbars(info: HostInfo) {
    if (info.front) SnackbarHost(LocalUi.current.snackbars, Modifier.imePadding())
}

@Composable
private fun BlankScreen(screen: ScreenView, info: HostInfo) {
    val ui = LocalUi.current
    Scaffold(
        topBar = {
            // A screen brought back after the system ended the app has its title before its page.
            if (info.context.chrome) {
                TopAppBar(
                    title = { screen.title?.let { Text(it, maxLines = 1, overflow = TextOverflow.Ellipsis) } },
                    navigationIcon = { NavigationIconView(wayOut(info), screen) },
                )
            }
        },
        snackbarHost = { Snackbars(info) },
    ) { padding ->
        Box(Modifier.fillMaxSize().padding(padding), contentAlignment = Alignment.Center) {
            val failure = screen.failure
            if (failure != null && !screen.loading) {
                FailureState(failure) { ui.send(Events.refresh(screen.id)) }
            } else {
                PatientProgress()
            }
        }
    }
}

/** A progress indicator that waits a moment before it shows, so a quick load does not flash one. */
@Composable
internal fun PatientProgress() {
    var show by remember { mutableStateOf(false) }
    LaunchedEffect(Unit) {
        kotlinx.coroutines.delay(350)
        show = true
    }
    val label = stringResource(R.string.navree_loading)
    if (show) Waiting(Modifier.size(48.dp).semantics { contentDescription = label })
}

@Composable
internal fun FailureState(failure: Failure, retry: () -> Unit) {
    Column(Modifier.padding(32.dp), horizontalAlignment = Alignment.CenterHorizontally) {
        if (failure == Failure.Offline) {
            Icon(Glyphs.CloudOff, contentDescription = null, modifier = Modifier.size(48.dp), tint = MaterialTheme.colorScheme.onSurfaceVariant)
            Spacer(Modifier.height(16.dp))
        }
        Text(failureTitle(failure), style = MaterialTheme.typography.titleLarge, textAlign = TextAlign.Center, modifier = Modifier.semantics { heading() })
        Text(
            failureText(failure),
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            textAlign = TextAlign.Center,
            modifier = Modifier.padding(top = 8.dp, bottom = 24.dp),
        )
        Button(onClick = retry) { Text(stringResource(R.string.navree_retry)) }
    }
}

@Composable
private fun WebScreen(screen: ScreenView, content: Content.Web, info: HostInfo) {
    val ui = LocalUi.current
    var title by rememberSaveable(screen.id) { mutableStateOf(content.title ?: "") }
    Scaffold(
        topBar = {
            if (info.context.chrome) {
                TopAppBar(
                    title = { Text(title.ifEmpty { info.name }, maxLines = 1, overflow = TextOverflow.Ellipsis) },
                    navigationIcon = { NavigationIconView(wayOut(info), screen) },
                )
            }
        },
        snackbarHost = { Snackbars(info) },
    ) { padding ->
        WebPage(
            url = content.url,
            serial = content.serial,
            onLink = { ui.send(Events.webLink(screen.id, it)) },
            onTitle = { title = it },
            modifier = Modifier.padding(padding).consumeWindowInsets(padding).imePadding(),
        )
    }
}

@Composable
private fun NativeScreen(view: ScreenView, screen: Navree.Screen, info: HostInfo) {
    val ui = LocalUi.current
    val window = ui.window
    val (arranged, bars) = remember(screen, view.states, info.context) { arrange(screen, view, info.context) }
    val context = info.context.copy(toolbar = bars.isNotEmpty())
    val plan = remember(arranged, context, window) { ScreenRules.plan(arranged, context, window) }
    val place = Place(view, selectedUrl = info.selectedUrl)
    val list = rememberLazyListState()
    var searching by rememberSaveable { mutableStateOf(false) }
    val pinned = TopAppBarDefaults.pinnedScrollBehavior()
    val leaving = TopAppBarDefaults.enterAlwaysScrollBehavior()
    val behaviour = if (plan.scroll == BarScroll.EnterAlways) leaving else pinned
    // What the app bar holds is not drawn a second time in the content.
    val form = screen.body as? Navree.Form
    val barSubmit = if (plan.submit == SubmitPlace.Bar) form?.submitButton() else null
    val query = remember(screen) { ScreenRules.query(screen) }
    val hoisted = if (plan.searchInQuery) query?.form else null
    val atTop by remember { derivedStateOf { list.firstVisibleItemIndex == 0 && list.firstVisibleItemScrollOffset < 24 } }
    val fab = plan.fab?.let { arranged.actions.getOrNull(it) as? Navree.Button }
    // A glance the page opened stands beside the content on a wide window.
    val side = if (ScreenRules.sheet(task = false, window = window) == Presentation.SideSheet) openGlance(place) else null

    // What the screen showed at its first drawing comes in without motion; a part an op adds later may.
    val shown = remember(view.id) { Shown().apply { screen.body.walk().mapNotNull { it.nodeId }.forEach { ids.add(it) } } }
    LaunchedEffect(view.id) { shown.first = false }
    Row(Modifier.fillMaxSize()) {
        CompositionLocalProvider(LocalShown provides shown) {
        Scaffold(
            modifier = Modifier.weight(1f).nestedScroll(behaviour.nestedScrollConnection),
            topBar = { ScreenBar(view, arranged, plan, place, info, behaviour, searching) { searching = it } },
            floatingActionButton = {
                if (fab != null && plan.fabPlace == FabPlace.BottomEnd) Fab(fab, place, plan.fabExtended, expanded = atTop)
            },
            snackbarHost = { Snackbars(info) },
        ) { padding ->
            BoxWithConstraints(
                Modifier
                    .fillMaxSize()
                    .padding(top = padding.calculateTopPadding())
                    .consumeWindowInsets(padding)
                    .imePadding(),
            ) {
                val items = remember(screen.body, view, maxWidth, barSubmit, hoisted, window.fontScale) {
                    blocks(screen.body, view, maxWidth.value.toInt(), omit = barSubmit, hoisted = hoisted, fontScale = window.fontScale)
                }
                val floating = (fab != null && plan.fabPlace == FabPlace.BottomEnd) || bars.isNotEmpty()
                val bottom = padding.calculateBottomPadding() + if (floating) 88.dp else 16.dp
                // The screen's content stands in one column: the same start edge and the same widest width for everything in it.
                val column = ContentRules.columnMostDp(cardsSideBySide(items))
                val pane = maxWidth.value
                val body: @Composable () -> Unit = {
                    CompositionLocalProvider(LocalPane provides pane, LocalColumn provides column) {
                        Body(items, place.copy(omit = barSubmit), list, PaddingValues(top = 8.dp, bottom = bottom))
                    }
                }
                if (plan.pullToRefresh) {
                    PullToRefreshBox(isRefreshing = view.refreshing, onRefresh = { ui.send(Events.refresh(view.id)) }, modifier = Modifier.fillMaxSize()) { body() }
                } else {
                    body()
                }
                // A fresh tree is on its way while the last one is shown.
                if (view.loading && !view.refreshing) LinearProgressIndicator(Modifier.fillMaxWidth())

                for (bar in bars) {
                    Box(Modifier.align(Alignment.BottomCenter).navigationBarsPadding().padding(bottom = 16.dp)) { ToolbarView(bar, place) }
                }

                val focus = view.focus
                LaunchedEffect(focus?.serial) {
                    if (focus != null) {
                        val index = blockOf(items, focus.target)
                        if (index >= 0) list.animateScrollToItem(index)
                    }
                }
            }
        }
        }
        if (side != null) {
            SideSheet(side, place, Modifier.width((window.widthDp * 0.4f).coerceIn(360f, 480f).dp))
        }
    }
    Overlays(place)
    QuestionDialog(place)
}

/** The screen's column of blocks, drawn as it scrolls. */
@Composable
internal fun Body(blocks: List<Block>, place: Place, state: LazyListState, padding: PaddingValues) {
    LazyColumn(state = state, contentPadding = padding, modifier = Modifier.fillMaxSize()) {
        items(count = blocks.size, key = { blocks[it].key }, contentType = { blocks[it]::class }) { index ->
            BlockView(blocks[index], place)
        }
    }
}

@Composable
private fun LazyItemScope.BlockView(block: Block, place: Place) {
    val ui = LocalUi.current
    val marginDp = ContentRules.marginDp(ui.window)
    val margin = marginDp.dp
    val column = LocalColumn.current
    val pane = LocalPane.current
    when (block) {
        is Block.Single -> {
            val inner = place.copy(form = block.form ?: place.form)
            val node = block.node
            // A table where there is room for one takes the pane's width: its columns need it.
            val most = when {
                node is Navree.Table -> ContentRules.TABLE_MOST_DP
                block.form != null -> FormRules.FORM_MOST_DP
                else -> column
            }
            Roomed(ContentRules.contentDp(pane, most, marginDp)) {
                Box(Modifier.readable(most).padding(horizontal = margin, vertical = if (node is Navree.Field) 8.dp else 0.dp)) {
                    // A form's own button under its fields sits at the form's end, as wide as its words.
                    if (node is Navree.Button && block.form != null) {
                        Box(Modifier.fillMaxWidth(), contentAlignment = Alignment.CenterEnd) { NodeView(node, inner) }
                    } else {
                        NodeView(node, inner)
                    }
                }
            }
        }
        is Block.Item -> {
            val inner = place.copy(form = block.form ?: place.form)
            when (val node = block.node) {
                is Navree.Row -> Box(if (block.group == Group.None) Modifier.readable(column) else Modifier) { RowView(node, inner, block.group) }
                is Navree.FeedRow -> Box(Modifier.readable(column)) { FeedRowView(node, inner) }
                is Navree.Field -> if (block.group == Group.None) {
                    Box(Modifier.readable(FormRules.FORM_MOST_DP)) { FieldView(node, inner, asItem = true) }
                } else {
                    Grouped(block.group) { FieldView(node, inner, asItem = true) }
                }
                else -> NodeView(node, inner)
            }
        }
        is Block.TableItem -> Box(Modifier.readable(column)) { TableItemView(block.table, block.row, place.copy(form = block.form ?: place.form)) }
        is Block.Header -> Column(Modifier.readable(column).padding(horizontal = margin).padding(top = 16.dp, bottom = 8.dp)) {
            Text(block.text, style = MaterialTheme.typography.titleSmall, color = MaterialTheme.colorScheme.primary, modifier = Modifier.semantics { heading() })
            block.subtitle?.let { Text(it, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant) }
        }
        is Block.Filters -> FilterBar(block.form, place)
        is Block.Waiting -> Box(Modifier.fillMaxWidth().padding(24.dp), contentAlignment = Alignment.Center) { Waiting(Modifier.size(40.dp)) }
        is Block.Space -> Spacer(Modifier.height(block.dp.dp))
        is Block.More -> {
            val state = place.screen.lists[block.id]
            if (state?.failed == true) {
                RetryLine(stringResource(R.string.navree_more_failed)) { ui.send(Events.more(place.id, block.id)) }
            } else {
                // Drawing the end of the list is what asks for the next page; the core asks once.
                LaunchedEffect(block.key) { ui.send(Events.more(place.id, block.id)) }
                Box(Modifier.fillMaxWidth().padding(16.dp), contentAlignment = Alignment.Center) {
                    CircularProgressIndicator(Modifier.size(24.dp), strokeWidth = 2.dp)
                }
            }
        }
        is Block.Fill -> Box(Modifier.fillParentMaxSize(), contentAlignment = Alignment.Center) { NodeView(block.node, place) }
        is Block.Before -> {
            val state = place.screen.lists[block.id]
            if (state?.failedBefore == true) {
                RetryLine(stringResource(R.string.navree_more_failed)) { ui.send(Events.more(place.id, block.id, before = true)) }
            } else {
                LaunchedEffect(block.key) { ui.send(Events.more(place.id, block.id, before = true)) }
                if (state?.loadingBefore == true) {
                    Box(Modifier.fillMaxWidth().padding(16.dp), contentAlignment = Alignment.Center) {
                        CircularProgressIndicator(Modifier.size(24.dp), strokeWidth = 2.dp)
                    }
                }
            }
        }
        is Block.Cards -> {
            val content = ContentRules.contentDp(pane, column, marginDp)
            // Where the room is known, the cards of a line are as tall as the tallest of them.
            val even = content > 0f && block.nodes.all(::measuresAhead)
            Row(
                Modifier.readable(column).padding(horizontal = margin).then(if (even) Modifier.height(IntrinsicSize.Min) else Modifier),
                horizontalArrangement = Arrangement.spacedBy(ContentRules.CARD_GAP_DP.dp),
            ) {
                val inner = place.copy(form = block.form ?: place.form)
                val width = if (content > 0f) ContentRules.cardWidthDp(content, block.columns) else 0f
                for (node in block.nodes) Box(Modifier.weight(1f).fillMaxHeight()) { Roomed(width) { NodeView(node, inner, Modifier.fillMaxHeight()) } }
                repeat(block.columns - block.nodes.size) { Spacer(Modifier.weight(1f)) }
            }
        }
    }
}

@Composable
private fun Fab(button: Navree.Button, place: Place, extended: Boolean, expanded: Boolean) {
    val ui = LocalUi.current
    val vector = rememberIcon(button.icon, button.standard) ?: Glyphs.Add
    if (extended) {
        ExtendedFloatingActionButton(
            text = { Text(button.label) },
            icon = { androidx.compose.material3.Icon(vector, contentDescription = null) },
            onClick = { ui.press(place, button) },
            expanded = expanded,
        )
    } else {
        FloatingActionButton(onClick = { ui.press(place, button) }) { androidx.compose.material3.Icon(vector, contentDescription = button.label) }
    }
}

/** The floating action button as the rail's head carries it. */
@Composable
internal fun RailFab(screen: ScreenView, plan: ScreenPlan) {
    val tree = screen.screen ?: return
    val button = plan.fab?.let { tree.actions.getOrNull(it) as? Navree.Button } ?: return
    Fab(button, Place(screen), extended = plan.fabExtended, expanded = true)
}

/**
 * A screen's actions and the toolbars that float over it. Where a toolbar may
 * not float (above a navigation bar), its items join the screen's actions and
 * are placed in the app bar and its overflow like any other.
 */
internal fun arrange(screen: Navree.Screen, view: ScreenView, context: ScreenContext): Pair<Navree.Screen, List<Navree.Toolbar>> {
    val bars = screenToolbars(screen, view)
    if (bars.isEmpty() || ScreenRules.toolbarFloats(context)) return screen to bars
    val items = bars.flatMap { it.items }.map { item ->
        if (item.menu.isNotEmpty()) Navree.Menu(icon = item.icon, id = item.id, items = item.menu, label = item.title) else item.asButton()
    }
    return screen.copy(actions = screen.actions.filterNot { it is Navree.Toolbar } + items) to emptyList()
}

/** A toolbar item as the button it stands for, so it is pressed (and asks) like one. */
private fun Navree.ToolbarItem.asButton(): Navree.Button =
    Navree.Button(action = action, disabled = disabled, icon = icon, iconOnly = true, id = id, label = title, prominence = prominence, role = role, standard = standard, undoable = undoable, ask = ask)

/**
 * A toolbar of the page as Material 3's floating toolbar: its items as icon
 * buttons named by their titles, a menu where an item has one, a destructive
 * item in the error colour and asking first.
 */
@Composable
private fun ToolbarView(bar: Navree.Toolbar, place: Place) {
    val ui = LocalUi.current
    FloatingActions {
        for (item in bar.items) {
            val vector = rememberIcon(item.icon, item.standard) ?: Glyphs.More
            if (item.menu.isNotEmpty()) {
                var open by remember { mutableStateOf(false) }
                Box {
                    TipIconButton(item.title, vector, { open = true }, enabled = !item.disabled)
                    DropdownMenu(expanded = open, onDismissRequest = { open = false }) { MenuItems(item.menu, place) { open = false } }
                }
            } else {
                val button = item.asButton()
                TipIconButton(
                    item.title,
                    vector,
                    { ui.press(place, button) },
                    enabled = !item.disabled,
                    tint = if (ScreenRules.destructive(button)) MaterialTheme.colorScheme.error else androidx.compose.ui.graphics.Color.Unspecified,
                )
            }
        }
    }
}

@Composable
private fun ScreenBar(
    view: ScreenView,
    screen: Navree.Screen,
    plan: ScreenPlan,
    place: Place,
    info: HostInfo,
    behaviour: TopAppBarScrollBehavior,
    searching: Boolean,
    setSearching: (Boolean) -> Unit,
) {
    val navigationIcon: @Composable () -> Unit = { NavigationIconView(plan.navigationIcon, view, screen.back) }
    val actions: @Composable RowScope.() -> Unit = { BarActions(screen, plan, place, info, setSearching) }
    // While rows are chosen the app bar is about them.
    val table = remember(screen) { SelectionRules.selectable(screen.body) }
    if (table != null) {
        val across = table.id?.let { view.lists[it]?.across } == true
        val count = SelectionRules.chosen(table, across).size
        if (count > 0 || across) {
            ContextualBar(screen, table, count, across, place)
            return
        }
    }
    val pager = remember(screen) { PagerRules.inBar(screen) }
    when {
        plan.bar == BarKind.None -> Unit
        pager != null && !searching -> TopAppBar(
            title = { DayPagerView(pager, place, inBar = true) },
            navigationIcon = navigationIcon,
            actions = actions,
            scrollBehavior = behaviour,
        )
        plan.bar == BarKind.Search || (searching && plan.search != SearchPlace.None) ->
            SearchTopBar(view, screen, plan, place, info, temporary = plan.bar != BarKind.Search) { setSearching(false) }
        else -> TopAppBar(
            title = {
                val subtitle = screen.subtitle
                if (plan.bar == BarKind.TwoLine && subtitle != null) {
                    Column {
                        Text(screen.title, maxLines = 1, overflow = TextOverflow.Ellipsis)
                        Text(subtitle, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 1, overflow = TextOverflow.Ellipsis)
                    }
                } else {
                    Text(screen.title, maxLines = 1, overflow = TextOverflow.Ellipsis)
                }
            },
            navigationIcon = navigationIcon,
            actions = actions,
            scrollBehavior = behaviour,
        )
    }
}

/**
 * Material's contextual app bar while rows are chosen: how many, a way to
 * clear them (also the system's back), the offer to choose every row when all
 * those held are, and the actions of the form the rows go with.
 */
@Composable
private fun ContextualBar(screen: Navree.Screen, table: Navree.Table, count: Int, across: Boolean, place: Place) {
    val ui = LocalUi.current
    val choosing = Choosing(table, across)
    val form = remember(screen, table) { SelectionRules.bulkForm(screen.body, table) }
    val total = table.select?.across?.total
    val numbers = remember { java.text.NumberFormat.getIntegerInstance() }
    BackHandler { choosing.tickAll(ui, place, on = false) }
    TopAppBar(
        title = {
            Text(
                if (across && total != null) stringResource(R.string.navree_all_chosen, numbers.format(total)) else pluralStringResource(R.plurals.navree_chosen, count, count),
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.semantics { liveRegion = LiveRegionMode.Polite },
            )
        },
        navigationIcon = { TipIconButton(stringResource(R.string.navree_clear_choice), Glyphs.Close, { choosing.tickAll(ui, place, on = false) }) },
        actions = {
            if (SelectionRules.offerAcross(table, across) && total != null) {
                TextButton(onClick = { choosing.tickAll(ui, place, on = true, across = true) }) {
                    Text(stringResource(R.string.navree_choose_across, numbers.format(total)), maxLines = 1)
                }
            }
            if (form != null) {
                val inForm = place.copy(form = form.id)
                for (button in form.children.filterIsInstance<Navree.Button>()) {
                    val icon = rememberIcon(button.icon, button.standard)
                    val danger = ScreenRules.destructive(button)
                    if (icon != null) {
                        TipIconButton(button.label, icon, { ui.press(inForm, button) }, enabled = usable(button, inForm), tint = if (danger) MaterialTheme.colorScheme.error else Color.Unspecified)
                    } else {
                        TextButton(onClick = { ui.press(inForm, button) }, enabled = usable(button, inForm)) {
                            Text(button.label, color = if (danger) MaterialTheme.colorScheme.error else Color.Unspecified, maxLines = 1)
                        }
                    }
                }
            }
        },
        colors = TopAppBarDefaults.topAppBarColors(containerColor = MaterialTheme.colorScheme.secondaryContainer),
    )
}

@Composable
private fun RowScope.BarActions(screen: Navree.Screen, plan: ScreenPlan, place: Place, info: HostInfo, setSearching: (Boolean) -> Unit) {
    val ui = LocalUi.current
    if (plan.search == SearchPlace.Action) {
        TipIconButton(screen.search?.placeholder ?: stringResource(R.string.navree_search), Glyphs.Search, { setSearching(true) })
    }
    for (action in plan.barActions) {
        when (val node = screen.actions.getOrNull(action.index)) {
            is Navree.Button -> if (action.style == BarActionStyle.Text) {
                TextButton(onClick = { ui.press(place, node) }, enabled = usable(node, place)) { Text(node.label) }
            } else {
                TipIconButton(node.label, rememberIcon(node.icon, node.standard) ?: Glyphs.More, { ui.press(place, node) }, enabled = usable(node, place))
            }
            is Navree.Menu -> MenuButtonView(node, place)
            else -> Unit
        }
    }
    val form = screen.body as? Navree.Form
    val submit = form?.submitButton()
    if (plan.submit == SubmitPlace.Bar && form != null && submit != null) {
        val inForm = place.copy(form = form.id)
        TextButton(onClick = { ui.press(inForm, submit) }, enabled = usable(submit, inForm)) {
            if (busy(submit, inForm)) CircularProgressIndicator(Modifier.size(18.dp), strokeWidth = 2.dp) else Text(submit.label)
        }
    }
    if (plan.overflow.isNotEmpty()) Overflow(plan.overflow.mapNotNull { screen.actions.getOrNull(it) }, place)
    val user = info.user
    if (plan.avatar && user != null) AccountMenu(user, place)
}

/** The overflow menu: what did not fit in the bar, then the destructive actions under a divider. */
@Composable
private fun Overflow(actions: List<Navree.Node>, place: Place) {
    var open by remember { mutableStateOf(false) }
    Box {
        TipIconButton(stringResource(R.string.navree_more_options), Glyphs.More, { open = true })
        DropdownMenu(expanded = open, onDismissRequest = { open = false }) {
            val firstDestructive = actions.indexOfFirst(ScreenRules::destructive)
            actions.forEachIndexed { index, action ->
                if (index == firstDestructive && index > 0) HorizontalDivider(Modifier.padding(vertical = 8.dp))
                MenuItems(listOf(action), place) { open = false }
            }
        }
    }
}

/** The signed-in person at the end of the bar: their avatar, and under it who they are and what they can do. */
@Composable
private fun AccountMenu(user: Navree.User, place: Place) {
    var open by remember { mutableStateOf(false) }
    val label = stringResource(R.string.navree_account)
    Box {
        IconButton(onClick = { open = true }, modifier = Modifier.semantics { contentDescription = "$label, ${user.name}" }) {
            AvatarView(Navree.Avatar(name = user.name), size = 32)
        }
        DropdownMenu(expanded = open, onDismissRequest = { open = false }) {
            Column(Modifier.padding(horizontal = 12.dp, vertical = 8.dp)) {
                Text(user.name, style = MaterialTheme.typography.titleSmall)
                user.email?.let { Text(it, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant) }
            }
            if (user.menu.isNotEmpty()) HorizontalDivider(Modifier.padding(vertical = 4.dp))
            // The person's menu belongs to no screen: its taps say none.
            MenuItems(user.menu, place) { open = false }
        }
    }
}

/**
 * The search bar in the app bar's place, opening into Material's search
 * view: while it is open the screen's own results fill it and change as the
 * words do. The words of a table's query go to the core as they are typed
 * (it waits for a pause); a screen's own search is sent when submitted.
 * `temporary` is the bar a deeper screen shows only while it is searched.
 */
@Composable
private fun SearchTopBar(
    view: ScreenView,
    screen: Navree.Screen,
    plan: ScreenPlan,
    place: Place,
    info: HostInfo,
    temporary: Boolean,
    leave: () -> Unit,
) {
    val ui = LocalUi.current
    val focus = LocalFocusManager.current
    val own = screen.search
    val query = if (own == null) ScreenRules.query(screen) else null
    val field = query?.search
    if (own == null && field == null) return
    val inQuery = place.copy(form = query?.form?.id)
    val current = if (own != null) own.value else (field?.let { inQuery.value(it) } as? FieldValue.Text)?.text ?: (field?.input as? Navree.TextInput)?.value ?: ""
    val hint = own?.placeholder ?: (field?.input as? Navree.TextInput)?.placeholder ?: field?.label
    var text by remember { mutableStateOf(current) }
    var expanded by rememberSaveable { mutableStateOf(temporary) }
    val echo = remember { Echo() }
    if (echo.changedOutside(current, text)) text = current

    fun submit() {
        if (own != null) ui.send(Events.search(view.id, text)) else ui.act(inQuery, Navree.Action.Submit())
    }

    fun type(next: String) {
        if (next == text) return
        text = next
        echo.report(next)
        if (own == null && field != null) ui.change(inQuery, field, FieldValue.Text(next))
    }

    fun close() {
        expanded = false
        focus.clearFocus()
        if (temporary) leave()
    }

    BackHandler(enabled = expanded) { close() }

    val input: @Composable () -> Unit = {
        SearchBarDefaults.InputField(
            query = text,
            onQueryChange = ::type,
            onSearch = {
                submit()
                focus.clearFocus()
            },
            expanded = expanded,
            onExpandedChange = { expanded = it },
            placeholder = { Text(hint ?: stringResource(R.string.navree_search), maxLines = 1, overflow = TextOverflow.Ellipsis) },
            leadingIcon = {
                when {
                    expanded -> TipIconButton(stringResource(R.string.navree_back), Glyphs.Back, { close() })
                    plan.navigationIcon == NavigationIcon.Menu -> NavigationIconView(NavigationIcon.Menu, view)
                    else -> Icon(Glyphs.Search, contentDescription = null)
                }
            },
            trailingIcon = {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    if (text.isNotEmpty()) {
                        TipIconButton(stringResource(R.string.navree_clear), Glyphs.Close, {
                            type("")
                            submit()
                        })
                    } else if (!expanded) {
                        for (action in plan.barActions) {
                            when (val node = screen.actions.getOrNull(action.index)) {
                                is Navree.Button -> TipIconButton(node.label, rememberIcon(node.icon, node.standard) ?: Glyphs.More, { ui.press(place, node) }, enabled = usable(node, place))
                                is Navree.Menu -> MenuButtonView(node, place)
                                else -> Unit
                            }
                        }
                        if (plan.overflow.isNotEmpty()) Overflow(plan.overflow.mapNotNull { screen.actions.getOrNull(it) }, place)
                        val user = info.user
                        if (plan.avatar && user != null) AccountMenu(user, place)
                    }
                }
            },
        )
    }

    Surface(color = MaterialTheme.colorScheme.surface) {
        Box(Modifier.fillMaxWidth().padding(bottom = if (expanded) 0.dp else 8.dp), contentAlignment = Alignment.TopCenter) {
            SearchBar(
                inputField = input,
                expanded = expanded,
                onExpandedChange = { expanded = it },
                modifier = if (expanded) Modifier.fillMaxWidth() else Modifier.fillMaxWidth().padding(horizontal = 16.dp),
            ) {
                // The search view: the screen's results, as they change with the words.
                val results = remember(screen.body, view) { blocks(screen.body, view, 400, hoisted = query?.form).filter { it is Block.TableItem || it is Block.Item || it is Block.More || it is Block.Fill } }
                Body(results, place, rememberLazyListState(), PaddingValues(bottom = 16.dp))
            }
        }
    }
}

/**
 * A table's filters as a row of chips over its rows, the way Android's own
 * apps offer them: a chip a filter, lit when it is in force, opening a menu
 * of its choices. Choosing tells the core, which asks for the rows again.
 */
@Composable
private fun FilterBar(form: Navree.Form, place: Place) {
    val ui = LocalUi.current
    val inForm = place.copy(form = form.id)
    val filters = remember(form) { form.fields().filter(FilterRules::isFilter) }
    if (filters.isEmpty()) return
    val margin = ContentRules.marginDp(ui.window).dp
    LazyRow(
        contentPadding = PaddingValues(horizontal = margin),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
        modifier = Modifier.fillMaxWidth().padding(bottom = 4.dp),
    ) {
        items(filters.size, key = { filters[it].id ?: filters[it].name }) { index ->
            val field = filters[index]
            val label = field.label ?: field.name
            var open by remember { mutableStateOf(false) }
            when (val input = field.input) {
                is Navree.ChoicesInput -> {
                    val chosen = (inForm.value(field) as? FieldValue.Many)?.values ?: input.values
                    val named = input.options.filter { it.value in chosen }
                    Box {
                        FilterChip(
                            selected = chosen.isNotEmpty(),
                            onClick = { open = true },
                            label = {
                                Text(
                                    when (named.size) {
                                        0 -> label
                                        1 -> named.first().label
                                        else -> "$label · ${named.size}"
                                    },
                                )
                            },
                            trailingIcon = { Icon(Glyphs.ExpandMore, contentDescription = null, modifier = Modifier.size(18.dp)) },
                        )
                        DropdownMenu(expanded = open, onDismissRequest = { open = false }) {
                            for (option in input.options) {
                                val on = option.value in chosen
                                DropdownMenuItem(
                                    text = { Text(option.label) },
                                    leadingIcon = { if (on) Icon(Glyphs.Check, contentDescription = null) else Spacer(Modifier.size(24.dp)) },
                                    trailingIcon = option.detail?.let { detail -> { Text(detail, style = MaterialTheme.typography.labelMedium) } },
                                    onClick = {
                                        val next = if (on) chosen - option.value else chosen + option.value
                                        ui.change(inForm, field, FieldValue.Many(input.options.map { it.value }.filter { it in next }))
                                    },
                                    modifier = Modifier.semantics { selected = on },
                                )
                            }
                        }
                    }
                }
                is Navree.ChoiceInput -> {
                    val value = (inForm.value(field) as? FieldValue.Text)?.text ?: input.value
                    val named = input.options.firstOrNull { it.value == value }
                    Box {
                        FilterChip(
                            selected = named != null && named.value.isNotEmpty(),
                            onClick = { open = true },
                            label = { Text(if (named != null && named.value.isNotEmpty()) named.label else label) },
                            trailingIcon = { Icon(Glyphs.ExpandMore, contentDescription = null, modifier = Modifier.size(18.dp)) },
                        )
                        DropdownMenu(expanded = open, onDismissRequest = { open = false }) {
                            for (option in input.options) {
                                DropdownMenuItem(
                                    text = { Text(option.label) },
                                    leadingIcon = { if (option.value == value) Icon(Glyphs.Check, contentDescription = null) else Spacer(Modifier.size(24.dp)) },
                                    onClick = {
                                        open = false
                                        ui.change(inForm, field, FieldValue.Text(option.value))
                                    },
                                )
                            }
                        }
                    }
                }
                is Navree.ToggleInput -> {
                    val on = (inForm.value(field) as? FieldValue.Flag)?.on ?: input.on
                    FilterChip(
                        selected = on,
                        onClick = { ui.change(inForm, field, FieldValue.Flag(!on)) },
                        label = { Text(label) },
                        leadingIcon = if (on) ({ Icon(Glyphs.Check, contentDescription = null, modifier = Modifier.size(18.dp)) }) else null,
                    )
                }
                else -> Unit
            }
        }
    }
}
