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
import androidx.compose.foundation.background
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.layout.Arrangement
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
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.only
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Badge
import androidx.compose.material3.BadgedBox
import androidx.compose.material3.Button
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.ModalWideNavigationRail
import androidx.compose.material3.rememberModalBottomSheetState
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
import androidx.compose.material3.WideNavigationRail
import androidx.compose.material3.WideNavigationRailItem
import androidx.compose.material3.WideNavigationRailState
import androidx.compose.material3.WideNavigationRailValue
import androidx.compose.material3.rememberWideNavigationRailState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.LaunchedEffect
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
import androidx.compose.ui.platform.LocalView
import androidx.compose.runtime.SideEffect
import androidx.core.view.WindowCompat
import android.app.Activity
import android.content.ContextWrapper
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.LayoutDirection
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Dialog
import com.navree.client.NavreeConfig
import com.navree.client.R
import com.navree.client.core.Destination
import com.navree.client.core.Driver
import com.navree.client.core.Events
import com.navree.client.core.Modal
import com.navree.client.core.NavPart
import com.navree.client.core.ScreenView
import com.navree.client.core.Stage
import com.navree.client.design.FabPlace
import com.navree.client.design.NavigationKind
import com.navree.client.design.NavigationPlan
import com.navree.client.design.NavigationRules
import com.navree.client.design.Presentation
import com.navree.client.design.ScreenContext
import com.navree.client.design.ScreenRules
import com.navree.client.design.Symbols
import com.navree.client.design.WidthClass
import com.navree.client.theme.Hct
import com.navree.client.theme.NavreeTheme
import com.navree.protocol.Navree
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.launch

/** The look a fresh app has before it knows its brand: Material's own baseline seed. */
private const val BASELINE_SEED = 0xFF6750A4.toInt()

/**
 * The whole app: the theme, the navigation the window calls for, the stack of
 * the destination in front, whatever is presented over it, and the passing
 * messages. Everything it shows comes from the core's view state.
 *
 * The palette grows from the brand's accent (the shell's, or the app's own
 * seed). The person's wallpaper colours are used when the app sets no accent
 * at all, or when it opts in.
 */
@Composable
fun NavreeApp(driver: Driver, config: NavreeConfig, image: suspend (String) -> ByteArray? = { null }) {
    val app = driver.app
    val nav = driver.nav
    val dark = isSystemInDarkTheme()
    val accent = app?.theme?.let { theme -> if (dark) theme.accentDark ?: theme.accent else theme.accent }
    val brand = Hct.parse(accent) ?: config.seed
    val dynamic = config.dynamicColour || brand == null
    NavreeTheme(seed = brand ?: BASELINE_SEED, dynamic = dynamic) {
        SystemBars(dark)
        val window = rememberWindow()
        val snackbars = remember { SnackbarHostState() }
        val ui = remember(driver, window) { Ui(driver, window, image, snackbars) }
        CompositionLocalProvider(LocalUi provides ui) {
            Surface(Modifier.fillMaxSize(), color = MaterialTheme.colorScheme.surface) {
                when (nav.stage) {
                    Stage.Ready -> Frame(nav, app?.name?.ifEmpty { null } ?: config.name, app?.user)
                    Stage.Upgrade -> UpgradeScreen()
                    Stage.Failed -> Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                        FailureState(nav.failure ?: com.navree.client.core.Failure.Server) { ui.send(Events.start) }
                    }
                    Stage.Starting -> Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) { PatientProgress() }
                }
            }
            Messages(driver, snackbars)
            ConfirmDialog()
        }
    }
}

/**
 * The system bars' icons follow the theme: dark icons over the light theme's
 * surfaces, light ones over the dark theme's. The app draws behind both bars.
 */
@Composable
private fun SystemBars(dark: Boolean) {
    val view = LocalView.current
    if (view.isInEditMode) return
    SideEffect {
        val activity = generateSequence(view.context) { (it as? ContextWrapper)?.baseContext }.filterIsInstance<Activity>().firstOrNull() ?: return@SideEffect
        WindowCompat.getInsetsController(activity.window, view).apply {
            isAppearanceLightStatusBars = !dark
            isAppearanceLightNavigationBars = !dark
        }
    }
}

