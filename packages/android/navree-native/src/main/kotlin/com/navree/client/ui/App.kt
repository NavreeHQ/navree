package com.navree.client.ui

import androidx.activity.compose.BackHandler
import androidx.activity.compose.PredictiveBackHandler
import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.ContentTransform
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.slideInHorizontally
import androidx.compose.animation.slideInVertically
import androidx.compose.animation.slideOutHorizontally
import androidx.compose.animation.slideOutVertically
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.WindowInsetsSides
import androidx.compose.foundation.layout.consumeWindowInsets
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.only
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Badge
import androidx.compose.material3.BadgedBox
import androidx.compose.material3.Button
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalWideNavigationRail
import androidx.compose.material3.NavigationItemIconPosition
import androidx.compose.material3.Scaffold
import androidx.compose.material3.ShortNavigationBar
import androidx.compose.material3.ShortNavigationBarArrangement
import androidx.compose.material3.ShortNavigationBarItem
import androidx.compose.material3.SnackbarDuration
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.SnackbarResult
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.VerticalDivider
import androidx.compose.material3.WideNavigationRail
import androidx.compose.material3.WideNavigationRailItem
import androidx.compose.material3.WideNavigationRailValue
import androidx.compose.material3.rememberWideNavigationRailState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.SaveableStateHolder
import androidx.compose.runtime.saveable.rememberSaveableStateHolder
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.platform.LocalLayoutDirection
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.LayoutDirection
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Dialog
import com.navree.client.R
import com.navree.client.design.FabPlace
import com.navree.client.design.HeightClass
import com.navree.client.design.NavigationKind
import com.navree.client.design.NavigationPlan
import com.navree.client.design.NavigationRules
import com.navree.client.design.Presentation
import com.navree.client.design.ScreenContext
import com.navree.client.design.ScreenRules
import com.navree.client.design.WidthClass
import com.navree.client.platform.NavreeConfig
import com.navree.client.platform.NavreeStore
import com.navree.client.protocol.NavItem
import com.navree.client.protocol.Present
import com.navree.client.protocol.Shell
import com.navree.client.protocol.Upgrade
import com.navree.client.runtime.ClientState
import com.navree.client.runtime.Entry
import com.navree.client.runtime.Event
import com.navree.client.runtime.Load
import com.navree.client.runtime.Modal
import com.navree.client.theme.Hct
import com.navree.client.theme.NavreeTheme
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.launch

/**
 * The whole app: the theme, the navigation the window calls for, the stack of
 * the destination in front, whatever is presented over it, and the passing
 * messages. Everything it shows comes from the store's state.
 */
@Composable
fun NavreeApp(store: NavreeStore, config: NavreeConfig, image: suspend (String) -> ByteArray? = { null }) {
    val state by store.state.collectAsState()
    val shell = state.shell
    val dark = isSystemInDarkTheme()
    val accent = shell?.theme?.let { theme -> if (dark) theme.accentDark ?: theme.accent else theme.accent }
    val seed = remember(accent, config.seed) { Hct.parse(accent) ?: config.seed }
    val dynamic = config.dynamicColour && shell?.theme?.dynamic != false
    NavreeTheme(seed = seed, dynamic = dynamic, corner = shell?.theme?.corner) {
        val window = rememberWindow()
        val snackbars = remember { SnackbarHostState() }
        val ui = remember(store, window) { Ui(store::dispatch, window, store.origin, image, snackbars) }
        CompositionLocalProvider(LocalUi provides ui) {
            Surface(Modifier.fillMaxSize(), color = MaterialTheme.colorScheme.surface) {
                val upgrade = state.upgrade
                when {
                    upgrade != null -> UpgradeScreen(upgrade)
                    shell == null -> StartScreen(state.shellLoad)
                    else -> Frame(state, shell, config.name)
                }
            }
            Messages(state, snackbars)
        }
    }
}

/** Shows the waiting messages one at a time as snackbars, and reports what became of each. */
@Composable
private fun Messages(state: ClientState, host: SnackbarHostState) {
    val ui = LocalUi.current
    val message = state.messages.firstOrNull() ?: return
    val notice = message.notice?.let { failureShort(it) }
    val text = listOfNotNull(message.title, message.text ?: notice).joinToString("\n")
    LaunchedEffect(message.id) {
        val result = host.showSnackbar(
            message = text,
            actionLabel = message.action?.label,
            duration = if (message.action != null) SnackbarDuration.Long else SnackbarDuration.Short,
        )
        ui.dispatch(if (result == SnackbarResult.ActionPerformed) Event.MessageActed(message.id) else Event.MessageShown(message.id))
    }
}

