@file:OptIn(ExperimentalFoundationApi::class)

package com.navree.client.ui

import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectDragGestures
import androidx.compose.foundation.gestures.scrollBy
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.SecondaryScrollableTabRow
import androidx.compose.material3.Tab
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.SideEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.saveable.Saver
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.runtime.withFrameNanos
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.LocalPinnableContainer
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.CollectionInfo
import androidx.compose.ui.semantics.CollectionItemInfo
import androidx.compose.ui.semantics.CustomAccessibilityAction
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.collectionInfo
import androidx.compose.ui.semantics.collectionItemInfo
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.customActions
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.zIndex
import com.navree.client.R
import com.navree.client.core.Events
import com.navree.client.design.LaneRules
import com.navree.client.design.ReorderRules
import com.navree.client.design.nodeId
import com.navree.protocol.Navree
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.drop
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch

/**
 * A tap on what a drag does, for a test to write down frame by frame: it is told a line each time a row is
 * picked up, dragged, put in a place, scrolled for, let go or lost. Nothing in the app sets it.
 */
internal object LaneTrace {
    @Volatile
    var tap: ((String) -> Unit)? = null
}

/**
 * What a list of rows that are moved is while a row of it is in hand, and
 * after the drop until the core's next screen shows the move.
 *
 * The row in hand stays an item of the lazy list the lane is laid in, with the
 * key it always had. It follows the finger by the way it is drawn (a layer
 * translated by the difference between where the finger is and where the item
 * stands), and the lane's order is changed as the finger passes the rows beside
 * it, so the row in hand is always one of the rows near the window and the
 * lazy list never has to keep a row that is far from it. It is also pinned
 * while it is held, so it is not let go of for a moment between two frames.
 * Where the list's own rows stand is told only at the drop.
 */
internal class LaneState {
    /** The ids of the rows in the order they are drawn while a move is in hand or waits for the core to show it; null: the list's own order. */
    var order by mutableStateOf<List<String>?>(null)

    /** The ids in the list's own order when `order` was made. */
    var base: List<String> = emptyList()

    /** The id of the row in hand. */
    var held by mutableStateOf<String?>(null)

    /** How far the finger has moved since the row was picked up, in px. */
    var offsetY by mutableFloatStateOf(0f)

    /** Where the row in hand stood in the list's frame when it was picked up, in px, and how tall it is. */
    var startOffset = 0
    var heldSize = 0

    /** The way the finger last moved: -1 up, 1 down, 0 not yet. */
    var towards = 0f

    private var baseList: Navree.List? = null
    private var baseIds: List<String> = emptyList()

    /** Tells the tap, if a test set one, what the drag is at: the row, where it stands, where the finger is, and what the list shows. */
    fun note(what: String, state: LazyListState?, listId: String?) {
        val tap = LaneTrace.tap ?: return
        val info = state?.layoutInfo
        val seen = info?.visibleItemsInfo
        val own = seen?.firstOrNull { it.key == LaneRules.key(listId, held.orEmpty()) }
        tap(
            "$what held=$held place=${order?.indexOf(held)} index=${own?.index} offset=${own?.offset} finger=${startOffset + offsetY} " +
                "offsetY=$offsetY start=$startOffset size=$heldSize towards=$towards first=${seen?.firstOrNull()?.index} last=${seen?.lastOrNull()?.index} " +
                "viewStart=${info?.viewportStartOffset} viewEnd=${info?.viewportEndOffset} total=${info?.totalItemsCount}",
        )
    }

    /** The ids of the rows of the list that are moved, in the list's own order; made once for a list. */
    fun baseOf(list: Navree.List): List<String> {
        if (baseList !== list) {
            baseIds = LaneRules.movable(list)
            baseList = list
        }
        return baseIds
    }

    /** The order the rows are drawn in, when it is not the list's own: while a row is held, and while the list is still as it was before the drop. */
    fun effective(list: Navree.List): List<String>? = order?.takeIf { held != null || base == baseOf(list) }