/** Shows the waiting messages one at a time as snackbars, and tells the core what became of each. */
@Composable
private fun Messages(driver: Driver, host: SnackbarHostState) {
    val ui = LocalUi.current
    val message = driver.messages.firstOrNull() ?: return
    val notice = message.notice?.let { noticeText(it) }
    val text = listOfNotNull(message.title, message.text ?: notice).joinToString("\n")
    LaunchedEffect(message.id) {
        val result = host.showSnackbar(
            message = text,
            actionLabel = message.action,
            duration = if (message.action != null) SnackbarDuration.Long else SnackbarDuration.Short,
        )
        ui.send(if (result == SnackbarResult.ActionPerformed) Events.messageAction(message.id) else Events.messageShown(message.id))
    }
}

@Composable
private fun UpgradeScreen() {
    val ui = LocalUi.current
    Column(
        Modifier.fillMaxSize().padding(32.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center,
    ) {
        Text(stringResource(R.string.navree_upgrade_title), style = MaterialTheme.typography.headlineSmall, textAlign = TextAlign.Center, modifier = Modifier.semantics { heading() })
        Text(
            stringResource(R.string.navree_upgrade_text),
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            textAlign = TextAlign.Center,
            modifier = Modifier.padding(top = 8.dp, bottom = 24.dp),
        )
        Button(onClick = { ui.send(Events.openUpgrade) }) { Text(stringResource(R.string.navree_upgrade_action)) }
    }
}

/** The groups of destinations, in order, as the navigation rules count them. */
private fun groups(destinations: List<Destination>): List<Int> {
    val out = ArrayList<Int>()
    var last: String? = null
    for ((index, destination) in destinations.withIndex()) {
        if (index == 0 || destination.group != last) out += 0
        out[out.lastIndex] += 1
        last = destination.group
    }
    return out
}

/** The navigation, the destination in front, and what is presented over them. */
@Composable
private fun Frame(nav: NavPart, name: String, user: Navree.User?) {
    val ui = LocalUi.current
    val window = ui.window
    val screens = ui.driver.screens
    val plan = remember(window, nav.destinations) { NavigationRules.plan(window, groups(nav.destinations)) }
    val holder = rememberSaveableStateHolder()
    val destination = nav.destinations.getOrNull(nav.selected)
    val frontOfTab = destination?.stack?.lastOrNull()?.let(screens::get)
    val kind = if (frontOfTab?.chrome == false) NavigationKind.None else plan.kind
    val covers = nav.modals.map { ScreenRules.presentation(it.present, window) }
    val covered = covers.any { it == Presentation.FullScreenDialog }
    val menu = rememberWideNavigationRailState()
    val scope = rememberCoroutineScope()

    // Back belongs to the app while there is somewhere in it to go back to.
    // Otherwise the system takes it and plays its own way home.
    BackHandler(enabled = nav.canGoBack) { ui.send(Events.back) }

    // The floating action button of the first screen moves to the rail's head.
    val root = destination?.stack?.firstOrNull()?.let(screens::get)
    val rootScreen = root?.screen
    val twoPanes = ScreenRules.twoPanes(window, rootScreen, destination?.stack?.size ?: 0)
    val rootContext = context(plan, user, root, index = 0, presentation = Presentation.Stack, detailPane = false)
    val rootPlan = if (root != null && rootScreen != null) {
        val (arranged, bars) = arrange(rootScreen, root, rootContext)
        ScreenRules.plan(arranged, rootContext.copy(toolbar = bars.isNotEmpty()), window)
    } else {
        null
    }
    val railFab = rootPlan != null && rootPlan.fab != null && rootPlan.fabPlace == FabPlace.RailHeader && (destination?.stack?.size == 1 || twoPanes)

    CompositionLocalProvider(LocalOpenMenu provides { scope.launch { menu.expand() } }) {
        Box(Modifier.fillMaxSize()) {
            // Under a full-screen cover the app is still drawn, but it is not there for TalkBack.
            Box(Modifier.fillMaxSize().then(if (covered) Modifier.clearAndSetSemantics {} else Modifier)) {
                val stack: @Composable () -> Unit = {
                    if (destination != null) {
                        StackHost(nav, user, name, plan, destination.stack, group = nav.selected, Presentation.Stack, holder, active = nav.modals.isEmpty())
                    }
                }
                when (kind) {
                    NavigationKind.None -> stack()
                    NavigationKind.Bar -> Scaffold(
                        bottomBar = { BottomBar(nav.destinations, plan, nav.selected) { ui.send(Events.select(it)) } },
                        contentWindowInsets = WindowInsets(0, 0, 0, 0),
                    ) { padding ->
                        Box(Modifier.fillMaxSize().padding(padding).consumeWindowInsets(padding)) { stack() }
                    }
                    NavigationKind.Rail, NavigationKind.ExpandedRail -> Row(Modifier.fillMaxSize()) {
                        SideRail(
                            destinations = nav.destinations,
                            plan = plan,
                            selected = nav.selected,
                            expanded = kind == NavigationKind.ExpandedRail,
                            fab = if (railFab && root != null && rootPlan != null) ({ RailFab(root, rootPlan) }) else null,
                        ) { ui.send(Events.select(it)) }
                        Box(Modifier.weight(1f).fillMaxHeight().consumeWindowInsets(WindowInsets.safeDrawing.only(WindowInsetsSides.Start))) { stack() }
                    }
                }
            }
            Covers(nav, user, name, plan, covers, holder)
            if (plan.menuButton) {
                AllDestinations(nav.destinations, nav.selected, menu) { index ->
                    scope.launch { menu.collapse() }
                    ui.send(Events.select(index))
                }
            }
        }
    }

    // What a screen remembered (how far it was scrolled) is dropped once the screen is gone.
    val live = nav.held()
    val seen = remember { HashSet<Long>() }
    LaunchedEffect(live) {
        for (key in seen - live) holder.removeState(key)
        seen.clear()
        seen.addAll(live)
    }
}

private fun context(plan: NavigationPlan, user: Navree.User?, screen: ScreenView?, index: Int, presentation: Presentation, detailPane: Boolean): ScreenContext =
    ScreenContext(
        root = index == 0 && presentation == Presentation.Stack,
        presentation = presentation,
        chrome = screen?.chrome ?: true,
        navigation = plan.kind,
        menuButton = plan.menuButton,
        user = user != null,
        // The first screen of the detail pane has the list beside it, not under it.
        canGoBack = if (detailPane) index > 1 else index > 0,
        detailPane = detailPane,
    )

/** A destination's icon: the system's symbol when its name maps to one, filled when selected; a count beside it. */
@Composable
private fun NavIcon(destination: Destination, selected: Boolean) {
    // A destination the page gave no icon still takes one: Material's navigation always shows an icon.
    val vector = rememberIcon(destination.icon, selected = selected)
        ?: androidx.compose.runtime.remember(destination.label, selected) { symbolVector(Symbols.forLabel(destination.label), selected) }
    val badge = destination.badge
    BadgedBox(badge = { if (!badge.isNullOrEmpty()) Badge { Text(badge) } }) {
        if (vector != null) Icon(vector, contentDescription = null) else Spacer(Modifier.size(24.dp))
    }
}

@Composable
private fun NavLabel(text: String, plan: NavigationPlan) {
    // A label is never cut: at the largest text it takes a second line.
    Text(text, maxLines = plan.labelLines, overflow = TextOverflow.Ellipsis, textAlign = TextAlign.Center)
}

@Composable
private fun BottomBar(destinations: List<Destination>, plan: NavigationPlan, selected: Int, choose: (Int) -> Unit) {
    val window = LocalUi.current.window
    // In a wider window the bar's items sit side by side with their icons, in the middle.
    val beside = window.width >= WidthClass.Medium
    ShortNavigationBar(arrangement = if (beside) ShortNavigationBarArrangement.Centered else ShortNavigationBarArrangement.EqualWeight) {
        for (index in plan.shown) {
            val destination = destinations.getOrNull(index) ?: continue
            ShortNavigationBarItem(
                selected = index == selected,
                onClick = { choose(index) },
                icon = { NavIcon(destination, index == selected) },
                label = { NavLabel(destination.label, plan) },
                iconPosition = if (beside) NavigationItemIconPosition.Start else NavigationItemIconPosition.Top,
            )
        }
    }
}

@Composable
private fun SideRail(
    destinations: List<Destination>,
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
                TipIconButton(stringResource(R.string.navree_menu), Glyphs.Menu, { scope.launch { state.toggle() } }, Modifier.padding(start = 24.dp))
                if (fab != null) Box(Modifier.padding(start = 20.dp, top = 8.dp, bottom = 8.dp)) { fab() }
            }
        },
    ) {
        var last: String? = null
        destinations.forEachIndexed { index, destination ->
            val heading = destination.group
            if (heading != null && heading != last && open && plan.groupHeadings) {
                Text(
                    heading,
                    style = MaterialTheme.typography.titleSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.padding(start = 36.dp, top = 16.dp, bottom = 8.dp).semantics { heading() },
                )
            }
            last = heading
            if (index in plan.shown || open) {
                WideNavigationRailItem(
                    selected = index == selected,
                    onClick = { choose(index) },
                    icon = { NavIcon(destination, index == selected) },
                    label = { NavLabel(destination.label, plan) },
                    railExpanded = open,
                )
            }
        }
    }
}