@Composable
private fun StartScreen(load: Load) {
    val ui = LocalUi.current
    Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
        if (load is Load.Failed) {
            FailureState(load.failure) { ui.dispatch(Event.Started) }
        } else {
            PatientProgress()
        }
    }
}

@Composable
private fun UpgradeScreen(upgrade: Upgrade) {
    val ui = LocalUi.current
    Column(
        Modifier.fillMaxSize().padding(32.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = androidx.compose.foundation.layout.Arrangement.Center,
    ) {
        Text(
            stringResource(R.string.navree_upgrade_title),
            style = MaterialTheme.typography.headlineSmall,
            textAlign = TextAlign.Center,
            modifier = Modifier.semantics { heading() },
        )
        Text(
            stringResource(R.string.navree_upgrade_text),
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            textAlign = TextAlign.Center,
            modifier = Modifier.padding(top = 8.dp, bottom = 24.dp),
        )
        val url = upgrade.url
        if (url != null) {
            Button(onClick = { ui.dispatch(Event.Opened(url)) }) { Text(stringResource(R.string.navree_upgrade_action)) }
        }
    }
}

/** The navigation, the destination in front, and what is presented over them. */
@Composable
private fun Frame(state: ClientState, shell: Shell, name: String) {
    val ui = LocalUi.current
    val window = ui.window
    val plan = remember(window, shell.nav) { NavigationRules.plan(window, shell.nav) }
    val holder = rememberSaveableStateHolder()
    val tab = state.tabs.getOrNull(state.selected)
    val tabFront = tab?.stack?.lastOrNull()
    val kind = if (tabFront?.chrome == false) NavigationKind.None else plan.kind
    val covers = state.modals.map { ScreenRules.presentation(it.present == Present.Full, window) }
    val covered = covers.any { it == Presentation.FullScreenDialog }
    val menu = rememberWideNavigationRailState()
    val scope = rememberCoroutineScope()
    val items = state.tabs.map { it.item }
    val appName = shell.name.ifEmpty { name }

    // Back belongs to the app while there is somewhere in it to go back to.
    // Otherwise the system takes it and plays its own way home.
    BackHandler(enabled = state.canGoBack) { ui.dispatch(Event.BackPressed) }

    // The floating action button of the first screen moves to the rail's head.
    val root = tab?.stack?.firstOrNull()
    val rootScreen = root?.screen
    val twoPanes = ScreenRules.twoPanes(window, rootScreen, tab?.stack?.size ?: 0)
    val rootContext = context(shell, plan, root, index = 0, presentation = Presentation.Stack, detailPane = false)
    val rootPlan = if (rootScreen != null) ScreenRules.plan(rootScreen, rootContext, window) else null
    val railFab = rootPlan != null && rootPlan.fab != null && rootPlan.fabPlace == FabPlace.RailHeader &&
        (tab?.stack?.size == 1 || twoPanes)

    CompositionLocalProvider(LocalOpenMenu provides { scope.launch { menu.expand() } }) {
        Box(Modifier.fillMaxSize()) {
            // Under a full-screen cover the app is still drawn, but it is not there for TalkBack.
            Box(Modifier.fillMaxSize().then(if (covered) Modifier.clearAndSetSemantics {} else Modifier)) {
                val stack: @Composable () -> Unit = {
                    if (tab != null) {
                        StackHost(state, shell, appName, plan, tab.stack, group = state.selected, Presentation.Stack, holder, active = state.modals.isEmpty())
                    }
                }
                when (kind) {
                    NavigationKind.None -> stack()
                    NavigationKind.Bar -> Scaffold(
                        bottomBar = { BottomBar(items, plan, state.selected) { ui.dispatch(Event.DestinationChosen(it)) } },
                        contentWindowInsets = WindowInsets(0, 0, 0, 0),
                    ) { padding ->
                        Box(Modifier.fillMaxSize().padding(padding).consumeWindowInsets(padding)) { stack() }
                    }
                    NavigationKind.Rail, NavigationKind.ExpandedRail -> Row(Modifier.fillMaxSize()) {
                        SideRail(
                            shell = shell,
                            plan = plan,
                            selected = state.selected,
                            expanded = kind == NavigationKind.ExpandedRail,
                            fab = if (railFab && root != null && rootScreen != null && rootPlan != null) {
                                { RailFab(root, rootScreen, rootPlan) }
                            } else {
                                null
                            },
                        ) { ui.dispatch(Event.DestinationChosen(it)) }
                        Box(
                            Modifier.weight(1f).fillMaxHeight()
                                .consumeWindowInsets(WindowInsets.safeDrawing.only(WindowInsetsSides.Start)),
                        ) { stack() }
                    }
                }
            }
            Covers(state, shell, appName, plan, covers, holder)
            if (plan.menuButton) {
                AllDestinations(shell, state.selected, menu) { index ->
                    scope.launch { menu.collapse() }
                    ui.dispatch(Event.DestinationChosen(index))
                }
            }
        }
    }

    // What a screen remembered (how far it was scrolled) is dropped once the screen is gone.
    val live = state.entries().map { it.key }.toSet()
    val seen = remember { HashSet<Long>() }
    LaunchedEffect(live) {
        for (key in seen - live) holder.removeState(key)
        seen.clear()
        seen.addAll(live)
    }
}

private fun context(
    shell: Shell,
    plan: NavigationPlan,
    entry: Entry?,
    index: Int,
    presentation: Presentation,
    detailPane: Boolean,
): ScreenContext = ScreenContext(
    root = index == 0 && presentation == Presentation.Stack,
    presentation = presentation,
    chrome = entry?.chrome ?: true,
    navigation = plan.kind,
    menuButton = plan.menuButton,
    user = shell.user != null,
    // The first screen of the detail pane has the list beside it, not under it.
    canGoBack = if (detailPane) index > 1 else index > 0,
    detailPane = detailPane,
)

@Composable
private fun NavIcon(item: NavItem) {
    val vector = rememberVector(item.icon)
    val badge = item.badge
    BadgedBox(badge = { if (!badge.isNullOrEmpty()) Badge { Text(badge) } }) {
        if (vector != null) Icon(vector, contentDescription = null) else Spacer(Modifier.size(24.dp))
    }
}

@Composable
private fun BottomBar(items: List<NavItem>, plan: NavigationPlan, selected: Int, choose: (Int) -> Unit) {
    val window = LocalUi.current.window
    // In a wider window the bar's items sit side by side with their icons, in the middle.
    val beside = window.width >= WidthClass.Medium
    ShortNavigationBar(
        arrangement = if (beside) ShortNavigationBarArrangement.Centered else ShortNavigationBarArrangement.EqualWeight,
    ) {
        for (index in plan.shown) {
            val item = items.getOrNull(index) ?: continue
            ShortNavigationBarItem(
                selected = index == selected,
                onClick = { choose(index) },
                icon = { NavIcon(item) },
                label = { Text(item.label, maxLines = 1, overflow = TextOverflow.Ellipsis) },
                iconPosition = if (beside) NavigationItemIconPosition.Start else NavigationItemIconPosition.Top,
            )
        }
    }
}

@Composable
private fun SideRail(
    shell: Shell,
    plan: NavigationPlan,
    selected: Int,
    expanded: Boolean,
    fab: (@Composable () -> Unit)?,
    choose: (Int) -> Unit,
) {
    val state = rememberWideNavigationRailState(if (expanded) WideNavigationRailValue.Expanded else WideNavigationRailValue.Collapsed)
    val scope = rememberCoroutineScope()
    val open = state.targetValue == WideNavigationRailValue.Expanded
    WideNavigationRail(
        state = state,
        header = {
            Column(horizontalAlignment = Alignment.Start) {
                IconButton(onClick = { scope.launch { state.toggle() } }, modifier = Modifier.padding(start = 24.dp)) {
                    Icon(Glyphs.Menu, contentDescription = stringResource(R.string.navree_menu))
                }
                if (fab != null) {
                    Box(Modifier.padding(start = 20.dp, top = 8.dp, bottom = 8.dp)) { fab() }
                }
            }
        },
    ) {
        var index = 0
        for (group in shell.nav.groups) {
            val heading = group.label
            if (heading != null && open && plan.groupHeadings) {
                Text(
                    heading,
                    style = MaterialTheme.typography.titleSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.padding(start = 36.dp, top = 16.dp, bottom = 8.dp).semantics { heading() },
                )
            }
            for (item in group.items) {
                val at = index++
                if (at !in plan.shown && !open) continue
                WideNavigationRailItem(
                    selected = at == selected,
                    onClick = { choose(at) },
                    icon = { NavIcon(item) },
                    label = { Text(item.label, maxLines = 1, overflow = TextOverflow.Ellipsis) },
                    railExpanded = open,
                )
            }
        }
    }
}

/** Every destination, in its group, in the expanded rail drawn over the content. Opened from the menu button. */
@Composable
private fun AllDestinations(
    shell: Shell,
    selected: Int,
    state: androidx.compose.material3.WideNavigationRailState,
    choose: (Int) -> Unit,
) {
    ModalWideNavigationRail(state = state, hideOnCollapse = true) {
        var index = 0
        for (group in shell.nav.groups) {
            group.label?.let { heading ->
                Text(
                    heading,
                    style = MaterialTheme.typography.titleSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.padding(start = 36.dp, top = 16.dp, bottom = 8.dp).semantics { heading() },
                )
            }
            for (item in group.items) {
                val at = index++
                WideNavigationRailItem(
                    selected = at == selected,
                    onClick = { choose(at) },
                    icon = { NavIcon(item) },
                    label = { Text(item.label, maxLines = 1, overflow = TextOverflow.Ellipsis) },
                    railExpanded = true,
                )
            }
        }
    }
}

/** What moves when the screen in front changes: which stack it is, and which entry. */
private data class Front(val group: Int, val key: Long)

/**
 * A stack of screens. In a wide window a list and what was opened from it are
 * two panes; otherwise the screen in front is the only one drawn. Going
 * deeper slides in from the end, going back slides out to it, and changing
 * destination fades through. A back gesture shows the screen under the one in
 * front while the finger is still down.
 */
@Composable
private fun StackHost(
    state: ClientState,
    shell: Shell,
    name: String,
    plan: NavigationPlan,
    stack: List<Entry>,
    group: Int,
    presentation: Presentation,
    holder: SaveableStateHolder,
    active: Boolean,
) {
    val window = LocalUi.current.window
    if (stack.isEmpty()) {
        Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) { PatientProgress() }
        return
    }
    val two = presentation == Presentation.Stack && ScreenRules.twoPanes(window, stack.first().screen, stack.size)
    val front = state.front?.key

    @Composable
    fun host(entry: Entry, index: Int, detailPane: Boolean, selectedUrl: String? = null) {
        val context = context(shell, plan, entry, index, presentation, detailPane)
        holder.SaveableStateProvider(entry.key) {
            ScreenHost(entry, HostInfo(context, shell.user, name, front = entry.key == front, selectedUrl = selectedUrl))
        }
    }

    if (two) {
        Row(Modifier.fillMaxSize()) {
            Box(Modifier.width(ScreenRules.listPaneDp(window).dp).fillMaxHeight()) {
                host(stack.first(), 0, detailPane = false, selectedUrl = stack[1].url)
            }
            VerticalDivider()
            Box(Modifier.weight(1f).fillMaxHeight()) {
                Moving(stack.drop(1), group, active, canPop = stack.size > 2) { entry, index -> host(entry, index + 1, detailPane = true) }
            }
        }
    } else {
        Moving(stack, group, active, canPop = stack.size > 1) { entry, index -> host(entry, index, detailPane = false) }
    }
}

