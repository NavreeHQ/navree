package com.navree.client

import android.graphics.Bitmap
import android.net.Uri
import android.text.format.Formatter
import androidx.activity.ComponentActivity
import androidx.activity.compose.LocalActivityResultRegistryOwner
import androidx.activity.result.ActivityResultRegistry
import androidx.activity.result.ActivityResultRegistryOwner
import androidx.activity.result.contract.ActivityResultContract
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.SemanticsNodeInteraction
import androidx.compose.ui.test.assertDoesNotExist
import androidx.compose.ui.test.assertExists
import androidx.compose.ui.test.assertIsEnabled
import androidx.compose.ui.test.assertIsNotEnabled
import androidx.compose.ui.test.assertIsSelected
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.hasSetTextAction
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.isFocused
import androidx.compose.ui.test.isSelected
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performTextReplacement
import androidx.core.app.ActivityOptionsCompat
import androidx.exifinterface.media.ExifInterface
import androidx.lifecycle.Lifecycle
import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.navree.client.core.Content
import com.navree.client.core.Events
import com.navree.client.core.FieldValue
import com.navree.client.core.Stage
import com.navree.client.design.ColourRules
import com.navree.client.design.walk
import com.navree.client.platform.PickedFiles
import mockwebserver3.Dispatcher
import mockwebserver3.MockResponse
import mockwebserver3.MockWebServer
import mockwebserver3.RecordedRequest
import okhttp3.HttpUrl.Companion.toHttpUrl
import org.junit.After
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import java.io.ByteArrayInputStream
import java.io.File
import java.security.MessageDigest
import java.util.Collections
import java.util.Random
import java.util.concurrent.atomic.AtomicInteger

/**
 * The two fields the client draws itself: a file field and a colour field.
 * Served by a recorded form on a real device, with the system's picker
 * replaced as the platform's own testing guide has it, by a registry that
 * answers the launch with the documents the test names (the picker's own
 * screen is not driven); what comes back goes the whole way: copied, shown,
 * told to the core, and sent in the form's request to a fake server, whose
 * copy of the bytes is compared by hash.
 */
