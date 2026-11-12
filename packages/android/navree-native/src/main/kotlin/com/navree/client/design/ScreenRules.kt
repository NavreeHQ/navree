package com.navree.client.design

import com.navree.client.protocol.ActionRole
import com.navree.client.protocol.ButtonNode
import com.navree.client.protocol.ChoiceInput
import com.navree.client.protocol.ChoicesInput
import com.navree.client.protocol.FieldNode
import com.navree.client.protocol.FormNode
import com.navree.client.protocol.ListNode
import com.navree.client.protocol.MenuNode
import com.navree.client.protocol.Node
import com.navree.client.protocol.RegionNode
import com.navree.client.protocol.Screen
import com.navree.client.protocol.StackNode
import com.navree.client.protocol.TableNode
import com.navree.client.protocol.TextInput
import com.navree.client.protocol.TextKind
import com.navree.client.protocol.ToggleInput
import com.navree.client.runtime.fields
import com.navree.client.runtime.submitButton

/** How a screen is on show. */
enum class Presentation {
    /** One of a stack of screens. */
    Stack,

    /** A full-screen dialog: a task of its own, closed with an X, finished from the bar. */
    FullScreenDialog,

    /** A basic dialog in the middle of a wide window. */
    Dialog,

    /** A modal bottom sheet. */
    BottomSheet,
}

/** The app bar a screen gets. */
enum class BarKind {
    /** No bar: the screen has no chrome. */
    None,

    /** A small top app bar with the title on one line. */
    Small,

    /** A small bar with the subtitle on a line of its own under the title. */
    TwoLine,

    /** A search bar in the bar's place. The destination's label names the screen. */
    Search,
}

/** What the bar does when the content scrolls. */
enum class BarScroll {
    /** Stays, and takes the scrolled colour once content is under it. */
    Pinned,

    /** Leaves with the content as it scrolls down and comes back on the first scroll up. */
    EnterAlways,
}

enum class NavigationIcon { None, Back, Close, Menu }

enum class FabPlace {
    /** The bottom end corner of the content, above the navigation bar. */
    BottomEnd,

    /** The head of the navigation rail. */
    RailHeader,
}

enum class SubmitPlace {
    /** The screen is not a form. */
    None,

    /** A text button at the end of the top app bar, where the keyboard cannot cover it. */
    Bar,

    /** A filled button under the last field. */
    Inline,
}

enum class SearchPlace {
    None,

    /** The search bar is the app bar. */
    Bar,

    /** An icon in the app bar that turns the bar into a search field. */
    Action,
}

/**
 * The query of a screen's main table, taken apart: its search box goes to the
 * app bar and its filters become a row of chips over the rows.
 */
data class QueryPlan(
    val form: FormNode,
    val search: FieldNode?,
    val filters: List<FieldNode>,
)

/** How an action in the bar is drawn. */
enum class BarActionStyle {
    /** An icon button; the label is its accessibility name and its tooltip. */
    Icon,

    /** A text button, for the one primary action that has no icon. */
    Text,
}

data class BarAction(val index: Int, val style: BarActionStyle)

/** What surrounds the screen: which rules apply depends on it. */
data class ScreenContext(
    /** The first screen of a top-level destination. */
    val root: Boolean = false,
    val presentation: Presentation = Presentation.Stack,
    /** Whether the screen is drawn with the app's chrome. */
    val chrome: Boolean = true,
    val navigation: NavigationKind = NavigationKind.Bar,
    /** Whether first screens carry the menu button (see [NavigationPlan.menuButton]). */
    val menuButton: Boolean = false,
    /** Whether the shell names a signed-in user. */
    val user: Boolean = false,
    /** Whether there is a screen under this one to go back to. */
    val canGoBack: Boolean = false,
    /** Drawn as the second pane, beside the list it was opened from. */
    val detailPane: Boolean = false,
)

/** Where everything on a screen's frame goes. Action numbers index `Screen.actions`. */
data class ScreenPlan(
    val bar: BarKind,
    val scroll: BarScroll,
    val navigationIcon: NavigationIcon,
    /** The creation action drawn as a floating action button. */
    val fab: Int?,
    val fabPlace: FabPlace,
    /** Whether the floating action button carries its label beside its icon. */
    val fabExtended: Boolean,
    /** The actions drawn in the bar, in order. */
    val barActions: List<BarAction>,
    /** The actions in the overflow menu, in order. Destructive ones come last, after a divider. */
    val overflow: List<Int>,
    val submit: SubmitPlace,
    val search: SearchPlace,
    /** The user's avatar at the end of the bar. */
    val avatar: Boolean,
    val pullToRefresh: Boolean,
    /** Whether the search is the screen's own, or the search box of its main table's query. */
    val searchInQuery: Boolean = false,
)

