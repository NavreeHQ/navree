package com.navree.client

import androidx.activity.ComponentActivity
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.state.ToggleableState
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.hasAnyAncestor
import androidx.compose.ui.test.hasAnyDescendant
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.hasScrollToIndexAction
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.isFocused
import androidx.compose.ui.test.isPopup
import androidx.compose.ui.test.isToggleable
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onFirst
import androidx.compose.ui.test.onLast
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performKeyInput
import androidx.compose.ui.test.performScrollToIndex
import androidx.compose.ui.test.performSemanticsAction
import androidx.compose.ui.test.performTouchInput
import androidx.compose.ui.test.pressKey
import com.navree.client.ui.LaneTrace
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.navree.client.core.Content
import mockwebserver3.Dispatcher
import mockwebserver3.MockResponse
import mockwebserver3.MockWebServer
import mockwebserver3.RecordedRequest
import okhttp3.HttpUrl.Companion.toHttpUrl
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import java.util.concurrent.CopyOnWriteArrayList

/**
 * A board whose first lane holds 200 cards (the most the board demo's server
 * keeps in a lane), drawn lazily: a card is held and dragged past the edge of
 * the lane so that the lane scrolls under it, a card is moved by the keyboard
 * and by its handle's menu, and a card that is scrolled out of the lane and back
 * shows what it showed. The server is a stand-in that records what it is told.
 */
