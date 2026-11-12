package com.navree.client.ui

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.consumeWindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyItemScope
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.FilterChip
import androidx.compose.material3.ExtendedFloatingActionButton
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
import androidx.compose.ui.input.nestedscroll.nestedScroll
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.navree.client.R
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
import com.navree.client.design.SearchPlace
import com.navree.client.design.SubmitPlace
import com.navree.client.protocol.ActionRole
import com.navree.client.protocol.AvatarNode
import com.navree.client.protocol.ButtonNode
import com.navree.client.protocol.ChoiceInput
import com.navree.client.protocol.ChoicesInput
import com.navree.client.protocol.TextInput
import com.navree.client.protocol.ToggleInput
import com.navree.client.protocol.FieldNode
import com.navree.client.protocol.FormNode
import com.navree.client.protocol.MenuNode
import com.navree.client.protocol.Navigate
import com.navree.client.protocol.Node
import com.navree.client.protocol.Present
import com.navree.client.protocol.RowNode
import com.navree.client.protocol.Screen
import com.navree.client.protocol.Submit
import com.navree.client.protocol.User
import com.navree.client.runtime.Content
import com.navree.client.runtime.Entry
import com.navree.client.runtime.Event
import com.navree.client.runtime.Failure
import com.navree.client.runtime.FieldValue
import com.navree.client.runtime.fields
import com.navree.client.runtime.submitButton
import kotlinx.coroutines.delay

/** Opens the full list of destinations over the content. Set by the frame that owns the navigation. */
internal val LocalOpenMenu = staticCompositionLocalOf<() -> Unit> { {} }

/** What a screen is drawn with, beyond its own entry. */
internal data class HostInfo(
    val context: ScreenContext,
    val user: User?,
    val name: String,
    /** This is the screen the user is looking at: it shows the passing messages. */
    val front: Boolean,
    val selectedUrl: String? = null,
)

/** One screen: its bar, its content and what floats over it, placed by the design rules. */
@Composable
internal fun ScreenHost(entry: Entry, info: HostInfo) {
    when (val content = entry.content) {
        Content.Blank -> BlankScreen(entry, info)
        is Content.Web -> WebScreen(entry, content, info)
        is Content.Native -> NativeScreen(entry, content.screen, info)
    }
}

@Composable
private fun NavigationIconView(icon: NavigationIcon, entry: Entry, info: HostInfo, back: String? = null) {
    val ui = LocalUi.current
    val openMenu = LocalOpenMenu.current
    when (icon) {
        NavigationIcon.None -> Unit
        NavigationIcon.Back -> IconButton(
            onClick = {
                if (info.context.canGoBack || back == null) {
                    ui.dispatch(Event.BackPressed)
                } else {
                    // Opened directly, with nothing under it: "up" is where the page says it is.
                    ui.dispatch(Event.Acted(entry.key, Navigate(back, Present.Replace)))
                }
            },
        ) { Icon(Glyphs.Back, contentDescription = stringResource(R.string.navree_back)) }
        NavigationIcon.Close -> IconButton(onClick = { ui.dispatch(Event.BackPressed) }) {
            Icon(Glyphs.Close, contentDescription = stringResource(R.string.navree_close))
        }
        NavigationIcon.Menu -> IconButton(onClick = openMenu) {
            Icon(Glyphs.Menu, contentDescription = stringResource(R.string.navree_menu))
        }
    }
}

/** The icon a screen with nothing drawn yet still needs: a way out. */
private fun wayOut(info: HostInfo): NavigationIcon = when {
    !info.context.chrome -> NavigationIcon.None
    info.context.canGoBack -> NavigationIcon.Back
    info.context.presentation != Presentation.Stack -> NavigationIcon.Close
    else -> NavigationIcon.None
}