    /** The rows' ids in the order they are drawn, and where each stands. */
    class View(val ids: List<String>, val places: Map<String, Int>)

    private var viewOrder: List<String>? = null
    private var viewBase: List<String>? = null
    private var viewMade: View? = null

    fun view(list: Navree.List): View {
        val own = baseOf(list)
        val wanted = effective(list)
        val made = viewMade
        if (made != null && viewBase === own && viewOrder === wanted) return made
        val ids = if (wanted != null && wanted.size == own.size && own.toHashSet().containsAll(wanted)) wanted else own
        val places = HashMap<String, Int>(ids.size * 2)
        ids.forEachIndexed { at, id -> places[id] = at }
        return View(ids, places).also {
            viewBase = own
            viewOrder = wanted
            viewMade = it
        }
    }

    private var targetsBody: Navree.Node? = null
    private var targetsList: Navree.List? = null
    private var targetsMade: List<Pair<Navree.List, String>> = emptyList()

    /** The lists a row of this one can be moved to; looked for once for a screen and a list, not once for each row. */
    fun targets(body: Navree.Node?, list: Navree.List): List<Pair<Navree.List, String>> {
        if (targetsBody !== body || targetsList !== list) {
            targetsMade = ReorderRules.targets(body, list)
            targetsBody = body
            targetsList = list
        }
        return targetsMade
    }

    /** Picks the row `id` up: from now on it follows the finger. False when the row is not one the list is showing. */
    fun begin(list: Navree.List, id: String, state: LazyListState): Boolean {
        val info = state.layoutInfo.visibleItemsInfo.firstOrNull { it.key == LaneRules.key(list.id, id) } ?: return false
        val shown = view(list).ids
        base = baseOf(list)
        order = shown
        startOffset = info.offset
        heldSize = info.size
        offsetY = 0f
        towards = 0f
        held = id
        note("begin", state, list.id)
        return true
    }

    fun drag(dy: Float, state: LazyListState? = null, listId: String? = null) {
        offsetY += dy
        if (dy != 0f) towards = if (dy < 0f) -1f else 1f
        note("drag dy=$dy", state, listId)
    }

    /** Where the row in hand is to be drawn against where its item stands: how far its layer is moved, in px. */
    fun translation(state: LazyListState, key: String): Float {
        val stands = state.layoutInfo.visibleItemsInfo.firstOrNull { it.key == key }?.offset ?: return 0f
        return startOffset + offsetY - stands
    }

    /** Puts the row in hand in the place the finger has reached among the rows that can be seen. */
    fun follow(list: Navree.List, state: LazyListState) {
        val id = held ?: return
        val shown = view(list)
        val from = shown.places[id]
        if (from == null) {
            cancel()
            return
        }
        val info = state.layoutInfo
        val slots = ArrayList<LaneRules.Slot>()
        for (item in info.visibleItemsInfo) {
            val other = LaneRules.idOf(list.id, item.key) ?: continue
            val place = shown.places[other] ?: continue
            if (place != from) slots += LaneRules.Slot(place, item.offset, item.size)
        }
        val ownKey = LaneRules.key(list.id, id)
        val heldTop = info.visibleItemsInfo.firstOrNull { it.key == ownKey }?.offset
        val to = LaneRules.landing(from, startOffset + offsetY, heldSize, slots, info.viewportStartOffset, info.viewportEndOffset, heldTop)
        if (to != from) {
            // A lazy list that has its first item moved follows the item: the row in hand, at the top of the window, would take the list along
            // with it as it passes each row, and pass the next, and the next. The window is kept where it is.
            val wasFirst = info.visibleItemsInfo.firstOrNull()?.key == ownKey
            val firstIndex = state.firstVisibleItemIndex
            val firstOffset = state.firstVisibleItemScrollOffset
            order = LaneRules.moved(shown.ids, from, to)
            if (wasFirst) state.requestScrollToItem(firstIndex, firstOffset)
        }
        note("follow from=$from to=$to heldTop=$heldTop slots=${slots.size}", state, list.id)
    }