@Composable
private fun Moving(
    entries: List<Entry>,
    group: Int,
    active: Boolean,
    canPop: Boolean,
    host: @Composable (Entry, Int) -> Unit,
) {
    val ui = LocalUi.current
    val top = entries.last()
    // The entry that is leaving is no longer in the state, but it is still drawn while it leaves.
    val known = remember { HashMap<Long, Pair<Entry, Int>>() }
    entries.forEachIndexed { index, entry -> known[entry.key] = entry to index }
    if (known.size > 16) known.keys.retainAll(entries.map { it.key }.toSet())
    val rtl = LocalLayoutDirection.current == LayoutDirection.Rtl

    var pulled by remember { mutableFloatStateOf(0f) }
    PredictiveBackHandler(enabled = active && canPop) { events ->
        try {
            events.collect { pulled = it.progress }
            ui.dispatch(Event.BackPressed)
        } catch (_: CancellationException) {
            // The gesture was let go before the edge: nothing happens.
        } finally {
            pulled = 0f
        }
    }

    Box(Modifier.fillMaxSize()) {
        if (pulled > 0f && entries.size > 1) {
            val under = entries[entries.size - 2]
            host(under, entries.size - 2)
        }
        AnimatedContent(
            targetState = Front(group, top.key),
            transitionSpec = { transition(initialState, targetState, rtl) },
            modifier = Modifier.fillMaxSize().graphicsLayer {
                // The screen in front shrinks a little under the finger, as the system's own screens do.
                val scale = 1f - 0.1f * pulled
                scaleX = scale
                scaleY = scale
                shape = RoundedCornerShape((28f * pulled).dp)
                clip = pulled > 0f
            },
            label = "screens",
        ) { shown ->
            val (entry, index) = known[shown.key] ?: (top to entries.lastIndex)
            Surface(Modifier.fillMaxSize(), color = MaterialTheme.colorScheme.surface) { host(entry, index) }
        }
    }
}