@Composable
private fun BlankScreen(entry: Entry, info: HostInfo) {
    val ui = LocalUi.current
    Scaffold(
        topBar = {
            if (info.context.chrome) {
                TopAppBar(title = {}, navigationIcon = { NavigationIconView(wayOut(info), entry, info) })
            }
        },
        snackbarHost = { if (info.front) SnackbarHost(ui.snackbars) },
    ) { padding ->
        Box(Modifier.fillMaxSize().padding(padding), contentAlignment = Alignment.Center) {
            val failure = entry.failure
            if (failure != null && !entry.loading) {
                FailureState(failure) { ui.dispatch(Event.Refreshed(entry.key)) }
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
        delay(350)
        show = true
    }
    val label = stringResource(R.string.navree_loading)
    if (show) CircularProgressIndicator(Modifier.semantics { contentDescription = label })
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
private fun WebScreen(entry: Entry, content: Content.Web, info: HostInfo) {
    val ui = LocalUi.current
    var title by rememberSaveable(entry.key) { mutableStateOf("") }
    Scaffold(
        topBar = {
            if (info.context.chrome) {
                TopAppBar(
                    title = { Text(title.ifEmpty { info.name }, maxLines = 1, overflow = TextOverflow.Ellipsis) },
                    navigationIcon = { NavigationIconView(wayOut(info), entry, info) },
                )
            }
        },
        snackbarHost = { if (info.front) SnackbarHost(ui.snackbars) },
    ) { padding ->
        WebPage(
            url = content.url,
            origin = ui.origin,
            onLink = { ui.dispatch(Event.LinkFollowed(entry.key, it)) },
            onTitle = { title = it },
            modifier = Modifier.padding(padding).consumeWindowInsets(padding).imePadding(),
        )
    }
}

@Composable
private fun NativeScreen(entry: Entry, screen: Screen, info: HostInfo) {
    val ui = LocalUi.current
    val window = ui.window
    val plan = remember(screen, info.context, window) { ScreenRules.plan(screen, info.context, window) }
    val place = Place(entry, selectedUrl = info.selectedUrl)
    val list = rememberLazyListState()
    var searching by rememberSaveable { mutableStateOf(false) }
    val pinned = TopAppBarDefaults.pinnedScrollBehavior()
    val leaving = TopAppBarDefaults.enterAlwaysScrollBehavior()
    val behaviour = if (plan.scroll == BarScroll.EnterAlways) leaving else pinned
    // What the app bar holds is not drawn a second time in the content.
    val form = screen.body as? FormNode
    val barSubmit = if (plan.submit == SubmitPlace.Bar) form?.submitButton() else null
    val query = remember(screen) { ScreenRules.query(screen) }
    val hoisted = if (plan.searchInQuery) query?.form else null
    val atTop by remember { derivedStateOf { list.firstVisibleItemIndex == 0 && list.firstVisibleItemScrollOffset < 24 } }
    val fab = plan.fab?.let { screen.actions.getOrNull(it) as? ButtonNode }

    Scaffold(
        modifier = Modifier.nestedScroll(behaviour.nestedScrollConnection),
        topBar = { ScreenBar(entry, screen, plan, place, info, behaviour, searching) { searching = it } },
        floatingActionButton = {
            if (fab != null && plan.fabPlace == FabPlace.BottomEnd) Fab(fab, place, plan.fabExtended, expanded = atTop)
        },
        snackbarHost = { if (info.front) SnackbarHost(ui.snackbars) },
    ) { padding ->
        BoxWithConstraints(
            Modifier
                .fillMaxSize()
                .padding(top = padding.calculateTopPadding())
                .consumeWindowInsets(padding)
                .imePadding(),
        ) {
            val items = remember(screen.body, entry.local, maxWidth, barSubmit, hoisted) {
                blocks(screen.body, entry.local, maxWidth.value.toInt(), omit = barSubmit, hoisted = hoisted)
            }
            val bottom = padding.calculateBottomPadding() + if (fab != null && plan.fabPlace == FabPlace.BottomEnd) 88.dp else 16.dp
            val body: @Composable () -> Unit = {
                Body(items, place.copy(omit = barSubmit), list, PaddingValues(top = 8.dp, bottom = bottom))
            }
            if (plan.pullToRefresh) {
                PullToRefreshBox(
                    isRefreshing = entry.refreshing,
                    onRefresh = { ui.dispatch(Event.Refreshed(entry.key)) },
                    modifier = Modifier.fillMaxSize(),
                ) { body() }
            } else {
                body()
            }
            // A fresh tree is on its way while the last one is shown.
            if (entry.loading && !entry.refreshing) LinearProgressIndicator(Modifier.fillMaxWidth())

            val focus = entry.local.focus
            LaunchedEffect(focus?.serial) {
                if (focus != null) {
                    val index = blockOf(items, focus.target)
                    if (index >= 0) list.animateScrollToItem(index)
                }
            }
        }
    }
    Overlays(entry, screen)
}

@Composable
private fun Body(blocks: List<Block>, place: Place, state: LazyListState, padding: PaddingValues) {
    LazyColumn(state = state, contentPadding = padding, modifier = Modifier.fillMaxSize()) {
        items(count = blocks.size, key = { blocks[it].key }, contentType = { blocks[it]::class }) { index ->
            BlockView(blocks[index], place)
        }
    }
}

@Composable
private fun LazyItemScope.BlockView(block: Block, place: Place) {
    val ui = LocalUi.current
    val margin = ContentRules.marginDp(ui.window).dp
    when (block) {
        is Block.Single -> {
            val inner = place.copy(form = block.form)
            val most = if (block.form != null) FormRules.FORM_MOST_DP else ContentRules.CONTENT_MOST_DP
            val node = block.node
            Box(Modifier.readable(most).padding(horizontal = margin, vertical = if (node is FieldNode) 8.dp else 0.dp)) {
                // A form's own button under its fields spans the form.
                val wide = node is ButtonNode && block.form != null
                NodeView(node, inner, if (wide) Modifier.fillMaxWidth() else Modifier)
            }
        }
        is Block.Item -> {
            val inner = place.copy(form = block.form)
            when (val node = block.node) {
                is RowNode -> Box(if (block.group == Group.None) Modifier.readable() else Modifier) { RowView(node, inner, block.group) }
                is FieldNode -> if (block.group == Group.None) {
                    Box(Modifier.readable(FormRules.FORM_MOST_DP)) { FieldView(node, inner, asItem = true) }
                } else {
                    Grouped(block.group) { FieldView(node, inner, asItem = true) }
                }
                else -> NodeView(node, inner)
            }
        }
        is Block.TableItem -> Box(Modifier.readable()) { TableItemView(block.table, block.row, place) }
        is Block.Header -> Column(Modifier.readable().padding(horizontal = margin).padding(top = 16.dp, bottom = 8.dp)) {
            Text(
                block.text,
                style = MaterialTheme.typography.titleSmall,
                color = MaterialTheme.colorScheme.primary,
                modifier = Modifier.semantics { heading() },
            )
            block.subtitle?.let {
                Text(it, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
        }
        is Block.Filters -> FilterBar(block.form, place)
        is Block.Waiting -> Box(Modifier.fillMaxWidth().padding(24.dp), contentAlignment = Alignment.Center) {
            CircularProgressIndicator(Modifier.size(24.dp), strokeWidth = 2.dp)
        }
        is Block.Footer -> Text(
            block.text,
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.readable().padding(horizontal = margin).padding(top = 8.dp),
        )
        is Block.Space -> Spacer(Modifier.height(block.dp.dp))
        is Block.More -> {
            // Drawing the end of the list is what asks for the next page.
            LaunchedEffect(block.more) { ui.dispatch(Event.EndReached(place.key, block.list)) }
            Box(Modifier.fillMaxWidth().padding(16.dp), contentAlignment = Alignment.Center) {
                CircularProgressIndicator(Modifier.size(24.dp), strokeWidth = 2.dp)
            }
        }
        is Block.Fill -> Box(Modifier.fillParentMaxSize(), contentAlignment = Alignment.Center) {
            NodeView(block.node, place)
        }
    }
}

@Composable
private fun Fab(button: ButtonNode, place: Place, extended: Boolean, expanded: Boolean) {
    val ui = LocalUi.current
    val vector = rememberVector(button.icon) ?: Glyphs.Add
    if (extended) {
        ExtendedFloatingActionButton(
            text = { Text(button.label) },
            icon = { Icon(vector, contentDescription = null) },
            onClick = { ui.press(place, button) },
            expanded = expanded,
        )
    } else {
        FloatingActionButton(onClick = { ui.press(place, button) }) {
            Icon(vector, contentDescription = button.label)
        }
    }
}

/** The floating action button as the rail's head carries it. */
@Composable
internal fun RailFab(entry: Entry, screen: Screen, plan: ScreenPlan) {
    val button = plan.fab?.let { screen.actions.getOrNull(it) as? ButtonNode } ?: return
    Fab(button, Place(entry), extended = plan.fabExtended, expanded = true)
}

@Composable
private fun ScreenBar(
    entry: Entry,
    screen: Screen,
    plan: ScreenPlan,
    place: Place,
    info: HostInfo,
    behaviour: TopAppBarScrollBehavior,
    searching: Boolean,
    setSearching: (Boolean) -> Unit,
) {
    val navigationIcon: @Composable () -> Unit = { NavigationIconView(plan.navigationIcon, entry, info, screen.back?.href) }
    val actions: @Composable RowScope.() -> Unit = { BarActions(screen, plan, place, info, setSearching) }
    when {
        plan.bar == BarKind.None -> Unit
        plan.bar == BarKind.Search || (searching && plan.search != SearchPlace.None) ->
            SearchTopBar(entry, screen, plan, place, info, temporary = plan.bar != BarKind.Search) { setSearching(false) }
        else -> TopAppBar(
            title = {
                val subtitle = screen.subtitle
                if (plan.bar == BarKind.TwoLine && subtitle != null) {
                    // The subtitle under the title, both on one line each.
                    Column {
                        Text(screen.title, maxLines = 1, overflow = TextOverflow.Ellipsis)
                        Text(
                            subtitle,
                            style = MaterialTheme.typography.bodyMedium,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                        )
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

@Composable
private fun RowScope.BarActions(screen: Screen, plan: ScreenPlan, place: Place, info: HostInfo, setSearching: (Boolean) -> Unit) {
    val ui = LocalUi.current
    if (plan.search == SearchPlace.Action) {
        IconButton(onClick = { setSearching(true) }) {
            Icon(Glyphs.Search, contentDescription = screen.search?.placeholder ?: stringResource(R.string.navree_search))
        }
    }
    for (action in plan.barActions) {
        when (val node = screen.actions.getOrNull(action.index)) {
            is ButtonNode -> if (action.style == BarActionStyle.Text) {
                TextButton(onClick = { ui.press(place, node) }, enabled = usable(node, place)) { Text(node.label) }
            } else {
                IconButton(onClick = { ui.press(place, node) }, enabled = usable(node, place)) {
                    Icon(rememberVector(node.icon) ?: Glyphs.More, contentDescription = node.label)
                }
            }
            is MenuNode -> MenuButtonView(node, place)
            else -> Unit
        }
    }
    val form = screen.body as? FormNode
    val submit = form?.submitButton()
    if (plan.submit == SubmitPlace.Bar && form != null && submit != null) {
        val inForm = place.copy(form = form.key)
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
private fun Overflow(actions: List<Node>, place: Place) {
    var open by remember { mutableStateOf(false) }
    Box {
        IconButton(onClick = { open = true }) {
            Icon(Glyphs.More, contentDescription = stringResource(R.string.navree_more_options))
        }
        DropdownMenu(expanded = open, onDismissRequest = { open = false }) {
            val firstDestructive = actions.indexOfFirst { it is ButtonNode && it.role == ActionRole.Destructive }
            actions.forEachIndexed { index, action ->
                if (index == firstDestructive && index > 0) HorizontalDivider(Modifier.padding(vertical = 8.dp))
                MenuItems(listOf(action), place) { open = false }
            }
        }
    }
}

/** The signed-in user at the end of the bar: their avatar, and under it who they are and what they can do. */
@Composable
private fun AccountMenu(user: User, place: Place) {
    var open by remember { mutableStateOf(false) }
    val label = stringResource(R.string.navree_account)
    Box {
        IconButton(onClick = { open = true }, modifier = Modifier.semantics { contentDescription = "$label, ${user.name}" }) {
            AvatarView(AvatarNode(src = user.avatar, name = user.name), Modifier.size(32.dp))
        }
        DropdownMenu(expanded = open, onDismissRequest = { open = false }) {
            Column(Modifier.padding(horizontal = 12.dp, vertical = 8.dp)) {
                Text(user.name, style = MaterialTheme.typography.titleSmall)
                user.email?.let {
                    Text(it, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
            }
            if (user.menu.isNotEmpty()) HorizontalDivider(Modifier.padding(vertical = 4.dp))
            MenuItems(user.menu, place) { open = false }
        }
    }
}

/**
 * The search bar in the app bar's place. What is typed is asked for after a
 * short pause and the screen's own list answers, so results filter in place.
 * `temporary` is the bar a deeper screen shows only while it is searched.
 */
@Composable
private fun SearchTopBar(
    entry: Entry,
    screen: Screen,
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
    if (own == null && (query == null || field == null)) return
    val formKey = query?.form?.key
    // The words in the box: the screen's own search, or the search field of its table's query.
    val applied = own?.value ?: (field?.input as? TextInput)?.value ?: ""
    val typed = if (own != null) entry.local.search else (entry.local.value(formKey, field?.name ?: "") as? FieldValue.Text)?.text
    val current = typed ?: applied
    val hint = own?.placeholder ?: (field?.input as? TextInput)?.placeholder ?: field?.label
    var text by remember { mutableStateOf(current) }
    val echo = remember { Echo() }
    if (echo.changedOutside(current, text)) text = current

    fun run() {
        if (own != null) {
            ui.dispatch(Event.SearchRun(entry.key))
        } else if (formKey != null) {
            ui.dispatch(Event.Acted(entry.key, Submit(form = formKey)))
        }
    }

    fun type(next: String) {
        if (next == text) return
        text = next
        echo.report(next)
        if (own != null) {
            ui.dispatch(Event.SearchChanged(entry.key, next))
        } else if (field != null) {
            ui.dispatch(Event.FieldChanged(entry.key, formKey, field.name, FieldValue.Text(next)))
        }
    }
    // Typing pauses, then the query is asked for.
    LaunchedEffect(text) {
        if (typed != null && text != applied) {
            delay(350)
            run()
        }
    }

    Surface(color = MaterialTheme.colorScheme.surface) {
        Box(Modifier.fillMaxWidth().padding(bottom = 8.dp), contentAlignment = Alignment.TopCenter) {
            SearchBar(
                inputField = {
                    SearchBarDefaults.InputField(
                        query = text,
                        onQueryChange = ::type,
                        onSearch = {
                            run()
                            focus.clearFocus()
                        },
                        expanded = false,
                        onExpandedChange = {},
                        placeholder = { Text(hint ?: stringResource(R.string.navree_search), maxLines = 1, overflow = TextOverflow.Ellipsis) },
                        leadingIcon = {
                            when {
                                temporary -> IconButton(
                                    onClick = {
                                        type("")
                                        run()
                                        leave()
                                    },
                                ) { Icon(Glyphs.Back, contentDescription = stringResource(R.string.navree_back)) }
                                plan.navigationIcon == NavigationIcon.Menu -> NavigationIconView(NavigationIcon.Menu, entry, info)
                                else -> Icon(Glyphs.Search, contentDescription = null)
                            }
                        },
                        trailingIcon = {
                            androidx.compose.foundation.layout.Row(verticalAlignment = Alignment.CenterVertically) {
                                if (text.isNotEmpty()) {
                                    IconButton(onClick = { type(""); run() }) {
                                        Icon(Glyphs.Close, contentDescription = stringResource(R.string.navree_clear))
                                    }
                                } else if (!temporary) {
                                    for (action in plan.barActions) {
                                        val node = screen.actions.getOrNull(action.index)
                                        if (node is ButtonNode) {
                                            IconButton(onClick = { ui.press(place, node) }, enabled = usable(node, place)) {
                                                Icon(rememberVector(node.icon) ?: Glyphs.More, contentDescription = node.label)
                                            }
                                        } else if (node is MenuNode) {
                                            MenuButtonView(node, place)
                                        }
                                    }
                                    if (plan.overflow.isNotEmpty()) Overflow(plan.overflow.mapNotNull { screen.actions.getOrNull(it) }, place)
                                }
                                val user = info.user
                                if (plan.avatar && user != null && !temporary) AccountMenu(user, place)
                            }
                        },
                    )
                },
                expanded = false,
                onExpandedChange = {},
                modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp),
            ) {}
        }
    }
}


/**
 * A table's filters as a row of chips over its rows, the way Android's own
 * apps offer them: a chip a filter, lit when it is in force, opening a menu
 * of its choices. Choosing asks for the rows again at once.
 */
@Composable
private fun FilterBar(form: FormNode, place: Place) {
    val ui = LocalUi.current
    val key = form.key
    val filters = form.fields().filter { it.input is ChoicesInput || it.input is ChoiceInput || it.input is ToggleInput }
    if (filters.isEmpty()) return
    val margin = ContentRules.marginDp(ui.window).dp
    fun put(field: FieldNode, value: FieldValue) {
        ui.dispatch(Event.FieldChanged(place.key, key, field.name, value))
        ui.dispatch(Event.Acted(place.key, Submit(form = key)))
    }
    LazyRow(
        contentPadding = PaddingValues(horizontal = margin),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
        modifier = Modifier.fillMaxWidth().padding(bottom = 4.dp),
    ) {
        items(filters.size, key = { filters[it].name }) { index ->
            val field = filters[index]
            val label = field.label ?: field.name
            var open by remember { mutableStateOf(false) }
            when (val input = field.input) {
                is ChoicesInput -> {
                    val chosen = (place.entry.local.value(key, field.name) as? FieldValue.Many)?.values ?: input.values
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
                                val value = option.value ?: continue
                                val on = value in chosen
                                DropdownMenuItem(
                                    text = { Text(option.label) },
                                    leadingIcon = {
                                        if (on) Icon(Glyphs.Check, contentDescription = null) else Spacer(Modifier.size(24.dp))
                                    },
                                    trailingIcon = option.detail?.let { detail -> { Text(detail, style = MaterialTheme.typography.labelMedium) } },
                                    onClick = {
                                        val next = if (on) chosen - value else chosen + value
                                        put(field, FieldValue.Many(input.options.mapNotNull { it.value }.filter { it in next }))
                                    },
                                    modifier = Modifier.semantics { selected = on },
                                )
                            }
                        }
                    }
                }
                is ChoiceInput -> {
                    val value = (place.entry.local.value(key, field.name) as? FieldValue.Text)?.text ?: input.value
                    val named = input.options.firstOrNull { it.value == value }
                    Box {
                        FilterChip(
                            selected = false,
                            onClick = { open = true },
                            label = { Text(if (named != null) "$label: ${named.label}" else label) },
                            trailingIcon = { Icon(Glyphs.ExpandMore, contentDescription = null, modifier = Modifier.size(18.dp)) },
                        )
                        DropdownMenu(expanded = open, onDismissRequest = { open = false }) {
                            for (option in input.options) {
                                DropdownMenuItem(
                                    text = { Text(option.label) },
                                    leadingIcon = {
                                        if (option.value == value) Icon(Glyphs.Check, contentDescription = null) else Spacer(Modifier.size(24.dp))
                                    },
                                    onClick = {
                                        open = false
                                        put(field, FieldValue.Text(option.value ?: ""))
                                    },
                                )
                            }
                        }
                    }
                }
                is ToggleInput -> {
                    val on = (place.entry.local.value(key, field.name) as? FieldValue.Flag)?.on ?: input.on
                    FilterChip(
                        selected = on,
                        onClick = { put(field, FieldValue.Flag(!on)) },
                        label = { Text(label) },
                        leadingIcon = if (on) {
                            { Icon(Glyphs.Check, contentDescription = null, modifier = Modifier.size(18.dp)) }
                        } else {
                            null
                        },
                    )
                }
                else -> Unit
            }
        }
    }
}