    /** How far the list is to scroll in this frame, in px, for the row in hand. */
    fun scrollWanted(state: LazyListState, edge: Float, most: Float): Float {
        if (held == null) return 0f
        val info = state.layoutInfo
        return LaneRules.scrollBy(startOffset + offsetY, heldSize, info.viewportStartOffset, info.viewportEndOffset, edge, most, towards)
    }

    /** Where a drop put the row. */
    class Drop(val before: String?)

    /** Lets go of the row in hand; says where it was put, or null when it was put back where it was. */
    fun finish(list: Navree.List): Drop? {
        val id = held ?: return null
        val shown = view(list).ids
        held = null
        offsetY = 0f
        if (shown == baseOf(list)) {
            order = null
            LaneTrace.tap?.invoke("finish: back where it was, nothing is sent held=$id")
            return null
        }
        LaneTrace.tap?.invoke("finish: sends before=${LaneRules.before(shown, id)} held=$id place=${shown.indexOf(id)} of ${shown.size}")
        return Drop(LaneRules.before(shown, id))
    }

    fun cancel() {
        LaneTrace.tap?.invoke("cancel: the drag was lost held=$held")
        held = null
        offsetY = 0f
        order = null
    }

    /**
     * Ends the waiting for the core after a drop: when the list has changed
     * (the core shows the move, or put the row back), and in any case after a
     * moment, so a move the core did not take cannot leave the lane drawn in
     * an order it does not have.
     */
    @Composable
    fun Settling(list: Navree.List) {
        SideEffect {
            if (held == null && order != null && base != baseOf(list)) order = null
        }
        val waiting = held == null && order != null
        LaunchedEffect(waiting) {
            if (waiting) {
                delay(SETTLE_MS)
                order = null
            }
        }
    }

    companion object {
        /** How long a lane stays in the order of a drop while the core's answer is on its way, in ms. */
        const val SETTLE_MS = 1500L
    }
}

/** The lanes of a screen that are items of its own list, by the id of their list. */
internal class Lanes {
    private val states = HashMap<String, LaneState>()

    fun of(listId: String?): LaneState = states.getOrPut(listId.orEmpty()) { LaneState() }

    /** The lists whose rows are among `blocks`, by id, in the order they stand. */
    fun lists(blocks: List<Block>): Map<String, Navree.List> {
        var found: LinkedHashMap<String, Navree.List>? = null
        for (block in blocks) {
            if (block !is Block.Moving) continue
            val map = found ?: LinkedHashMap<String, Navree.List>().also { found = it }
            map.putIfAbsent(block.list.id.orEmpty(), block.list)
        }
        return found ?: emptyMap()
    }

    /** `blocks` with the rows of each lane in the order the lane is drawn in now. */
    fun arranged(blocks: List<Block>, lists: Map<String, Navree.List>): List<Block> {
        var out = blocks
        for ((id, list) in lists) {
            val order = of(id).effective(list) ?: continue
            out = LaneRules.arranged(out, { block -> (block as? Block.Moving)?.takeIf { it.list.id.orEmpty() == id }?.row?.id }, order)
        }
        return out
    }
}

internal val LocalPageList = staticCompositionLocalOf<LazyListState?> { null }
internal val LocalLanes = staticCompositionLocalOf<Lanes?> { null }
internal val LocalLaneChoices = staticCompositionLocalOf<LaneChoices?> { null }

/** Which lane of a board is in hand where the lanes are shown one at a time, kept through a turn of the device. */
internal class LaneChoices {
    var chosen by mutableStateOf<Map<String, Int>>(emptyMap())

    fun choose(id: String, at: Int) {
        chosen = chosen + (id to at)
    }
}