/**
 * Where the parts of a screen's frame go on Android. The tree says what each
 * action is; this says where it is drawn.
 *
 * - **Creation** is the floating action button: the first `create` action of a
 *   screen in a stack. At a first screen it carries its label (and gives it up
 *   when the list scrolls or the text is at its largest); deeper it is the icon
 *   alone. Beside a navigation rail it moves to the rail's head.
 * - **Primary, then secondary** actions go in the top app bar, as icon buttons,
 *   in the order primary first. A compact window shows two, a medium one three,
 *   wider ones four; one fewer at the largest text sizes. When they do not all
 *   fit, the last place goes to the overflow menu and the rest go into it.
 *   An action with no icon goes into the overflow, except one primary action,
 *   which may be a text button.
 * - **Destructive** actions are never in the bar. They close the overflow
 *   menu, under a divider, and ask before they run.
 * - **A form** that is the whole screen is finished from the bar: its submit
 *   is a text button at the bar's end, where the keyboard cannot cover it. A
 *   form on a screen with no chrome, or among other content, keeps its button
 *   under the last field.
 * - **Presented** screens are full-screen dialogs: an X, the title, the submit.
 *   They have no floating action button and no avatar.
 * - **Search** on a first screen is a search bar in the app bar's place. On a
 *   deeper screen it is an icon that turns the bar into a search field.
 * - **The title** sits in a small bar that stays put, with the subtitle on a
 *   line under it when the screen has one and there is room. In a short
 *   window the bar leaves as the content scrolls and comes back on the first
 *   scroll up.
 */
object ScreenRules {
    fun plan(screen: Screen, context: ScreenContext, window: WindowInfo): ScreenPlan {
        val modal = context.presentation != Presentation.Stack
        val wholeForm = screen.body is FormNode
        val query = if (screen.search == null) query(screen) else null
        val searchable = screen.search != null || query?.search != null
        val submit = when {
            !wholeForm || (screen.body as FormNode).submitButton() == null -> SubmitPlace.None
            !context.chrome -> SubmitPlace.Inline
            context.presentation == Presentation.BottomSheet || context.presentation == Presentation.Dialog -> SubmitPlace.Inline
            else -> SubmitPlace.Bar
        }

        val searchPlace = when {
            !searchable || !context.chrome || modal -> SearchPlace.None
            context.root && !context.detailPane -> SearchPlace.Bar
            else -> SearchPlace.Action
        }

        val bar = when {
            !context.chrome -> BarKind.None
            searchPlace == SearchPlace.Bar -> BarKind.Search
            screen.subtitle != null && window.height != HeightClass.Compact && !window.largestText -> BarKind.TwoLine
            else -> BarKind.Small
        }

        val navigationIcon = when {
            !context.chrome -> NavigationIcon.None
            // A presented screen is closed with an X; a screen pushed inside it goes back with an arrow.
            modal -> if (context.canGoBack) NavigationIcon.Back else NavigationIcon.Close
            context.detailPane && !context.canGoBack -> NavigationIcon.None
            context.root -> if (context.menuButton) NavigationIcon.Menu else NavigationIcon.None
            context.canGoBack || screen.back?.href != null -> NavigationIcon.Back
            else -> NavigationIcon.None
        }

        // The floating action button.
        val fab = if (modal || !context.chrome) null else screen.actions.indexOfFirst { role(it) == ActionRole.Create }.takeIf { it >= 0 }
        val beside = context.navigation == NavigationKind.Rail || context.navigation == NavigationKind.ExpandedRail
        val fabPlace = if (fab != null && beside && context.root && !context.detailPane) FabPlace.RailHeader else FabPlace.BottomEnd
        val fabExtended = when {
            fab == null -> false
            fabPlace == FabPlace.RailHeader -> context.navigation == NavigationKind.ExpandedRail
            else -> context.root && !window.largestText
        }

        // The rest, in the order they are offered a place: primary first, destructive last.
        val rest = screen.actions.indices.filter { it != fab && usable(screen.actions[it]) }
        val cancelIsTheX = modal
        val candidates = rest.filter { index ->
            val role = role(screen.actions[index])
            role != ActionRole.Destructive && !(cancelIsTheX && role == ActionRole.Cancel)
        }.sortedBy { index -> if (isPrimary(role(screen.actions[index]))) 0 else 1 }
        val destructive = rest.filter { role(screen.actions[it]) == ActionRole.Destructive }

        var places = when (window.width) {
            WidthClass.Compact -> 2
            WidthClass.Medium -> 3
            else -> 4
        }
        if (window.largestText) places -= 1
        if (searchPlace == SearchPlace.Action) places -= 1
        if (submit == SubmitPlace.Bar) places -= 1
        // A search bar holds two things after the field: the avatar takes one of them.
        val avatar = context.root && context.user && context.chrome && !modal && !context.detailPane
        if (bar == BarKind.Search) places = if (avatar) 1 else 2
        places = places.coerceAtLeast(0)

        val withIcon = candidates.filter { hasIcon(screen.actions[it]) }
        val textPrimary = candidates.firstOrNull { !hasIcon(screen.actions[it]) && isPrimary(role(screen.actions[it])) && screen.actions[it] is ButtonNode }
            ?.takeIf { bar != BarKind.Search && submit != SubmitPlace.Bar }
        val eligible = candidates.filter { it in withIcon || it == textPrimary }
        val neverInBar = candidates.filter { it !in eligible }
        val needsOverflow = neverInBar.isNotEmpty() || destructive.isNotEmpty() || eligible.size > places
        val inBar = if (needsOverflow) eligible.take((places - 1).coerceAtLeast(0)) else eligible
        val overflow = (candidates.filter { it !in inBar }.sorted()) + destructive

        return ScreenPlan(
            bar = bar,
            scroll = if (window.height == HeightClass.Compact && !modal && bar != BarKind.Search) BarScroll.EnterAlways else BarScroll.Pinned,
            navigationIcon = navigationIcon,
            fab = fab,
            fabPlace = fabPlace,
            fabExtended = fabExtended,
            barActions = inBar.map { BarAction(it, if (it == textPrimary) BarActionStyle.Text else BarActionStyle.Icon) },
            overflow = overflow,
            submit = submit,
            search = searchPlace,
            avatar = avatar,
            pullToRefresh = screen.refresh && context.presentation == Presentation.Stack,
            searchInQuery = searchPlace != SearchPlace.None && screen.search == null,
        )
    }

