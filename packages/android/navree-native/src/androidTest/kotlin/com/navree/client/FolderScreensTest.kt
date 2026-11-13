package com.navree.client

import androidx.activity.ComponentActivity
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performTouchInput
import androidx.compose.ui.test.swipeUp
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import com.navree.client.core.Content
import com.navree.client.core.Events
import com.navree.client.core.Stage
import com.navree.client.ui.NavreeApp
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.intOrNull
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import mockwebserver3.MockWebServer
import okhttp3.HttpUrl.Companion.toHttpUrl
import org.junit.After
import org.junit.Assume.assumeTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

/**
 * Screens from a folder of recorded answers on the device, named by the
 * instrumentation argument `answers` (without it the test is skipped). The
 * folder holds `shell.json`, the answers, and `screens.json`, the screens to
 * open in order:
 *
 * ```
 * [{"path": "/", "file": "today.json", "name": "today", "title": "Today", "shots": 4}]
 * ```
 *
 * `file` is served at `path` (more paths may be served with no shot by
 * giving `"shots": 0`); each screen is opened by its address, waited for by
 * its `title`, and photographed from its top `shots` times a window apart, as
 * `<device>-<look>-<name>-<n>.png` beside the other tests' shots.
 */
@OptIn(ExperimentalTestApi::class)
@RunWith(AndroidJUnit4::class)
class FolderScreensTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    private val server = MockWebServer()

    @After
    fun stop() {
        server.close()
    }

    /** A file of the folder, read as the shell user, who can read what `adb push` left there. */
    private fun read(folder: String, file: String): String? =
        Shots.shell("cat '$folder/$file'").takeIf { it.isNotEmpty() && !it.startsWith("cat:") }

    private fun waitFor(what: String, timeout: Long = 30_000, check: () -> Boolean) {
        try {
            compose.waitUntil(timeout) { check() }
        } catch (e: Throwable) {
            throw AssertionError("waited for $what", e)
        }
        compose.waitForIdle()
    }

    @Test
    fun theFoldersScreens() {
        val folder = InstrumentationRegistry.getArguments().getString("answers")?.trimEnd('/')
        assumeTrue("no folder of recorded answers was named", folder != null)
        folder!!
        val list = Json.parseToJsonElement(requireNotNull(read(folder, "screens.json")) { "$folder/screens.json is missing" }).jsonArray
        data class Visit(val path: String, val file: String, val name: String, val title: String?, val shots: Int)
        val visits = list.map { item ->
            val o = item.jsonObject
            Visit(
                path = o["path"]!!.jsonPrimitive.content,
                file = o["file"]!!.jsonPrimitive.content,
                name = o["name"]?.jsonPrimitive?.contentOrNull ?: o["file"]!!.jsonPrimitive.content.removeSuffix(".json"),
                title = o["title"]?.jsonPrimitive?.contentOrNull,
                shots = o["shots"]?.jsonPrimitive?.intOrNull ?: 1,
            )
        }
        val routes = mapOf("/_navree/native/shell" to "shell") + visits.associate { it.path to it.file.removeSuffix(".json") }
        server.dispatcher = Recorded("", routes) { file -> read(folder, file) }
        server.start()
        val origin = server.url("/").toString().trimEnd('/')
        val config = NavreeConfig(origin = origin, name = "", seed = null, loopbackHttp = true, openAnyLink = true)
        lateinit var app: NavreeSession
        compose.runOnUiThread {
            app = NavreeSession(compose.activity.application, config, origin.toHttpUrl())
            app.effects.activity = compose.activity
        }
        compose.setContent { Shots.Looked { NavreeApp(app.driver, config, image = app::image) } }
        waitFor("the app is ready", 60_000) { app.driver.nav.stage == Stage.Ready }

        for (visit in visits.filter { it.shots > 0 }) {
            compose.runOnUiThread { app.driver.send(Events.openLink("${server.url(visit.path)}")) }
            waitFor("the screen at ${visit.path}") {
                val front = app.driver.nav.front?.let { app.driver.screens[it] }
                val tree = (front?.content as? Content.Native)?.screen
                tree != null && !front.loading && (visit.title == null || tree.title == visit.title)
            }
            Thread.sleep(1200)
            compose.waitForIdle()
            Shots.take("${visit.name}-1")
            for (index in 2..visit.shots) {
                compose.onRoot().performTouchInput { swipeUp(startY = bottom * 0.85f, endY = bottom * 0.2f, durationMillis = 600) }
                Thread.sleep(900)
                compose.waitForIdle()
                Shots.take("${visit.name}-$index")
            }
        }
    }
}