private val choicesSaver = Saver<LaneChoices, ArrayList<Any>>(
    save = { choices ->
        ArrayList<Any>().apply {
            for ((id, at) in choices.chosen) {
                add(id)
                add(at)
            }
        }
    },
    restore = { saved ->
        LaneChoices().also { choices ->
            choices.chosen = (0 until saved.size step 2).associate { saved[it] as String to saved[it + 1] as Int }
        }
    },
)

@Composable
internal fun rememberLaneChoices(): LaneChoices = rememberSaveable(saver = choicesSaver) { LaneChoices() }

/** The tabs of lanes shown one at a time, a tab a lane with its count under its name. */
@Composable
internal fun LaneTabsView(block: Block.LaneTabs) {
    val choices = LocalLaneChoices.current
    SecondaryScrollableTabRow(selectedTabIndex = block.chosen, edgePadding = 0.dp) {
        block.lanes.forEachIndexed { index, lane ->
            Tab(
                selected = index == block.chosen,
                onClick = { choices?.choose(block.id, index) },
                text = {
                    Column(horizontalAlignment = Alignment.CenterHorizontally) {
                        Text(lane.title ?: "", maxLines = 1, overflow = TextOverflow.Ellipsis)
                        lane.subtitle?.let { Text(it, style = MaterialTheme.typography.labelSmall, maxLines = 1) }
                    }
                },
            )
        }
    }
}

/** The end of a list that has more: drawing it asks for the next rows (the core asks once), or says the last ask failed. */
@Composable
internal fun MoreEnd(key: String, id: String, place: Place) {
    val ui = LocalUi.current
    val state = place.screen.lists[id]
    if (LaneRules.end(true, id, state?.failed == true) == LaneRules.End.Retry) {
        RetryLine(stringResource(R.string.navree_more_failed)) { ui.send(Events.more(place.id, id)) }
    } else {
        LaunchedEffect(key) { ui.send(Events.more(place.id, id)) }
        Box(Modifier.fillMaxWidth().padding(16.dp), contentAlignment = Alignment.Center) {
            CircularProgressIndicator(Modifier.size(24.dp), strokeWidth = 2.dp)
        }
    }
}

/**
 * One row of a list whose rows are moved: a card with a handle at its end. The
 * handle drags the row (where `listState` is the lazy list the row is an item
 * of), and opens a menu of the moves (up, down, to another list), which are
 * also offered to a screen reader as actions of the handle. The row says its
 * place in its list, which is the place it is drawn in now.
 *
 * `modifier` is what goes round the card (the screen's margins), `animation`
 * what makes the item move when its neighbours do, and is not used while the
 * row is in hand.
 */
