package com.navree.client

import androidx.activity.ComponentActivity
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.hasAnyAncestor
import androidx.compose.ui.test.hasClickAction
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.hasSetTextAction
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.pressKey
import androidx.compose.ui.test.performKeyInput
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.isFocused
import androidx.compose.ui.test.isPopup
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performImeAction
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.onLast
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.longClick
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onFirst
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.test.performTouchInput
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import com.navree.client.core.Content
import com.navree.client.core.Events
import com.navree.client.core.ScreenView
import com.navree.client.core.Stage
import com.navree.client.design.ListRules
import com.navree.client.design.ReorderRules
import com.navree.client.design.ScreenRules
import com.navree.client.design.walk
import com.navree.protocol.Navree
import okhttp3.HttpUrl.Companion.toHttpUrl
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Assert.assertNotNull
import org.junit.Assume.assumeTrue
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

/**
 * The client against the demos themselves, running on the machine the
 * emulator runs on (`-e store http://10.0.2.2:8095`, `-e inbox …`, `-e board …`).
 * Every screen is the server's answer of now, drawn by the client, and
 * photographed: each destination, then what a person does next on it. Without
 * an app's argument its test is skipped.
 */
@RunWith(AndroidJUnit4::class)
class LiveDemosTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    @Before
    fun readingListsFollowThePictures() {
        Shots.rule = compose
    }

    private val arguments = InstrumentationRegistry.getArguments()

    private fun waitFor(what: String, timeout: Long = 30_000, check: () -> Boolean) {
        try {
            compose.waitUntil(timeout) { check() }
        } catch (e: Throwable) {
            throw AssertionError("waited for $what", e)
        }
        compose.waitForIdle()
    }

    private fun front(app: NavreeSession): ScreenView? = app.driver.nav.front?.let { app.driver.screens[it] }

    /** Waits for the screen in front to show something, and for drawing to settle. */
    private fun settled(app: NavreeSession, title: String? = null) {
        waitFor("a screen${title?.let { " \"$it\"" } ?: ""}") {
            val front = front(app) ?: return@waitFor false
            val tree = (front.content as? Content.Native)?.screen
            !front.loading && (if (title == null) tree != null || front.content is Content.Web else tree?.title == title)
        }
        Thread.sleep(1500)
        compose.waitForIdle()
    }

    /** What the screen in front is, as a tree, for the file beside each picture. */
    private fun treeOf(app: NavreeSession): String? =
        app.driver.nav.front?.let { app.driver.screens[it] }?.screen?.let { com.navree.protocol.Navree.json.encodeToString(com.navree.protocol.Navree.Screen.serializer(), it) }

    private fun send(app: NavreeSession, event: kotlinx.serialization.json.JsonObject) {
        compose.runOnUiThread { app.driver.send(event) }
    }

    private fun start(origin: String): NavreeSession {
        val config = NavreeConfig(origin = origin, name = "Demo", seed = null, loopbackHttp = true, openAnyLink = true)
        lateinit var app: NavreeSession
        compose.runOnUiThread {
            app = NavreeSession(compose.activity.application, config, origin.toHttpUrl())
            app.effects.activity = compose.activity
        }
        Shots.tree = { treeOf(app) }
        compose.setContent { Shots.Looked { Host(app) } }
        waitFor("the app is ready", 60_000) { app.driver.nav.stage == Stage.Ready }
        settled(app)
        return app
    }

    /** Every destination of the app, photographed in turn; ends on the first. */
    private fun destinations(app: NavreeSession, name: String) {
        val count = app.driver.nav.destinations.size
        for (index in 0 until count) {
            send(app, Events.select(index))
            settled(app)
            val label = app.driver.nav.destinations[index].label.lowercase().replace(Regex("[^a-z0-9]+"), "-")
            Shots.take("$name-$label")
        }
        send(app, Events.select(0))
        settled(app)
    }

    /** The first row of the screen's main table or list, by the words it is drawn with. */
    private fun firstRowWords(screen: Navree.Screen): String? {
        ScreenRules.mainTable(screen.body)?.let { table ->
            return table.rows.firstOrNull()?.let { ListRules.asListRow(table.columns, it).headline }
        }
        // A card of the board is a row whose title is a button of its own, not a row with an action.
        return screen.body.walk().filterIsInstance<Navree.Row>().firstOrNull { it.action != null || ListRules.titleButton(it) != null }?.title
    }

    /** Opens the first row of the screen in front and photographs what it opened. */
    private fun openFirstRow(app: NavreeSession, shot: String) {
        val screen = requireNotNull(front(app)?.screen) { "$shot: no screen is in front" }
        val words = requireNotNull(firstRowWords(screen)) { "$shot: the screen in front has no row to open" }
        val before = front(app)?.id
        compose.onAllNodesWithText(words).onFirst().performClick()
        // What a row opens is the server's to say: a screen, a cover, or a sheet over the list.
        waitFor("what the row opened") {
            val now = front(app)
            now?.id != before || app.driver.nav.modals.isNotEmpty() || now?.overlays?.isNotEmpty() == true
        }
        settled(app)
        Shots.take(shot)
    }

    /** Opens the search view of the screen in front and types, then photographs it. */
    private fun search(app: NavreeSession, words: String, shot: String) {
        val field = compose.onAllNodes(hasSetTextAction()).onFirst()
        field.performClick()
        // The search view opens as a field of its own: the words go to the one that has the focus, once it has it.
        compose.waitForIdle()
        val focused = compose.onAllNodes(hasSetTextAction() and isFocused())
        waitFor("the search field has the focus", 8_000) { focused.fetchSemanticsNodes().isNotEmpty() }
        Thread.sleep(300)
        focused.onFirst().performTextInput(words)
        waitFor("the words are in the field", 8_000) { compose.onAllNodes(hasSetTextAction() and hasText(words)).fetchSemanticsNodes().isNotEmpty() }
        Thread.sleep(2500)
        compose.waitForIdle()
        Shots.take(shot)
    }

    @Test
    fun theStoreDemo() {
        val origin = arguments.getString("store")
        assumeTrue("no store demo was named", origin != null)
        val app = start(origin!!)
        destinations(app, "store")

        send(app, Events.select(1))
        settled(app)
        openFirstRow(app, "store-order")
        send(app, Events.back)
        settled(app)
        search(app, "Kaur", "store-orders-search")
    }

    /** One stop of a keyboard's Tab: the node's id, its name, and where it stands in the window. */
    private class Stop(val id: Int, val name: String, val inside: Boolean, val widthDp: Float, val heightDp: Float)

    /** What a screen reader says of a node: its description, its words, what is typed in it. */
    private fun nameOf(node: androidx.compose.ui.semantics.SemanticsNode): String {
        val config = node.config
        return listOfNotNull(
            config.getOrNull(SemanticsProperties.ContentDescription)?.joinToString(", "),
            config.getOrNull(SemanticsProperties.Text)?.joinToString(", ") { it.text },
            config.getOrNull(SemanticsProperties.EditableText)?.text,
        ).filter { it.isNotEmpty() }.joinToString(" | ")
    }

    /** Everything on the screen that can take the focus (the nodes that say whether they have it), as `name@top` in dp, in the order they stand from the top. */
    private fun focusableThings(): List<String> =
        compose.onAllNodes(androidx.compose.ui.test.SemanticsMatcher.keyIsDefined(SemanticsProperties.Focused)).fetchSemanticsNodes()
            .filter { it.size.width > 0 && it.size.height > 0 }
            .sortedWith(compareBy({ it.boundsInRoot.top }, { it.boundsInRoot.left }))
            .map { node -> "${nameOf(node).take(24)}@${(node.boundsInRoot.top / node.layoutInfo.density.density).toInt()}" }

    private fun focusedStop(): Stop? {
        val focused = compose.onAllNodes(isFocused()).fetchSemanticsNodes()
        if (focused.size != 1) return null
        val node = focused.single()
        val density = node.layoutInfo.density.density
        val named = nameOf(node)
        val window = compose.onRoot().fetchSemanticsNode().boundsInRoot
        val box = node.boundsInRoot
        val inside = box.left >= window.left - 1f && box.top >= window.top - 1f && box.right <= window.right + 1f && box.bottom <= window.bottom + 1f
        return Stop(node.id, named, inside, node.size.width / density, node.size.height / density)
    }

    /**
     * A hardware keyboard's Tab through a screen of fields and buttons (the
     * store's inventory): every stop is one control with a name, a size a finger
     * could hit and bounds inside the window; no control is a stop twice; and
     * every field that was in view at the start is a stop.
     */
    @Test
    fun aTabWalkThroughTheStoresInventoryVisitsEachVisibleFieldOnceAndStopsOnlyWhereSomethingCanBeSeenAndSaid() {
        val origin = arguments.getString("store")
        assumeTrue("no store demo was named", origin != null)
        val app = start(origin!!)
        send(app, Events.select(2))
        settled(app)
        val start = compose.onAllNodes(hasSetTextAction()).fetchSemanticsNodes()
        val fields = start.map { it.id }
        val named = start.associate { node -> node.id to (node.config.getOrNull(SemanticsProperties.EditableText)?.text.orEmpty() + " " + node.config.getOrNull(SemanticsProperties.Text)?.joinToString(", ") { it.text }.orEmpty()).trim() }
        assertTrue("the inventory shows fields to walk through", fields.isNotEmpty())
        val seen = ArrayList<Stop>()
        // Two stops a row (its field and its button), the page's own controls first, and a margin.
        repeat((fields.size * 3 + 20).coerceAtMost(120)) { press ->
            Shots.pressTab()
            compose.waitForIdle()
            val stop = focusedStop()
            assertTrue("press ${press + 1} of Tab landed on nothing that can be seen or said; the stops so far: ${seen.map { it.name.take(30) }}", stop != null)
            stop!!
            assertTrue("press ${press + 1}: the stop has no name", stop.name.isNotEmpty())
            assertTrue("press ${press + 1}: \"${stop.name}\" is ${stop.widthDp}x${stop.heightDp} dp, too small to hit", stop.widthDp >= 8f && stop.heightDp >= 8f)
            assertTrue("press ${press + 1}: \"${stop.name}\" stands outside the window", stop.inside)
            assertTrue("press ${press + 1}: \"${stop.name}\" is a stop for the second time", seen.none { it.id == stop.id })
            seen += stop
        }
        val missed = fields.filter { id -> seen.none { it.id == id } }
        assertTrue("fields that were in view at the start and were never a stop in ${seen.size} presses: ${missed.size} of ${fields.size}: ${missed.map { named[it] }}; the stops were ${seen.map { it.name.take(24) }}", missed.isEmpty())
    }

    /**
     * With the focus nowhere, a keyboard's Tab reaches the screen's search
     * within as many presses as the screen has things that can take the focus:
     * the search stands at the top of the screen and Tab goes by where things
     * stand. When it does not, the message names every focusable thing and every
     * stop.
     */
    @Test
    fun aKeyboardReachesTheSearchOfAScreenWithinTheNumberOfFocusableThingsOnIt() {
        val origin = arguments.getString("store")
        assumeTrue("no store demo was named", origin != null)
        val app = start(origin!!)
        send(app, Events.select(2))
        settled(app)
        val things = focusableThings()
        assertTrue("the inventory has things that can take the focus", things.isNotEmpty())
        val stops = ArrayList<String>()
        var reached = false
        repeat(things.size + 2) {
            if (reached) return@repeat
            Shots.pressTab()
            compose.waitForIdle()
            val node = compose.onAllNodes(isFocused()).fetchSemanticsNodes()
            val named = node.joinToString(" + ") { nameOf(it).take(28) + "#" + it.id }
            stops += named
            if (node.any { nameOf(it).contains("Search", ignoreCase = true) }) reached = true
        }
        assertTrue("Search was never a stop in ${stops.size} presses (the screen has ${things.size} things that can take the focus: $things); the stops were $stops", reached)
    }

    /** After a field's keyboard action ends the typing, the out-of-sight place that takes the focus is not a stop of the next Tab. */
    @Test
    fun aKeyboardsDoneLeavesNoInvisibleStopForTheNextTab() {
        val origin = arguments.getString("store")
        assumeTrue("no store demo was named", origin != null)
        val app = start(origin!!)
        send(app, Events.select(2))
        settled(app)
        // The first field is the search box; the next is a row's quantity, a field of a form that its keyboard action ends.
        val field = compose.onAllNodes(hasSetTextAction())[1]
        field.performClick()
        compose.waitForIdle()
        field.performImeAction()
        compose.waitForIdle()
        Thread.sleep(500)
        assertEquals("no field holds the focus after Done", 0, compose.onAllNodes(hasSetTextAction() and isFocused()).fetchSemanticsNodes().size)
        repeat(6) { press ->
            Shots.pressTab()
            compose.waitForIdle()
            val stop = focusedStop()
            assertTrue("press ${press + 1} of Tab after Done landed on nothing that can be seen or said", stop != null)
        }
    }

    @Test
    fun theInboxDemo() {
        val origin = arguments.getString("inbox")
        assumeTrue("no inbox demo was named", origin != null)
        val app = start(origin!!)
        destinations(app, "inbox")
        openFirstRow(app, "inbox-conversation")
    }

    @Test
    fun theBoardDemo() {
        val origin = arguments.getString("board")
        assumeTrue("no board demo was named", origin != null)
        val app = start(origin!!)
        destinations(app, "board")
        openFirstRow(app, "board-card")
    }

    /**
     * What a person does next, as opposed to a screen to look at, is walked in two looks (the plain one and
     * the right to left one, whose windows and menus mirror): the other looks only draw screens.
     */
    private fun walkedHere() = assumeTrue("walked in the light and right to left looks only", Shots.look == "light" || Shots.look == "rtl")

    /** The cards of every lane of a board, as the tree says. */
    private fun cards(screen: Navree.Screen?): List<Navree.Row> =
        screen?.body.let { body -> body?.walk().orEmpty() }.filterIsInstance<Navree.List>().filter { it.reorder != null }
            .flatMap { it.children.filterIsInstance<Navree.Row>() }.toList()

    /** The cards of the lane whose list has this id. */
    private fun lane(screen: Navree.Screen?, id: String?): List<Navree.Row> =
        screen?.body.let { body -> body?.walk().orEmpty() }.filterIsInstance<Navree.List>().filter { it.id == id }
            .flatMap { it.children.filterIsInstance<Navree.Row>() }.toList()

    /** The labels a card carries (the badges that have a dot). */
    private fun labels(card: Navree.Row): List<String> = card.walk().filterIsInstance<Navree.Badge>().filter { it.dot }.map { it.text }.toList()

    @Test
    fun theBoardsFiltersNarrowTheBoardAndClearAgain() {
        walkedHere()
        val origin = arguments.getString("board")
        assumeTrue("no board demo was named", origin != null)
        val app = start(origin!!)
        val before = cards(front(app)?.screen)
        assertTrue("the board has cards", before.isNotEmpty())
        assertTrue("and not only bugs", before.any { "Bug" !in labels(it) })
        // A filter is a chip that opens its choices, not a field with a box.
        compose.onNode(hasText("Label") and hasClickAction()).performClick()
        waitFor("the label choices") { compose.onAllNodes(hasText("Bug") and hasAnyAncestor(isPopup())).fetchSemanticsNodes().isNotEmpty() }
        Thread.sleep(1500)
        Shots.take("board-filter-menu")
        compose.onAllNodes(hasText("Bug") and hasAnyAncestor(isPopup())).onFirst().performClick()
        // The cards that stay carry the label that was chosen (a card may carry another beside it).
        Thread.sleep(2500)
        Shots.take("board-filter-chosen")
        try {
            waitFor("a board of bugs only") {
                val now = cards(front(app)?.screen)
                now.isNotEmpty() && now.all { "Bug" in labels(it) }
            }
        } catch (e: AssertionError) {
            val now = cards(front(app)?.screen)
            throw AssertionError("${e.message}: ${now.size} cards, the first labelled ${now.take(6).map { labels(it) }}, the screen at ${front(app)?.url}", e)
        }
        Thread.sleep(800)
        Shots.take("board-filtered")
        compose.onNode(hasText("Clear filters") and hasClickAction()).performClick()
        waitFor("every card again") { cards(front(app)?.screen).any { "Bug" !in labels(it) } }
        settled(app)
        Shots.take("board-filter-cleared")
    }

    @Test
    fun aCardMovesToAnotherLaneByItsHandle() {
        walkedHere()
        val origin = arguments.getString("board")
        assumeTrue("no board demo was named", origin != null)
        val app = start(origin!!)
        val screen = front(app)?.screen
        val lists = screen?.body?.walk().orEmpty().filterIsInstance<Navree.List>().filter { it.reorder != null && it.children.any { child -> child is Navree.Row } }.toList()
        assertTrue("the board has lanes with cards", lists.size >= 2)
        val from = lists.first()
        val card = from.children.filterIsInstance<Navree.Row>().first()
        val (target, name) = ReorderRules.targets(screen?.body, from).first()
        // A card has no row of fields to move it by: its handle opens the moves.
        assertTrue("no Move row on a card", compose.onAllNodes(hasText("Move") and hasClickAction()).fetchSemanticsNodes().isEmpty())
        compose.onAllNodes(hasContentDescription("Move ${card.title}"), useUnmergedTree = true).onFirst().performClick()
        waitFor("the card's moves") { compose.onAllNodes(hasText("Move to $name") and hasAnyAncestor(isPopup())).fetchSemanticsNodes().isNotEmpty() }
        Thread.sleep(1500)
        Shots.take("board-card-menu")
        compose.onAllNodes(hasText("Move to $name") and hasAnyAncestor(isPopup())).onFirst().performClick()
        waitFor("the card in $name") { lane(front(app)?.screen, target.id).any { it.title == card.title } }
        settled(app)
        Shots.take("board-card-moved")
        // And back, so the next look finds the board as it was.
        send(app, Events.move(front(app)!!.id, card.id!!, target.id!!, from.id!!, null))
        waitFor("the card back where it was") { lane(front(app)?.screen, target.id).none { it.title == card.title } }
    }

    @Test
    fun aFormThatIsRefusedShowsWhatIsWrongAndTheFirstFieldTakesTheFocus() {
        walkedHere()
        val origin = arguments.getString("board")
        assumeTrue("no board demo was named", origin != null)
        val app = start(origin!!)
        val card = cards(front(app)?.screen).first()
        compose.onAllNodesWithText(card.title).onFirst().performClick()
        waitFor("the card opened") { front(app)?.overlays?.isNotEmpty() == true || app.driver.nav.modals.isNotEmpty() }
        settled(app)
        val send = compose.onAllNodes(hasText("Comment") and hasClickAction()).onFirst()
        runCatching { send.performScrollTo() }
        send.performClick()
        val wrong = SemanticsMatcher.keyIsDefined(SemanticsProperties.Error)
        waitFor("the field says what is wrong") { compose.onAllNodes(wrong).fetchSemanticsNodes().isNotEmpty() }
        Thread.sleep(800)
        Shots.take("board-card-form-errors")
        waitFor("the field in error has the focus") { compose.onAllNodes(wrong and isFocused()).fetchSemanticsNodes().isNotEmpty() }
    }

    @Test
    fun aFieldThatIsDoneWithLetsGoOfTheFocusWithoutTheSearchTakingIt() {
        walkedHere()
        val origin = arguments.getString("store")
        assumeTrue("no store demo was named", origin != null)
        val app = start(origin!!)
        send(app, Events.select(2))
        settled(app)
        val fields = compose.onAllNodes(hasSetTextAction())
        assertTrue("a search and a restock field", fields.fetchSemanticsNodes().size >= 2)
        val restock = fields[1]
        restock.performClick()
        restock.performTextInput("7")
        restock.performImeAction()
        Thread.sleep(1500)
        compose.waitForIdle()
        // No field has the focus, so no keyboard is up; the search is still there to be reached.
        assertEquals("no field holds the focus", 0, compose.onAllNodes(hasSetTextAction() and isFocused()).fetchSemanticsNodes().size)
        assertTrue("the search is still a field", compose.onAllNodes(hasSetTextAction()).fetchSemanticsNodes().isNotEmpty())
        Shots.take("store-restock-done")
    }
}