private fun transition(from: Front, to: Front, rtl: Boolean): ContentTransform {
    val fade = tween<Float>(durationMillis = 250)
    // "The end" is the right edge, or the left one where text runs right to left.
    val end = if (rtl) -1 else 1
    return when {
        // Another destination: one fades out, the next fades in.
        from.group != to.group -> fadeIn(tween(200, delayMillis = 90)) togetherWith fadeOut(tween(90))
        // Deeper: the new screen comes from the end.
        to.key > from.key -> (slideInHorizontally(tween(300)) { end * it / 8 } + fadeIn(fade)) togetherWith fadeOut(tween(120))
        // Back: the old screen leaves to the end.
        else -> fadeIn(fade) togetherWith (slideOutHorizontally(tween(300)) { end * it / 8 } + fadeOut(tween(160)))
    }
}

/** Screens presented over the app: full screen where the window is small, a dialog where it is not. */
@Composable
private fun Covers(
    state: ClientState,
    shell: Shell,
    name: String,
    plan: NavigationPlan,
    presentations: List<Presentation>,
    holder: SaveableStateHolder,
) {
    val ui = LocalUi.current
    val window = ui.window
    val full = state.modals.withIndex().lastOrNull { presentations.getOrNull(it.index) == Presentation.FullScreenDialog }
    val known = remember { HashMap<Long, Modal>() }
    full?.let { known[it.value.stack.first().key] = it.value }

    AnimatedContent(
        targetState = full?.value?.stack?.first()?.key,
        transitionSpec = {
            if (targetState != null) {
                (slideInVertically(tween(300)) { it / 6 } + fadeIn(tween(200))) togetherWith fadeOut(tween(100))
            } else {
                fadeIn(tween(100)) togetherWith (slideOutVertically(tween(250)) { it / 6 } + fadeOut(tween(200)))
            }
        },
        label = "cover",
    ) { key ->
        val modal = key?.let { state.modals.firstOrNull { modal -> modal.stack.first().key == it } ?: known[it] }
        if (modal != null) {
            Surface(Modifier.fillMaxSize(), color = MaterialTheme.colorScheme.surface) {
                StackHost(
                    state, shell, name, plan, modal.stack, group = -1 - state.modals.indexOf(modal),
                    Presentation.FullScreenDialog, holder, active = state.modals.lastOrNull() == modal,
                )
            }
        }
    }

    state.modals.forEachIndexed { index, modal ->
        if (presentations.getOrNull(index) == Presentation.Dialog) {
            val first = modal.stack.first().key
            Dialog(onDismissRequest = { ui.dispatch(Event.ModalDismissed(first)) }) {
                Surface(
                    shape = MaterialTheme.shapes.extraLarge,
                    color = MaterialTheme.colorScheme.surfaceContainerHigh,
                    modifier = Modifier.widthIn(max = 560.dp).fillMaxWidth().heightIn(max = (window.heightDp * 0.9f).dp),
                ) {
                    StackHost(
                        state, shell, name, plan, modal.stack, group = -1 - index,
                        Presentation.Dialog, holder, active = index == state.modals.lastIndex,
                    )
                }
            }
        }
    }
}