@Composable
internal fun MovingRow(
    row: Navree.Row,
    list: Navree.List,
    lane: LaneState,
    listState: LazyListState?,
    place: Place,
    modifier: Modifier = Modifier,
    animation: Modifier = Modifier,
) {
    val ui = LocalUi.current
    val id = row.id ?: return
    val listId = list.id
    val view = lane.view(list)
    val ids = view.ids
    val index = view.places[id] ?: -1
    val targets = lane.targets(place.screen.screen?.body, list)
    val held = lane.held == id
    val key = LaneRules.key(listId, id)
    val up = index > 0
    val down = index in 0 until ids.lastIndex
    val upWords = stringResource(R.string.navree_move_up)
    val downWords = stringResource(R.string.navree_move_down)
    val toWords = targets.map { (_, name) -> stringResource(R.string.navree_move_to, name) }
    val handleName = stringResource(R.string.navree_move_card, row.title)
    val density = LocalDensity.current

    fun move(to: Navree.List, before: String?) {
        val from = listId ?: return
        val target = to.id ?: return
        ui.send(Events.move(place.id, id, from, target, before))
    }

    fun step(steps: Int) {
        val landing = ReorderRules.landing(ids, index, steps) ?: return
        move(list, landing.before)
    }

    val moves = buildList {
        if (up) add(CustomAccessibilityAction(upWords) { step(-1); true })
        if (down) add(CustomAccessibilityAction(downWords) { step(1); true })
        targets.forEachIndexed { at, (to, _) -> add(CustomAccessibilityAction(toWords[at]) { move(to, null); true }) }
    }

    val currentList by rememberUpdatedState(list)
    val currentDrop by rememberUpdatedState<(String?) -> Unit> { before -> move(currentList, before) }

    if (listState != null) {
        // While the row is held its item is kept, wherever the list scrolls to.
        val pin = LocalPinnableContainer.current
        DisposableEffect(held, pin) {
            val handle = if (held) pin?.pin() else null
            if (held) lane.note("pinned=${handle != null} (the item is composed)", listState, listId)
            onDispose {
                handle?.release()
                if (held) lane.note("released, or the item left the composition", listState, listId)
            }
        }
        // While it is held: it takes its place among the rows beside it, and the list scrolls by itself while it is near an edge of the window.
        LaunchedEffect(held) {
            if (!held) return@LaunchedEffect
            val edge = with(density) { 72.dp.toPx() }
            val most = with(density) { 20.dp.toPx() }
            launch {
                snapshotFlow { lane.offsetY to listState.layoutInfo }.collect { lane.follow(currentList, listState) }
            }
            var last = 0L
            while (isActive) {
                if (lane.scrollWanted(listState, edge, most) == 0f) {
                    snapshotFlow { lane.scrollWanted(listState, edge, most) != 0f }.first { it }
                    last = 0L
                    continue
                }
                val now = withFrameNanos { it }
                val frames = if (last == 0L) 1f else (now - last) / FRAME_NANOS
                last = now
                val wanted = lane.scrollWanted(listState, edge, most) * frames
                val used = if (wanted != 0f) listState.scrollBy(wanted) else 0f
                lane.note("scroll wanted=$wanted used=$used", listState, listId)
                if (wanted != 0f && used == 0f) {
                    // At the end of what is loaded: nothing to do until the finger moves or more rows come.
                    snapshotFlow { lane.offsetY to listState.layoutInfo.totalItemsCount }.drop(1).first()
                    last = 0L
                }
            }
        }
    }

    val dragging: Modifier = if (listState == null) {
        Modifier
    } else {
        Modifier.pointerInput(id) {
            detectDragGestures(
                onDragStart = { lane.begin(currentList, id, listState) },
                onDrag = { change, amount ->
                    change.consume()
                    lane.drag(amount.y, listState, listId)
                },
                onDragEnd = { lane.finish(currentList)?.let { currentDrop(it.before) } },
                onDragCancel = { lane.cancel() },
            )
        }
    }

    Box(
        modifier
            .then(if (held) Modifier else animation)
            .zIndex(if (held) 1f else 0f)
            .graphicsLayer { translationY = if (held && listState != null) lane.translation(listState, key) else 0f }
            .semantics { collectionItemInfo = CollectionItemInfo(index, 1, 0, 1) }
            .padding(bottom = 8.dp),
    ) {
        // The card is the row container: a tonal step above the surface, lifted while it is dragged.
        Card(
            Modifier.fillMaxWidth(),
            colors = CardDefaults.cardColors(containerColor = if (held) MaterialTheme.colorScheme.surfaceContainerHigh else MaterialTheme.colorScheme.surfaceContainerLow),
            elevation = CardDefaults.cardElevation(defaultElevation = if (held) 6.dp else 0.dp),
        ) {
            RowView(
                // The form that moves a card by its fields is for a page with no handle: here the handle is the one way.
                ReorderRules.withoutMoveForm(list, row),
                place,
                Group.None,
                handle = {
                    var open by remember { mutableStateOf(false) }
                    Box {
                        Icon(
                            Glyphs.DragHandle,
                            contentDescription = null,
                            tint = MaterialTheme.colorScheme.onSurfaceVariant,
                            modifier = Modifier
                                .size(48.dp)
                                .then(dragging)
                                // One stop for a screen reader: the handle, named for its card, with the moves as its actions.
                                .semantics {
                                    contentDescription = handleName
                                    customActions = moves
                                }
                                .clickable(role = Role.Button) { open = true }
                                .padding(12.dp),
                        )
                        DropdownMenu(expanded = open, onDismissRequest = { open = false }) {
                            if (up) DropdownMenuItem(text = { Text(upWords) }, onClick = { open = false; step(-1) })
                            if (down) DropdownMenuItem(text = { Text(downWords) }, onClick = { open = false; step(1) })
                            targets.forEachIndexed { at, (to, _) ->
                                DropdownMenuItem(text = { Text(toWords[at]) }, onClick = { open = false; move(to, null) })
                            }
                        }
                    }
                },
            )
        }
    }
}