@OptIn(ExperimentalTestApi::class)
@RunWith(AndroidJUnit4::class)
class InputFieldsTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    private val context get() = ApplicationProvider.getApplicationContext<android.content.Context>()
    private val server = MockWebServer()
    private val picker = FakePicker()
    private lateinit var claims: Claims
    private lateinit var app: NavreeSession

    @Before
    fun readingListsFollowThePictures() {
        Shots.rule = compose
    }

    @After
    fun stop() {
        server.close()
        TestDocuments.clear()
        PickedFiles.of(context).mostBytes = com.navree.client.design.FileRules.MOST_BYTES
        PickedFiles.of(context).clear()
    }

    /** What the picker answers with when it is launched: the documents the test named last. */
    private class FakePicker : ActivityResultRegistry() {
        @Volatile
        var next: List<Uri> = emptyList()
        val launches = AtomicInteger()

        @Suppress("UNCHECKED_CAST")
        override fun <I, O> onLaunch(requestCode: Int, contract: ActivityResultContract<I, O>, input: I, options: ActivityOptionsCompat?) {
            launches.incrementAndGet()
            dispatchResult(requestCode, next as O)
        }
    }

    /** Answers a native page's request from the recorded answers and a post with the form again, remembering the post. */
    private class Claims(private val recorded: Recorded, private val form: String) : Dispatcher() {
        val posts: MutableList<RecordedRequest> = Collections.synchronizedList(ArrayList())

        override fun dispatch(request: RecordedRequest): MockResponse {
            if (request.method != "POST") return recorded.dispatch(request)
            posts += request
            return MockResponse.Builder().code(200).addHeader("Content-Type", "application/vnd.navree.native+json").body(form).build()
        }
    }

    private fun claimsForm(): String = """
        {"navree":1,"build":"inputs","vocab":0,"screen":{"title":"Receipts","subtitle":"Charkha Supply Co.","body":{"t":"form","id":"claim","method":"post","action":"/claims","children":[
          {"t":"field","id":"f-label","name":"label","label":"Label colour","input":{"t":"colour","value":"#16a34a"}},
          {"t":"field","id":"f-papers","name":"papers","label":"Papers","help":"Bills and photographs for the claim.","input":{"t":"file","accept":"image/*,.pdf","multiple":true}},
          {"t":"field","id":"f-photo","name":"photo","label":"Photo","input":{"t":"file","accept":"image/*"}},
          {"t":"field","id":"f-snap","name":"snap","label":"Snap","input":{"t":"file","accept":"image/*","keep_location":true}},
          {"t":"button","label":"Save","role":"primary","action":{"t":"submit"}}
        ]}}}
    """.trimIndent()

    /** Starts the app against the recorded answers and the claims form, and waits for the form. */
    private fun startOnTheClaimsForm(): NavreeSession {
        val form = claimsForm()
        return start(form, "/claims/new" to "claims", "Receipts") { file -> if (file == "claims.json") form else null }
    }

    private fun start(form: String, route: Pair<String, String>, title: String, read: ((String) -> String?)? = null): NavreeSession {
        claims = Claims(Recorded("", mapOf("/_navree/native/shell" to "shell", "/" to "screen-dashboard", route.first to route.second), read), form)
        server.dispatcher = claims
        server.start()
        val origin = server.url("/").toString().trimEnd('/')
        val config = NavreeConfig(origin = origin, name = "Charkha Supply Co.", seed = null, loopbackHttp = true, openAnyLink = true)
        lateinit var made: NavreeSession
        compose.runOnUiThread {
            made = NavreeSession(compose.activity.application, config, origin.toHttpUrl())
            made.effects.activity = compose.activity
        }
        app = made
        Shots.tree = { made.driver.nav.front?.let { made.driver.screens[it] }?.screen?.let { com.navree.protocol.Navree.json.encodeToString(com.navree.protocol.Navree.Screen.serializer(), it) } }
        val owner = object : ActivityResultRegistryOwner {
            override val activityResultRegistry: ActivityResultRegistry = picker
        }
        compose.setContent { CompositionLocalProvider(LocalActivityResultRegistryOwner provides owner) { Shots.Looked { Host(made) } } }
        waitFor("the app is ready", 60_000) { made.driver.nav.stage == Stage.Ready }
        front("Today")
        compose.runOnUiThread { made.driver.send(Events.openLink("${server.url(route.first)}")) }
        front(title)
        return made
    }

    private fun waitFor(what: String, timeout: Long = 20_000, check: () -> Boolean) {
        try {
            compose.waitUntil(timeout) { check() }
        } catch (e: Throwable) {
            throw AssertionError("waited for $what", e)
        }
        compose.waitForIdle()
    }

    /** Waits for the screen in front to show a native tree titled [title], then for drawing to settle. */
    private fun front(title: String) {
        waitFor("the screen \"$title\"") {
            val front = app.driver.nav.front?.let { app.driver.screens[it] }
            (front?.content as? Content.Native)?.screen?.title == title && !front.loading
        }
        Thread.sleep(600)
        compose.waitForIdle()
    }

    /** What the core holds in a field of the form in front. */
    private fun held(id: String): FieldValue? {
        val front = app.driver.nav.front?.let { app.driver.screens[it] }
        return front?.field("claim", id)?.value
    }

    private fun heldFiles(id: String) = (held(id) as? FieldValue.Files)?.files.orEmpty()

    private fun reveal(node: SemanticsNodeInteraction): SemanticsNodeInteraction {
        // A form that is longer than the window scrolls; one that is not has nothing to scroll to.
        runCatching { node.performScrollTo() }
        return node
    }

    private fun chooseButton(label: String, named: String) = compose.onNode(hasContentDescription("$named, $label"))

    private fun pick(label: String, button: String, vararg documents: Uri) {
        picker.next = documents.toList()
        val before = picker.launches.get()
        reveal(chooseButton(label, button)).performClick()
        waitFor("the picker to be asked") { picker.launches.get() > before }
    }

    private fun document(name: String, type: String?, bytes: ByteArray, knowsSize: Boolean = true): Uri = TestDocuments.add(context, name, type, bytes, knowsSize)

    private fun randomBytes(size: Int, seed: Long = 42): ByteArray = ByteArray(size).also { Random(seed).nextBytes(it) }

    private fun sha(bytes: ByteArray): String = MessageDigest.getInstance("SHA-256").digest(bytes).joinToString("") { "%02x".format(it) }

    /** A part of a recorded multipart request. */
    private class Part(val headers: String, val bytes: ByteArray) {
        val name: String? get() = Regex("name=\"([^\"]*)\"").find(headers)?.groupValues?.get(1)
        val filename: String? get() = Regex("filename=\"([^\"]*)\"").find(headers)?.groupValues?.get(1)
        val type: String? get() = Regex("(?i)content-type: *([^\r\n]*)").find(headers)?.groupValues?.get(1)
    }

    private fun indexOf(whole: ByteArray, part: ByteArray, from: Int): Int {
        var at = from
        while (at <= whole.size - part.size) {
            var same = true
            for (i in part.indices) if (whole[at + i] != part[i]) { same = false; break }
            if (same) return at
            at++
        }
        return -1
    }

    /** The parts of a multipart request, in order. */
    private fun partsOf(request: RecordedRequest): List<Part> {
        val boundary = request.headers["Content-Type"]!!.substringAfter("boundary=").trim('"')
        val body = request.body!!.toByteArray()
        val delimiter = "--$boundary".toByteArray()
        val found = ArrayList<Part>()
        var start = indexOf(body, delimiter, 0)
        while (start >= 0) {
            val after = start + delimiter.size
            // The delimiter that ends the body is followed by two dashes.
            if (after + 1 < body.size && body[after] == '-'.code.toByte() && body[after + 1] == '-'.code.toByte()) break
            val next = indexOf(body, delimiter, after)
            if (next < 0) break
            // A part starts after the line break that follows its delimiter and ends before the one that precedes the next.
            val segment = body.copyOfRange(after + 2, next - 2)
            val split = indexOf(segment, "\r\n\r\n".toByteArray(), 0)
            found += Part(String(segment, 0, split, Charsets.UTF_8), segment.copyOfRange(split + 4, segment.size))
            start = next
        }
        return found
    }

    private fun save() {
        reveal(compose.onNodeWithText("Save")).performClick()
        waitFor("the form to be sent") { claims.posts.isNotEmpty() }
    }

    // ---------------------------------------------------------------- both kinds of field are a native screen

    /**
     * The protocol's own form example holds a colour field ("Label colour") and
     * a file field ("Papers"): served whole, it is a native screen drawn by the
     * client, not the web page it was while the client said it could not draw them.
     */
    @Test
    fun theProtocolsFormExampleWithAColourAndAFileFieldIsANativeScreen() {
        server.dispatcher = Recorded("", mapOf("/_navree/native/shell" to "shell", "/" to "screen-dashboard", "/customers/new" to "screen-form"))
        server.start()
        val origin = server.url("/").toString().trimEnd('/')
        val config = NavreeConfig(origin = origin, name = "Charkha Supply Co.", seed = null, loopbackHttp = true, openAnyLink = true)
        lateinit var made: NavreeSession
        compose.runOnUiThread {
            made = NavreeSession(compose.activity.application, config, origin.toHttpUrl())
            made.effects.activity = compose.activity
        }
        app = made
        Shots.tree = { null }
        compose.setContent { Shots.Looked { Host(made) } }
        waitFor("the app is ready", 60_000) { made.driver.nav.stage == Stage.Ready }
        front("Today")
        compose.runOnUiThread { made.driver.send(Events.openLink("${server.url("/customers/new")}")) }
        front("New customer")
        val screen = app.driver.screens[app.driver.nav.front!!]!!
        assertTrue("the form is a native screen, not a web page", screen.content is Content.Native)
        val kinds = (screen.content as Content.Native).screen.body.walk().filterIsInstance<com.navree.protocol.Navree.Field>().map { it.input }.toList()
        assertTrue("the example's colour field is in the tree the client draws", kinds.any { it is com.navree.protocol.Navree.ColourInput })
        assertTrue("the example's file field is in the tree the client draws", kinds.any { it is com.navree.protocol.Navree.FileInput })
        reveal(compose.onNode(hasContentDescription("Label colour", substring = true))).assertExists()
        reveal(compose.onNode(hasContentDescription("Choose files, Papers"))).assertExists()
        Shots.take("inputs-protocol-form")
    }

    // ---------------------------------------------------------------- the colour field

    private fun openColour() {
        reveal(compose.onNode(hasContentDescription("Label colour", substring = true))).performClick()
        waitFor("the colour dialog") { compose.onAllNodes(hasText("Hex value", substring = true)).fetchSemanticsNodes().isNotEmpty() || compose.onAllNodes(hasSetTextAction()).fetchSemanticsNodes().isNotEmpty() }
        Thread.sleep(400)
        compose.waitForIdle()
    }

    /** The swatch of a colour: a selectable with a name that ends in its value (the field's own overlay, which names the value too, is not one). */
    private fun isSwatch(hex: String) = SemanticsMatcher("is the swatch $hex") { node ->
        val said = node.config.getOrNull(SemanticsProperties.ContentDescription)?.joinToString(", ").orEmpty()
        said.endsWith(", $hex") && !said.startsWith("Label colour") && node.config.contains(SemanticsProperties.Selected)
    }

    private fun swatch(hex: String) = compose.onNode(isSwatch(hex))

    /** Choosing a swatch and confirming sends `#rrggbb`, which the form then posts under the field's name. */
    @Test
    fun choosingASwatchSendsTheValueInTheProtocolsFormAndTheFormPostsIt() {
        startOnTheClaimsForm()
        assertEquals("the page's own value is held first", FieldValue.Text("#16a34a"), held("f-label"))
        reveal(compose.onNode(hasContentDescription("Label colour, #16a34a"))).assertExists()
        openColour()
        swatch("#2563eb").performClick()
        compose.waitForIdle()
        assertEquals("nothing is sent until the choice is confirmed", FieldValue.Text("#16a34a"), held("f-label"))
        compose.onNodeWithText("OK").performClick()
        waitFor("the colour to reach the core") { held("f-label") == FieldValue.Text("#2563eb") }
        reveal(compose.onNode(hasContentDescription("Label colour, #2563eb"))).assertExists()
        save()
        val body = claims.posts.last().body!!.utf8()
        assertTrue("the form posts the colour as #rrggbb under the field's name: $body", body.contains("label=%232563eb"))
    }

    /** A hex value that is not a colour is said so at the box, cannot be confirmed, and changes nothing. */
    @Test
    fun aWrongHexShowsTheErrorAndSendsNothing() {
        startOnTheClaimsForm()
        openColour()
        compose.onNode(hasSetTextAction()).performTextReplacement("12zz")
        compose.waitForIdle()
        compose.onNodeWithText("Not a colour", substring = true).assertExists()
        compose.onNodeWithText("OK").assertIsNotEnabled()
        Shots.take("inputs-colour-wrong-hex")
        compose.onNodeWithText("Cancel").performClick()
        compose.waitForIdle()
        assertEquals("a wrong value sent nothing", FieldValue.Text("#16a34a"), held("f-label"))
        // A right one, with no hash and in capitals, is read and can be confirmed.
        openColour()
        compose.onNode(hasSetTextAction()).performTextReplacement("DC2626")
        compose.waitForIdle()
        compose.onNodeWithText("OK").assertIsEnabled()
        compose.onNodeWithText("OK").performClick()
        waitFor("the typed colour to reach the core") { held("f-label") == FieldValue.Text("#dc2626") }
    }

    /** The colour the field holds is the swatch that is chosen, and it is announced as chosen: one swatch, by more than colour. */
    @Test
    fun theChosenSwatchIsAnnouncedAsChosen() {
        startOnTheClaimsForm()
        openColour()
        swatch("#16a34a").assertIsSelected()
        val selected = compose.onAllNodes(isSelected()).fetchSemanticsNodes()
        assertEquals("one swatch is chosen", 1, selected.size)
        val name = selected.single().config.getOrNull(SemanticsProperties.ContentDescription)?.joinToString(", ").orEmpty()
        assertEquals("it is named by its colour's family and its value", "Green, #16a34a", name)
        for (rgb in ColourRules.PALETTE) {
            val swatches = compose.onAllNodes(isSwatch(ColourRules.hex(rgb))).fetchSemanticsNodes()
            assertEquals("${ColourRules.hex(rgb)} is one swatch with a name", 1, swatches.size)
        }
        Shots.take("inputs-colour-sheet")
        Shots.requireWindowsFollowTheScreen("the colour dialog")
    }

    /**
     * A hardware keyboard's Tab reaches the colour field as one named stop,
     * Enter opens it, and every control inside is a stop with a name: each
     * swatch, the three sliders, the hex box and the two buttons.
     */
    @Test
    fun aTabWalkReachesTheColourFieldOpensItAndEveryControlInsideIsANamedStop() {
        startOnTheClaimsForm()
        fun focusedName(): String? {
            val focused = compose.onAllNodes(isFocused()).fetchSemanticsNodes()
            if (focused.size != 1) return null
            val config = focused.single().config
            return listOfNotNull(
                config.getOrNull(SemanticsProperties.ContentDescription)?.joinToString(", "),
                config.getOrNull(SemanticsProperties.Text)?.joinToString(", ") { it.text },
                config.getOrNull(SemanticsProperties.EditableText)?.text,
            ).filter { it.isNotEmpty() }.joinToString(" | ").ifEmpty { null }
        }
        fun key(code: Int) {
            Shots.shell("input keyevent $code")
            Thread.sleep(250)
            compose.waitForIdle()
        }
        val outside = ArrayList<String>()
        var reached = false
        repeat(12) { press ->
            if (!reached) {
                key(61)
                val name = focusedName()
                assertNotNull("press ${press + 1} of Tab landed on nothing that can be seen or said; the stops so far: $outside", name)
                outside += name!!
                reached = name.startsWith("Label colour")
            }
        }
        assertTrue("Tab reached the colour field as a stop named by its label and value; the stops: $outside", reached)
        key(66)
        Thread.sleep(500)
        compose.waitForIdle()
        val inside = LinkedHashSet<String>()
        repeat(26) { press ->
            key(61)
            val name = focusedName()
            assertNotNull("press ${press + 1} of Tab inside the colour dialog landed on nothing that can be seen or said; the stops so far: $inside", name)
            inside += name!!
        }
        for (rgb in ColourRules.PALETTE) {
            assertTrue("a stop for the swatch ${ColourRules.hex(rgb)}; the stops: $inside", inside.any { it.contains(", ${ColourRules.hex(rgb)}") })
        }
        for (control in listOf("Hue", "Saturation", "Lightness", "Cancel", "OK")) {
            assertTrue("a stop named $control; the stops: $inside", inside.any { it.startsWith(control) })
        }
        assertTrue("a stop for the hex box, named by what it holds or by its label; the stops: $inside", inside.any { it.contains("Hex value") || it.startsWith("#") })
    }

    // ---------------------------------------------------------------- the file field

    private val pdf = "application/pdf"

    /** A file chosen is shown with its name and size, the core holds it by token, and the remove button clears it. */
    @Test
    fun aChosenFileShowsItsNameAndSizeAndRemovingItClearsTheField() {
        startOnTheClaimsForm()
        val bytes = randomBytes(1_234_567)
        pick("Papers", "Choose files", document("Receipt – Ludhiana.pdf", pdf, bytes))
        waitFor("the file to reach the core") { heldFiles("f-papers").size == 1 }
        val file = heldFiles("f-papers").single()
        assertEquals("Receipt – Ludhiana.pdf", file.name)
        assertEquals(pdf, file.mediaType)
        assertEquals(bytes.size.toLong(), file.size)
        reveal(compose.onNode(hasText("Receipt – Ludhiana.pdf", substring = true))).assertExists()
        compose.onNode(hasText(Formatter.formatShortFileSize(context, bytes.size.toLong()), substring = true)).assertExists()
        assertNotNull("the copy the form will send is kept", PickedFiles.of(context).file(file.token))
        // A second file is added to the first, as the field takes several.
        pick("Papers", "Add files", document("Bill Amritsar.pdf", pdf, randomBytes(2_000, seed = 7), knowsSize = false))
        waitFor("the second file to reach the core") { heldFiles("f-papers").size == 2 }
        assertEquals("a provider that does not know its size is read as far as it goes", 2_000L, heldFiles("f-papers")[1].size)
        Shots.take("inputs-files-chosen")
        reveal(compose.onNode(hasContentDescription("Remove Receipt – Ludhiana.pdf"))).performClick()
        waitFor("the first file to be removed") { heldFiles("f-papers").size == 1 }
        assertEquals("Bill Amritsar.pdf", heldFiles("f-papers").single().name)
        waitFor("its copy to go") { PickedFiles.of(context).file(file.token) == null }
        compose.onNode(hasText("Receipt – Ludhiana.pdf", substring = true)).assertDoesNotExist()
    }

    /** A file that is too large or is not a kind the field takes is said so at the field, and is not added. */
    @Test
    fun aFileThatIsTooLargeOrOfAnotherKindIsRefusedWithWordsAndNotAdded() {
        startOnTheClaimsForm()
        PickedFiles.of(context).mostBytes = 1_000
        pick("Papers", "Choose files", document("Pariwar tasveer.pdf", pdf, randomBytes(5_000)))
        reveal(compose.onNode(hasText("larger than", substring = true))).assertExists()
        compose.onNode(hasText("Pariwar tasveer.pdf is larger than", substring = true)).assertExists()
        assertTrue("a file that is too large is not held", heldFiles("f-papers").isEmpty())
        Shots.take("inputs-file-too-large")
        PickedFiles.of(context).mostBytes = com.navree.client.design.FileRules.MOST_BYTES
        pick("Papers", "Choose files", document("notes.txt", "text/plain", "Dukaan di sooch".toByteArray()))
        reveal(compose.onNode(hasText("not a kind of file this field takes", substring = true))).assertExists()
        compose.onNode(hasText("image/*, .pdf", substring = true)).assertExists()
        assertTrue("a file of another kind is not held", heldFiles("f-papers").isEmpty())
        // The next good choice takes the words away.
        pick("Papers", "Choose files", document("Rasid Jalandhar.pdf", pdf, randomBytes(3_000, seed = 9)))
        waitFor("the good file to reach the core") { heldFiles("f-papers").size == 1 }
        compose.onNode(hasText("not a kind of file this field takes", substring = true)).assertDoesNotExist()
    }

    /** Sending the form delivers each file's bytes to the server exactly, under the field's name, with its name and type. */
    @Test
    fun sendingTheFormDeliversTheChosenFilesBytesExactly() {
        startOnTheClaimsForm()
        // Every byte value, line breaks and the dashes of a boundary among them: nothing the parts' framing could confuse.
        val first = randomBytes(300_000) + "\r\n--boundary-looking\r\n".toByteArray() + randomBytes(10_000, seed = 3)
        val second = randomBytes(40_000, seed = 11)
        pick("Papers", "Choose files", document("Receipt – Ludhiana.pdf", pdf, first), document("Bill Amritsar.png", "image/png", second))
        waitFor("both files to reach the core") { heldFiles("f-papers").size == 2 }
        save()
        val request = claims.posts.last()
        assertTrue("a form with files is sent as multipart", request.headers["Content-Type"]!!.startsWith("multipart/form-data"))
        val parts = partsOf(request)
        val sent = parts.filter { it.name == "papers" }
        assertEquals("one part for each file, under the field's name: ${parts.map { it.name }}", 2, sent.size)
        assertEquals("Receipt – Ludhiana.pdf", sent[0].filename)
        assertEquals(pdf, sent[0].type)
        assertEquals(sha(first), sha(sent[0].bytes))
        assertEquals(first.size, sent[0].bytes.size)
        assertEquals("Bill Amritsar.png", sent[1].filename)
        assertEquals("image/png", sent[1].type)
        assertEquals(sha(second), sha(sent[1].bytes))
        assertEquals("the colour is sent with them, as text", "#16a34a", String(parts.single { it.name == "label" }.bytes, Charsets.UTF_8))
        // The files were read from their copies, as a stream: the documents themselves could have gone.
        assertArrayEquals(second, sent[1].bytes)
    }

    /**
     * A copy the system cleared while the app was away is shown as gone when
     * the app comes back, and the form is sent with no part for it: never an
     * empty file under its name.
     */
    @Test
    fun aFileThatIsGoneIsShownAsGoneAndIsNotSentAtAll() {
        startOnTheClaimsForm()
        pick("Papers", "Choose files", document("Receipt – Ludhiana.pdf", pdf, randomBytes(50_000)))
        waitFor("the file to reach the core") { heldFiles("f-papers").size == 1 }
        compose.onNode(hasText("No longer available", substring = true)).assertDoesNotExist()
        val token = heldFiles("f-papers").single().token
        assertTrue(PickedFiles.of(context).file(token)!!.delete())
        // The app goes to the background and comes back.
        compose.activityRule.scenario.moveToState(Lifecycle.State.STARTED)
        compose.activityRule.scenario.moveToState(Lifecycle.State.RESUMED)
        waitFor("the file to be shown as gone") { compose.onAllNodes(hasText("No longer available", substring = true)).fetchSemanticsNodes().isNotEmpty() }
        Shots.take("inputs-file-gone")
        save()
        val request = claims.posts.last()
        val text = String(request.body!!.toByteArray(), Charsets.UTF_8)
        assertFalse("no part with a file name was sent: nothing for a file that is gone", text.contains("filename="))
        assertFalse("not an empty part under the field's name", partsOf(request).any { it.name == "papers" })
        // It can be removed, and chosen again.
        reveal(compose.onNode(hasContentDescription("Remove Receipt – Ludhiana.pdf"))).performClick()
        waitFor("the gone file to be removed") { heldFiles("f-papers").isEmpty() }
        compose.onNode(hasText("No longer available", substring = true)).assertDoesNotExist()
    }

    /**
     * The protocol says a client takes the place out of every photo unless the
     * field keeps it: the same photograph, chosen for a field that does not
     * keep it and one that does, is sent without it and with it.
     */
    @Test
    fun thePlaceAPictureWasTakenIsTakenOutUnlessTheFieldKeepsIt() {
        startOnTheClaimsForm()
        val file = File(context.cacheDir, "ludhiana-photo.jpg")
        val picture = Bitmap.createBitmap(16, 16, Bitmap.Config.ARGB_8888).also { it.eraseColor(0xFF2563EB.toInt()) }
        file.outputStream().use { out -> picture.compress(Bitmap.CompressFormat.JPEG, 90, out) }
        ExifInterface(file).apply {
            setLatLong(30.9010, 75.8573)
            saveAttributes()
        }
        val bytes = file.readBytes()
        assertNotNull("the fixture holds the place it was taken", ExifInterface(ByteArrayInputStream(bytes)).latLong)
        val photo = document("Ludhiana photo.jpg", "image/jpeg", bytes)
        pick("Photo", "Choose a file", photo)
        waitFor("the photo to reach the core") { heldFiles("f-photo").size == 1 }
        pick("Snap", "Choose a file", photo)
        waitFor("the snap to reach the core") { heldFiles("f-snap").size == 1 }
        save()
        val parts = partsOf(claims.posts.last())
        val plain = parts.single { it.name == "photo" }
        val kept = parts.single { it.name == "snap" }
        assertNull("the place is taken out of the photo of a field that does not keep it", ExifInterface(ByteArrayInputStream(plain.bytes)).latLong)
        assertNotNull("and left in the one of a field that does", ExifInterface(ByteArrayInputStream(kept.bytes)).latLong)
        file.delete()
    }

    // ---------------------------------------------------------------- pictures

    /** Both fields as a person sees them, with files chosen, in the look the job set; and the colour dialog open. */
    @Test
    fun bothFieldsArePicturedWithAFileChosenAndTheColourDialogOpen() {
        startOnTheClaimsForm()
        reveal(compose.onNode(hasContentDescription("Choose files, Papers")))
        Shots.take("inputs-fields-empty")
        pick("Papers", "Choose files", document("Receipt – Ludhiana.pdf", pdf, randomBytes(1_234_567)), document("Bill Amritsar.png", "image/png", randomBytes(88_000, seed = 5)))
        waitFor("both files to reach the core") { heldFiles("f-papers").size == 2 }
        Thread.sleep(400)
        reveal(compose.onNode(hasContentDescription("Choose files, Papers")))
        Shots.take("inputs-fields-filled")
        openColour()
        compose.onNode(hasSetTextAction()).performTextReplacement("#7c3aed")
        compose.waitForIdle()
        Thread.sleep(300)
        Shots.take("inputs-colour-sheet-custom")
        Shots.requireWindowsFollowTheScreen("the colour dialog")
    }
}