/** Every destination, in its group, in the expanded rail drawn over the content. Opened from the menu button. */
@Composable
private fun AllDestinations(destinations: List<Destination>, selected: Int, state: WideNavigationRailState, choose: (Int) -> Unit) {
    ModalWideNavigationRail(state = state, hideOnCollapse = true) {
        var last: String? = null
        destinations.forEachIndexed { index, destination ->
            val heading = destination.group
            if (heading != null && heading != last) {
                Text(
                    heading,
                    style = MaterialTheme.typography.titleSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.padding(start = 36.dp, top = 16.dp, bottom = 8.dp).semantics { heading() },
                )
            }
            last = heading
            WideNavigationRailItem(
                selected = index == selected,
                onClick = { choose(index) },
                icon = { NavIcon(destination, index == selected) },
                label = { Text(destination.label, maxLines = 2, overflow = TextOverflow.Ellipsis) },
                railExpanded = true,
            )
        }
    }
}

/** What moves when the screen in front changes: which stack it is, and which screen. */
private data class Front(val group: Int, val id: Long)

/**
 * A stack of screens. In a wide window a list and what was opened from it are
 * two panes, each in a rounded container on the surface tone, as Android's
 * own list-detail layouts are; otherwise the screen in front is the only one
 * drawn. Going deeper slides in from the end, going back slides out to it,
 * and changing destination fades through. A back gesture shows the screen
 * under the one in front while the finger is still down.
 */