    /** How a route's `sheet` or `full` is shown: a task of its own, full screen where space is short, a dialog where it is not. */
    fun presentation(full: Boolean, window: WindowInfo): Presentation = when {
        full -> Presentation.FullScreenDialog
        window.width == WidthClass.Compact || window.height == HeightClass.Compact -> Presentation.FullScreenDialog
        else -> Presentation.Dialog
    }

    /**
     * Whether a list and the screen opened from it are drawn side by side:
     * the window is at least expanded and the first screen of the stack is a
     * collection. A wide window is two panes, not one stretched phone screen.
     */
    fun twoPanes(window: WindowInfo, first: Screen?, depth: Int): Boolean =
        depth >= 2 && window.width >= WidthClass.Expanded && window.height != HeightClass.Compact && isCollection(first?.body)

    /** The width of the list pane, in dp, when there are two. */
    fun listPaneDp(window: WindowInfo): Int = if (window.width >= WidthClass.Large) 412 else 360

    /** Whether the body is, in the main, a list of things to open. */
    fun isCollection(body: Node?): Boolean = when (body) {
        is ListNode, is TableNode -> true
        is RegionNode -> body.children.any(::isCollection)
        is StackNode -> body.children.any { it is ListNode || it is TableNode || (it is RegionNode && isCollection(it)) } &&
            body.children.none { it is FormNode }
        else -> false
    }

    /** The screen's main table: the body itself, or the first table among the body's own parts, through regions. */
    fun mainTable(body: Node?): TableNode? = when (body) {
        is TableNode -> body
        is RegionNode -> body.children.firstNotNullOfOrNull(::mainTable)
        is StackNode -> body.children.firstNotNullOfOrNull { if (it is StackNode) null else mainTable(it) }
        else -> null
    }

    /**
     * The query of the screen's main table, when it has one: the search box
     * (a text field of the search kind) and the filters (choices, switches
     * and dates). A text field that is not a search stays where it is.
     */
    fun query(screen: Screen): QueryPlan? {
        val form = mainTable(screen.body)?.query ?: return null
        val fields = form.fields()
        val search = fields.firstOrNull { (it.input as? TextInput)?.kind == TextKind.Search }
        val filters = fields.filter { it !== search && (it.input is ChoicesInput || it.input is ChoiceInput || it.input is ToggleInput) }
        return QueryPlan(form, search, filters)
    }

    private fun role(node: Node): ActionRole = (node as? ButtonNode)?.role ?: ActionRole.None

    private fun isPrimary(role: ActionRole): Boolean = role == ActionRole.Primary || role == ActionRole.Confirm

    private fun usable(node: Node): Boolean = node is ButtonNode || node is MenuNode

    private fun hasIcon(node: Node): Boolean = when (node) {
        is ButtonNode -> node.icon != null
        is MenuNode -> node.icon != null
        else -> false
    }
}