@OptIn(ExperimentalTestApi::class)
@RunWith(AndroidJUnit4::class)
class LaneMoveTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    @Before
    fun readingListsFollowThePictures() {
        Shots.rule = compose
    }

    private val server = MockWebServer()

    /** The forms of the moves the server was told, in order. */
    private val moves = CopyOnWriteArrayList<Map<String, String>>()

    /** What happened while a card was dragged, a line each, with the time in ms since the test began: written beside the pictures. */
    private val trace = java.util.Collections.synchronizedList(ArrayList<String>())
    private val began = System.nanoTime()

    private fun mark(what: String) {
        trace += "${(System.nanoTime() - began) / 1_000_000} ms $what"
    }

    @After
    fun stop() {
        server.close()
    }

    private fun open(cards: Int, ticks: Boolean = false) {
        val body = LaneBoard.json(cards, ticks)
        val served = Recorded("", routes = mapOf("/_navree/native/shell" to "shell", "/" to "screen-board"), read = { file -> if (file == "screen-board.json") body else null })
        server.dispatcher = object : Dispatcher() {
            override fun dispatch(request: RecordedRequest): MockResponse {
                if (request.method == "POST" && request.url.encodedPath == "/cards/move") {
                    val form = LaneBoard.form(request.body?.utf8().orEmpty())
                    moves += form
                    mark("SERVER told of a move: $form")
                    return MockResponse.Builder().code(204).build()
                }
                return served.dispatch(request)
            }
        }
        server.start()
        val origin = server.url("/").toString().trimEnd('/')
        val config = NavreeConfig(origin = origin, name = "Board", seed = null, loopbackHttp = true, openAnyLink = true)
        lateinit var made: NavreeSession
        compose.runOnUiThread {
            made = NavreeSession(compose.activity.application, config, origin.toHttpUrl())
            made.effects.activity = compose.activity
        }
        compose.setContent { Shots.Looked { Host(made) } }
        compose.waitUntil(180_000) {
            val front = made.driver.nav.front?.let { made.driver.screens[it] }
            (front?.content as? Content.Native)?.screen?.title == "Board" && !front.loading
        }
        compose.waitForIdle()
        waitFor("the first card") { shown().isNotEmpty() }
    }

    private fun waitFor(what: String, timeout: Long = 20_000, check: () -> Boolean) {
        try {
            compose.waitUntil(timeout) { check() }
        } catch (e: Throwable) {
            throw AssertionError("waited for $what; the cards drawn: ${shown()}; the server was told $moves", e)
        }
        compose.waitForIdle()
    }

    /** The numbers of the cards of the first lane that are drawn, from the top of the window down. */
    private fun shown(): List<Int> =
        compose.onAllNodes(hasText(LaneBoard.TITLE_END, substring = true)).fetchSemanticsNodes()
            // A card that is composed ahead of the window and not placed has no place to sort by.
            .filter { it.layoutInfo.isPlaced }
            .sortedBy { it.boundsInRoot.top }
            .mapNotNull { node ->
                val words = node.config.getOrNull(SemanticsProperties.Text)?.joinToString(" ") { it.text }.orEmpty()
                Regex("Card (\\d+) of").find(words)?.groupValues?.get(1)?.toInt()
            }
            .distinct()

    private fun handle(at: Int) =
        compose.onAllNodes(hasContentDescription("Move ${LaneBoard.title(at)}"), useUnmergedTree = true).onFirst()

    /** Whether the handle of a card has the focus, in the tree a screen reader reads (the keyboard tests of the screens read that one) or in the tree of every part. */
    private fun focused(at: Int) =
        listOf(false, true).any { unmerged ->
            compose.onAllNodes(isFocused() and hasContentDescription("Move ${LaneBoard.title(at)}"), useUnmergedTree = unmerged).fetchSemanticsNodes().isNotEmpty()
        }

    /** What has the focus now, in words. */
    private fun stopNow(): String = compose.onAllNodes(isFocused()).fetchSemanticsNodes().joinToString(" + ") { node ->
        listOfNotNull(
            node.config.getOrNull(SemanticsProperties.ContentDescription)?.joinToString(", "),
            node.config.getOrNull(SemanticsProperties.Text)?.joinToString(", ") { it.text },
        ).joinToString(" | ").ifEmpty { "(a stop with no name)" }
    }.ifEmpty { "(nothing)" }

    /** One of the moves a screen reader or a keyboard is offered on a card's handle. */
    private fun moveWith(at: Int, label: String) {
        val action = handle(at).fetchSemanticsNode().config[SemanticsActions.CustomActions].first { it.label == label }
        compose.runOnUiThread { action.action() }
    }

    /** Opens a card's menu and chooses a move in it. */
    private fun chooseInMenu(label: String) {
        waitFor("the menu with \"$label\"") { compose.onAllNodes(hasText(label) and hasAnyAncestor(isPopup())).fetchSemanticsNodes().isNotEmpty() }
        compose.onAllNodes(hasText(label) and hasAnyAncestor(isPopup())).onFirst().performClick()
    }

    /** What the server was told by its nth move, and that it was told it by a card of the first lane. */
    private fun told(nth: Int, item: Int, before: Int?) {
        waitFor("the server's ${nth + 1}th move") { moves.size > nth }
        val form = moves[nth]
        assertEquals("the card that moved, by its key", item.toString(), form["item"])
        assertEquals("from the first lane", "todo", form["from"])
        assertEquals("to the first lane", "todo", form["to"])
        assertEquals("the card it stands before", before?.toString().orEmpty(), form["before"].orEmpty())
    }

    @Test
    fun aCardHeldPastTheBottomOfALaneOfTwoHundredCardsScrollsTheLaneUnderItAndStandsWhereItIsLetGo() {
        // Whatever comes of it, the file beside the pictures says what the drag did, a line for each step of it.
        LaneTrace.tap = { line -> mark(line) }
        try {
            dragAndDrop()
        } finally {
            LaneTrace.tap = null
            mark("the cards drawn at the end: ${runCatching { shown() }.getOrNull()}; the server was told: $moves")
            Shots.put("${Shots.device}-${Shots.look}-lane-drag-trace.txt", trace.joinToString("\n") + "\n")
        }
    }

    private fun dragAndDrop() {
        open(200)
        val root = compose.onRoot().fetchSemanticsNode().boundsInRoot
        val grab = handle(0).fetchSemanticsNode().boundsInRoot.center
        mark("the window is ${root.width} x ${root.height} px; the handle of card 0 is at $grab")
        compose.onRoot().performTouchInput {
            mark("the finger goes down on the handle")
            down(grab)
            // Into the lane, clear of its edges: the card lifts and follows the finger, and nothing scrolls.
            for (step in 1..10) {
                mark("the finger moves to y=${grab.y + 20f * step}")
                moveTo(Offset(grab.x, grab.y + 20f * step), delayMillis = 16)
            }
        }
        compose.waitForIdle()
        assertTrue("the held card is still drawn", shown().contains(0))
        mark("a picture is taken with the card held")
        Shots.take("lane-held")
        // The clock is held and moved by hand, a frame at a time: the events of one block are given to the screen at once, with no frame
        // between them, so a lane that scrolls by itself (it does it a frame at a time) would see the finger arrive and go in one frame.
        compose.mainClock.autoAdvance = false
        try {
            val from = grab.y + 200f
            val to = root.bottom - 6f
            // Down to the bottom edge, and held there for more than three seconds: the lane scrolls by itself, by more than two windows of cards, so the card would leave the list if it were not kept.
            for (step in 1..20) {
                val y = from + (to - from) * step / 20f
                mark("the finger moves to y=$y")
                compose.onRoot().performTouchInput { moveTo(Offset(grab.x, y)) }
                compose.mainClock.advanceTimeBy(16)
            }
            repeat(200) { n ->
                if (n % 20 == 0) mark("the finger is held at the edge, ${n * 16} ms")
                compose.mainClock.advanceTimeBy(16)
            }
            mark("the finger goes up")
            compose.onRoot().performTouchInput { up() }
            compose.mainClock.advanceTimeBy(200)
        } finally {
            compose.mainClock.autoAdvance = true
        }
        compose.waitForIdle()
        mark("the test is idle after the finger went up")
        waitFor("the server told of the move") { moves.size == 1 }
        // One move, at the drop, with the final place: not one for each card the held card passed on the way.
        Thread.sleep(1_500)
        assertEquals("the server was told one move: $moves", 1, moves.size)
        val form = moves.single()
        assertEquals("the card that was held", "0", form["item"])
        assertEquals("in its own lane", "todo", form["from"])
        assertEquals("in its own lane", "todo", form["to"])

        waitFor("the card drawn where it was let go") { shown().contains(0) }
        val order = shown()
        val others = order.filter { it != 0 }
        val at = order.indexOf(0)
        assertTrue("the lane scrolled under the card: it shows cards from ${others.first()} on, in $order", others.first() >= 25)
        assertEquals("the other cards keep their order", others.sorted(), others)
        if (at + 1 < order.size) {
            assertEquals("the server was told the card stands before the card that is drawn after it: $order", order[at + 1].toString(), form["before"])
        } else {
            assertTrue("the card is the last drawn: the card it stands before is further down (${form["before"]}), $order", (form["before"]?.toIntOrNull() ?: Int.MAX_VALUE) > others.last())
        }
        // The finger was let go at the bottom edge, so the card stands in the last place that is wholly drawn: one of the last two cards.
        assertTrue("the card, let go at the bottom edge, stands among the last two cards drawn: place $at of ${order.size} in $order", at >= order.size - 2)
        Shots.take("lane-dropped")
    }

    @Test
    fun aCardMovedByTheKeyboardAndByItsMenuKeepsTheFocusAndStandsWhereTheMoveSays() {
        open(200)
        // As the screens' own Tab walks do: the screen has settled, and the key goes to the root.
        Thread.sleep(1_500)
        compose.waitForIdle()
        // The keyboard's Tab walks the screen from its top, a stop at a time, to the first card's handle (a button takes the focus only when the keyboard is in use).
        val stops = ArrayList<String>()
        for (press in 1..10) {
            if (focused(0)) break
            compose.onRoot().performKeyInput { pressKey(Key.Tab) }
            compose.waitForIdle()
            stops += stopNow()
        }
        val tabbed = focused(0)
        if (!tabbed) {
            handle(0).performSemanticsAction(SemanticsActions.RequestFocus)
            compose.waitForIdle()
        }
        val node = handle(0).fetchSemanticsNode()
        assertTrue(
            "the first card's handle has the focus (reached by Tab: $tabbed; the stops of Tab: $stops; now: ${stopNow()}; " +
                "the handle says focused=${node.config.getOrNull(SemanticsProperties.Focused)} and can be asked for the focus: ${node.config.contains(SemanticsActions.RequestFocus)}; " +
                "the window has the focus: ${compose.activity.hasWindowFocus()}; the view that has it: ${compose.activity.currentFocus})",
            focused(0),
        )

        // The moves a keyboard and a screen reader are offered on the handle.
        moveWith(0, "Move down")
        waitFor("the first card second") { shown().take(3) == listOf(1, 0, 2) }
        told(0, item = 0, before = 2)
        assertTrue("the focus stays on the card that moved", focused(0))

        // Enter on the handle opens its menu.
        handle(0).performKeyInput { pressKey(Key.Enter) }
        chooseInMenu("Move down")
        waitFor("the card third") { shown().take(4) == listOf(1, 2, 0, 3) }
        told(1, item = 0, before = 3)
        assertTrue("the focus stays on the card after a move from its menu", focused(0))
        Shots.take("lane-moved")

        // A tap on the handle opens the same menu: up from the third place.
        handle(0).performSemanticsAction(SemanticsActions.OnClick)
        chooseInMenu("Move up")
        waitFor("the card second again") { shown().take(3) == listOf(1, 0, 2) }
        told(2, item = 0, before = 2)
    }

    @Test
    fun aCardThatIsScrolledOutOfTheLaneAndBackShowsWhatItShowed() {
        open(200, ticks = true)
        val first = LaneBoard.title(0)
        fun tick() = compose.onAllNodes(isToggleable() and hasAnyAncestor(hasText(first))).onFirst()
        fun ticked() = tick().fetchSemanticsNode().config.getOrNull(SemanticsProperties.ToggleableState) == ToggleableState.On
        assertTrue("the first card is not ticked at first", !ticked())
        tick().performSemanticsAction(SemanticsActions.OnClick)
        waitFor("the first card ticked") { ticked() }

        // The list that holds the cards: the screen's own on a phone, the lane's on a wide window (the last that holds a card).
        fun list(card: String) = compose.onAllNodes(hasScrollToIndexAction() and hasAnyDescendant(hasText(card, substring = true))).onLast()
        list(first).performScrollToIndex(150)
        waitFor("the first card gone from what is drawn") { compose.onAllNodes(hasText(first)).fetchSemanticsNodes().isEmpty() }
        assertTrue("a lane of 200 cards has few of them drawn: ${shown().size}", shown().size < 60)
        list(LaneBoard.TITLE_END).performScrollToIndex(0)
        waitFor("the first card drawn again") { compose.onAllNodes(hasText(first)).fetchSemanticsNodes().isNotEmpty() }
        assertTrue("the first card is still ticked after it was scrolled out and back", ticked())
        Shots.take("lane-ticked")
    }
}