@Composable
private fun StackHost(
    nav: NavPart,
    user: Navree.User?,
    name: String,
    plan: NavigationPlan,
    stack: List<Long>,
    group: Int,
    presentation: Presentation,
    holder: SaveableStateHolder,
    active: Boolean,
) {
    val ui = LocalUi.current
    val window = ui.window
    val screens = stack.mapNotNull { ui.driver.screens[it] }
    if (screens.isEmpty()) {
        Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) { PatientProgress() }
        return
    }
    val two = presentation == Presentation.Stack && ScreenRules.twoPanes(window, screens.first().screen, screens.size)
    val front = nav.front

    @Composable
    fun host(screen: ScreenView, index: Int, detailPane: Boolean, selectedUrl: String? = null) {
        val context = context(plan, user, screen, index, presentation, detailPane)
        holder.SaveableStateProvider(screen.id) {
            ScreenHost(screen, HostInfo(context, user, name, front = screen.id == front, selectedUrl = selectedUrl))
        }
    }

    if (two) {
        Row(
            Modifier.fillMaxSize().background(MaterialTheme.colorScheme.surfaceContainer).padding(end = 12.dp, bottom = 12.dp),
            horizontalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Surface(shape = RoundedCornerShape(bottomEnd = 24.dp), modifier = Modifier.width(ScreenRules.listPaneDp(window).dp).fillMaxHeight()) {
                host(screens.first(), 0, detailPane = false, selectedUrl = screens[1].url)
            }
            Surface(
                shape = MaterialTheme.shapes.extraLarge,
                modifier = Modifier.weight(1f).fillMaxHeight().windowInsetsPadding(WindowInsets.safeDrawing.only(WindowInsetsSides.Top)).padding(top = 12.dp),
            ) {
                Moving(screens.drop(1), group, active, canPop = screens.size > 2) { screen, index -> host(screen, index + 1, detailPane = true) }
            }
        }
    } else {
        Moving(screens, group, active, canPop = screens.size > 1) { screen, index -> host(screen, index, detailPane = false) }
    }
}