private const val FRAME_NANOS = 16_666_667f

/** A child of a lane as the lazy list holds it: a row that is moved (`id` is its own) or anything else. */
private class LaneEntry(val at: Int, val id: String?, val key: String)

private fun laneEntries(list: Navree.List): List<LaneEntry> {
    val movable = LaneRules.movable(list).toHashSet()
    val taken = HashSet<String>()
    return list.children.mapIndexed { at, child ->
        val id = (child as? Navree.Row)?.id?.takeIf { it in movable && taken.add(it) }
        LaneEntry(at, id, if (id != null) LaneRules.key(list.id, id) else "plain:$at:${child.nodeId.orEmpty()}")
    }
}

/**
 * A list whose rows the person puts in order or moves to another list, laid
 * lazily where it has a height to scroll in (a lane of a board on a wide
 * window): only the rows near the window are drawn, whatever the list holds. A
 * list that stands where nothing bounds its height (inside a card, say) is
 * drawn whole and its rows are moved by their menus.
 */
@Composable
internal fun ReorderList(node: Navree.List, place: Place, modifier: Modifier = Modifier) {
    BoxWithConstraints(modifier) {
        if (constraints.hasBoundedHeight) LaneColumn(node, place) else WholeLane(node, place)
    }
}

/** A lane that is a lazy list of its own, no taller than the room it is given. */
@Composable
private fun LaneColumn(node: Navree.List, place: Place) {
    val lane = remember(node.id) { LaneState() }
    val listState = rememberLazyListState()
    val entries = remember(node) { laneEntries(node) }
    val shown = LaneRules.arranged(entries, { it.id }, lane.effective(node))
    lane.Settling(node)
    val more = LaneRules.end(node.more != null, node.id, place.screen.lists[node.id.orEmpty()]?.failed == true) != LaneRules.End.Nothing
    LazyColumn(state = listState, modifier = Modifier.fillMaxWidth()) {
        items(count = shown.size, key = { shown[it].key }, contentType = { if (shown[it].id != null) 0 else 1 }) { at ->
            val entry = shown[at]
            val child = node.children[entry.at]
            if (entry.id != null && child is Navree.Row) {
                MovingRow(child, node, lane, listState, place, animation = Modifier.animateItem())
            } else {
                Box(Modifier.animateItem().padding(bottom = 8.dp)) {
                    if (child is Navree.Row) RowView(child, place, Group.None) else NodeView(child, place)
                }
            }
        }
        val id = node.id
        if (more && id != null) item(key = "end:$id", contentType = 2) { MoreEnd("end:$id", id, place) }
    }
}

/** A lane drawn whole, where no lazy list can be laid: its rows are moved by their menus and by a screen reader's actions. */
@Composable
private fun WholeLane(node: Navree.List, place: Place) {
    val lane = remember(node.id) { LaneState() }
    val movable = remember(node) { LaneRules.movable(node).toHashSet() }
    Column(Modifier.semantics { collectionInfo = CollectionInfo(rowCount = movable.size, columnCount = 1) }) {
        for (child in node.children) {
            if (child is Navree.Row && child.id in movable) {
                MovingRow(child, node, lane, null, place)
            } else {
                Box(Modifier.padding(bottom = 8.dp)) {
                    if (child is Navree.Row) RowView(child, place, Group.None) else NodeView(child, place)
                }
            }
        }
    }
}