@Composable
private fun Moving(
    screens: List<ScreenView>,
    group: Int,
    active: Boolean,
    canPop: Boolean,
    host: @Composable (ScreenView, Int) -> Unit,
) {
    val ui = LocalUi.current
    val top = screens.last()
    // The screen that is leaving is no longer in a stack, but it is still drawn while it leaves.
    val known = remember { HashMap<Long, Pair<ScreenView, Int>>() }
    screens.forEachIndexed { index, screen -> known[screen.id] = screen to index }
    if (known.size > 16) known.keys.retainAll(screens.map { it.id }.toSet())
    val rtl = LocalLayoutDirection.current == LayoutDirection.Rtl

    var pulled by remember { mutableFloatStateOf(0f) }
    PredictiveBackHandler(enabled = active && canPop) { events ->
        try {
            events.collect { pulled = it.progress }
            ui.send(Events.back)
        } catch (_: CancellationException) {
            // The gesture was let go before the edge: nothing happens.
        } finally {
            pulled = 0f
        }
    }

    Box(Modifier.fillMaxSize()) {
        if (pulled > 0f && screens.size > 1) host(screens[screens.size - 2], screens.size - 2)
        AnimatedContent(
            targetState = Front(group, top.id),
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
            val (screen, index) = known[shown.id]?.let { (old, at) -> (ui.driver.screens[old.id] ?: old) to at } ?: (top to screens.lastIndex)
            Surface(Modifier.fillMaxSize(), color = MaterialTheme.colorScheme.surface) { host(screen, index) }
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
        to.id > from.id -> (slideInHorizontally(tween(300)) { end * it / 8 } + fadeIn(fade)) togetherWith fadeOut(tween(120))
        // Back: the old screen leaves to the end.
        else -> fadeIn(fade) togetherWith (slideOutHorizontally(tween(300)) { end * it / 8 } + fadeOut(tween(160)))
    }
}

/** Screens presented over the app: full screen where the window is small, a dialog where it is not. */
@Composable
private fun Covers(
    nav: NavPart,
    user: Navree.User?,
    name: String,
    plan: NavigationPlan,
    presentations: List<Presentation>,
    holder: SaveableStateHolder,
) {
    val ui = LocalUi.current
    val window = ui.window
    val full = nav.modals.withIndex().lastOrNull { presentations.getOrNull(it.index) == Presentation.FullScreenDialog }
    val known = remember { HashMap<Long, Modal>() }
    full?.value?.stack?.firstOrNull()?.let { known[it] = full.value }

    AnimatedContent(
        targetState = full?.value?.stack?.firstOrNull(),
        transitionSpec = {
            if (targetState != null) {
                (slideInVertically(tween(300)) { it / 6 } + fadeIn(tween(200))) togetherWith fadeOut(tween(100))
            } else {
                fadeIn(tween(100)) togetherWith (slideOutVertically(tween(250)) { it / 6 } + fadeOut(tween(200)))
            }
        },
        label = "cover",
    ) { first ->
        val modal = first?.let { nav.modals.firstOrNull { modal -> modal.stack.firstOrNull() == it } ?: known[it] }
        if (modal != null) {
            // A cover the person cannot take away (the sign-in a session that ended is sent to) has no back of its own.
            BackHandler(enabled = !modal.locked && nav.modals.lastOrNull() == modal && modal.stack.size <= 1) { ui.send(Events.back) }
            Surface(Modifier.fillMaxSize(), color = MaterialTheme.colorScheme.surface) {
                StackHost(nav, user, name, plan, modal.stack, group = -1 - nav.modals.indexOf(modal), Presentation.FullScreenDialog, holder, active = nav.modals.lastOrNull() == modal)
            }
        }
    }

    nav.modals.forEachIndexed { index, modal ->
        if (presentations.getOrNull(index) == Presentation.Dialog) {
            val first = modal.stack.firstOrNull() ?: return@forEachIndexed
            Dialog(onDismissRequest = { if (!modal.locked) ui.send(Events.dismissed(first)) }) {
                Surface(
                    shape = MaterialTheme.shapes.extraLarge,
                    color = MaterialTheme.colorScheme.surfaceContainerHigh,
                    modifier = Modifier.widthIn(max = 560.dp).fillMaxWidth().heightIn(max = (window.heightDp * 0.9f).dp),
                ) {
                    StackHost(nav, user, name, plan, modal.stack, group = -1 - index, Presentation.Dialog, holder, active = index == nav.modals.lastIndex)
                }
            }
        } else if (presentations.getOrNull(index) == Presentation.BottomSheet) {
            // A glance on a phone: a modal bottom sheet, opened fully, with its own stack.
            val first = modal.stack.firstOrNull() ?: return@forEachIndexed
            val state = rememberModalBottomSheetState(skipPartiallyExpanded = true, confirmValueChange = { !modal.locked })
            ModalBottomSheet(
                onDismissRequest = { if (!modal.locked) ui.send(Events.dismissed(first)) },
                sheetState = state,
                sheetGesturesEnabled = !modal.locked,
            ) {
                Box(Modifier.fillMaxWidth().heightIn(max = (window.heightDp * 0.92f).dp)) {
                    StackHost(nav, user, name, plan, modal.stack, group = -1 - index, Presentation.BottomSheet, holder, active = index == nav.modals.lastIndex)
                }
            }
        }
    }
}
