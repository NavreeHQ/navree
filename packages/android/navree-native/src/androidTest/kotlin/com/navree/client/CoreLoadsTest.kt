package com.navree.client

import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import com.navree.client.core.Driver
import com.navree.client.core.Effect
import com.navree.client.core.EffectRunner
import com.navree.client.core.Engine
import com.navree.client.core.Events
import com.navree.client.platform.Heard
import com.navree.client.platform.PersonWatch
import com.navree.core.Core
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.booleanOrNull
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import java.io.File
import java.nio.file.Files

/** The runtime core's library loads on a device and answers through its four native methods. */
@RunWith(AndroidJUnit4::class)
class CoreLoadsTest {
    private val config = """
        {"origin":"https://shop.example",
         "client":{"platform":"android","os":"35","form":"phone","app":"1.0.0","lib":"0.1.0","vocab":0,"text_percent":100}}
    """.trimIndent().toByteArray()

    @Test
    fun startingAsksForTheShell() {
        Core(config).use { core ->
            val update = String(core.send("""{"t":"start"}""".toByteArray()))
            assertTrue(update, update.contains("\"t\":\"fetch\""))
            assertTrue(update, update.contains("https://shop.example/_navree/native/shell"))
            val nav = String(core.view("""{"part":"nav"}""".toByteArray()))
            assertTrue(nav, nav.contains("\"stage\":\"starting\""))
        }
    }

    @Test
    fun anEventThatDoesNotReadIsAnError() {
        Core(config).use { core ->
            val answer = String(core.send("not json".toByteArray()))
            assertTrue(answer, answer.contains("\"error\""))
        }
    }

    @Test
    fun everyConversationOfTheCoreReplaysAndMatchesWhatTheDriverHolds() {
        val assets = InstrumentationRegistry.getInstrumentation().context.assets
        val names = assets.list("transcripts").orEmpty().filter { it.endsWith(".json") }.sorted()
        assertTrue("transcripts packaged into androidTest assets", names.isNotEmpty())
        val required = listOf(
            "person-key-steady.json",
            "person-key-other-id.json",
            "session-ended-outside.json",
            "sign-in-cover.json",
            "sign-in-another-person.json",
            "sign-in-shell-fails.json",
        )
        for (name in required) {
            assertTrue("$name is replayed", name in names)
        }
        for (name in names) {
            replayTranscript(assets, name)
        }
    }

    @Test
    fun aPersonKeepsOneMarkWhenTheServersIdComesAndGoes() {
        val folder = Files.createTempDirectory("navree-person-core").toFile()
        try {
            val markFile = File(folder, PersonWatch.MARK)
            val watch = PersonWatch(markFile, "https://shop.example:443")
            val heard = ArrayList<Heard>()
            val marks = ArrayList<String>()

            fun shellBody(name: String, email: String?): String {
                val emailLine = if (email != null) ""","email":"$email"""" else ""
                return """{"navree":1,"build":"1","name":"Charkha Supply Co.","home":"/","nav":{"groups":[{"label":"Store","items":[{"label":"Home","href":"/"}]}]},"user":{"name":"$name"$emailLine,"menu":[]}}"""
            }

            fun response(request: Int, status: Int, body: String, personId: String? = null): JsonObject = buildJsonObject {
                put("t", JsonPrimitive("response"))
                put("request", JsonPrimitive(request))
                put("status", JsonPrimitive(status))
                put("content_type", JsonPrimitive("application/vnd.navree.native+json"))
                if (personId != null) {
                    put("headers", JsonArray(listOf(JsonArray(listOf(JsonPrimitive("x-navree-person"), JsonPrimitive(personId))))))
                }
                put("body", JsonPrimitive(body))
            }

            Core(config).use { core ->
                var requestSeq = 0
                val engine = object : Engine {
                    override fun send(event: ByteArray): ByteArray = core.send(event)
                    override fun view(query: ByteArray): ByteArray = core.view(query)
                }
                val driver = Driver(engine)
                driver.effects = EffectRunner { effect ->
                    if (effect is Effect.Fetch && effect.url.endsWith("/_navree/native/shell")) {
                        requestSeq = effect.request.toInt()
                    }
                }
                driver.person = { user ->
                    if (user != null) {
                        val word = watch.hear(user.key, user.name, user.email)
                        if (word.heard == Heard.Another) watch.changed(word) else if (word.write) watch.keep(word)
                        heard += word.heard
                        marks += word.mark
                    }
                }

                // 1. Named with an id: the person receives the id's key and mark.
                driver.send(Events.start)
                driver.send(response(requestSeq, 200, shellBody("Harpreet Sandhu", "harpreet.sandhu@example.com"), "0193a4b2-0000-7000-8000-00000000hs01"))
                val idKey = driver.app?.user?.key
                assertTrue("id key is present", !idKey.isNullOrEmpty())
                assertEquals(listOf(Heard.First), heard)
                val idMark = marks.single()
                assertEquals(PersonWatch.markOfKey(idKey!!), idMark)

                // 2. Renamed under the same id: one key and one mark stay.
                driver.send(Events.start)
                driver.send(response(requestSeq, 200, shellBody("Harpreet S. Sandhu", "harpreet@example.com"), "0193a4b2-0000-7000-8000-00000000hs01"))
                assertEquals(idKey, driver.app?.user?.key)
                assertEquals(Heard.Same, heard.last())
                assertEquals(idMark, marks.last())

                // 3. Under a sign-in cover nobody is named; the same id returning keeps the same mark.
                driver.send(Events.start)
                driver.send(response(requestSeq, 401, """{"navree":1,"build":"1","vocab":0,"ops":[{"op":"navigate","href":"/sign-in"}]}"""))
                assertNull("nobody is named under a sign-in cover", driver.app?.user)
                driver.send(Events.start)
                driver.send(response(requestSeq, 200, shellBody("Harpreet Sandhu", "harpreet.sandhu@example.com"), "0193a4b2-0000-7000-8000-00000000hs01"))
                assertEquals(idKey, driver.app?.user?.key)
                assertEquals(Heard.Same, heard.last())

                // 4. An answer without an id is distinct from the id-bearing account, then stays steady while id-less.
                driver.send(Events.start)
                driver.send(response(requestSeq, 200, shellBody("Harpreet Sandhu", "harpreet.sandhu@example.com"), null))
                val nameKey = driver.app?.user?.key
                assertTrue("name-and-email key is present", !nameKey.isNullOrEmpty())
                assertNotEquals("id and id-less answers never share a key", idKey, nameKey)
                assertEquals(Heard.Another, heard.last())
                val nameMark = marks.last()
                assertEquals(PersonWatch.markOfKey(nameKey!!), nameMark)

                driver.send(Events.start)
                driver.send(response(requestSeq, 200, shellBody("Harpreet Sandhu", "harpreet.sandhu@example.com"), null))
                assertEquals(nameKey, driver.app?.user?.key)
                assertEquals(Heard.Same, heard.last())
                assertEquals(nameMark, marks.last())

                // 5. When the server sends the id again, the id's key and mark start cleanly and match a fresh launch.
                driver.send(Events.start)
                driver.send(response(requestSeq, 200, shellBody("Harpreet Sandhu", "harpreet.sandhu@example.com"), "0193a4b2-0000-7000-8000-00000000hs01"))
                assertEquals(idKey, driver.app?.user?.key)
                assertEquals(Heard.Another, heard.last())
                assertEquals(idMark, marks.last())

                driver.send(Events.start)
                driver.send(response(requestSeq, 200, shellBody("Harpreet Sandhu", "harpreet.sandhu@example.com"), "0193a4b2-0000-7000-8000-00000000hs01"))
                assertEquals(Heard.Same, heard.last())
                assertEquals(idMark, marks.last())
            }
        } finally {
            folder.deleteRecursively()
        }
    }

    private fun replayTranscript(assets: android.content.res.AssetManager, name: String) {
        val text = assets.open("transcripts/$name").use { it.readBytes().decodeToString() }
        val root = Json.parseToJsonElement(text).jsonObject
        val configBytes = transcriptConfig(root["config"]).toString().toByteArray()
        var core = Core(configBytes)
        var lastUpdate: JsonElement = JsonNull
        val problems = ArrayList<String>()
        val unknownEffects = ArrayList<Effect>()

        fun makeDriver(current: Core): Driver {
            val engine = object : Engine {
                override fun send(event: ByteArray): ByteArray {
                    val out = current.send(event)
                    lastUpdate = Json.parseToJsonElement(String(out))
                    return out
                }

                override fun view(query: ByteArray): ByteArray = current.view(query)
            }
            return Driver(engine).apply {
                this.problems = { problems += it }
                this.effects = EffectRunner { if (it is Effect.Unknown) unknownEffects += it }
            }
        }

        var driver = makeDriver(core)
        var keptSnapshot: JsonElement = JsonNull
        try {
            val steps = root.getValue("steps").jsonArray
            steps.forEachIndexed { index, rawStep ->
                val step = rawStep.jsonObject
                val where = "$name step ${index + 1}"
                if (step["restart"]?.jsonPrimitive?.booleanOrNull == true) {
                    core.close()
                    core = Core(configBytes)
                    driver = makeDriver(core)
                }
                var send = step.getValue("send").jsonObject
                if (step["last"]?.jsonPrimitive?.booleanOrNull == true) {
                    assertEquals("$where: restored snapshot matches last snapshot", keptSnapshot, send["snapshot"] ?: JsonNull)
                }
                val bodyPath = step["body"]?.jsonPrimitive?.content
                if (bodyPath != null) {
                    val fixture = assets.open("transcripts/fixtures/$bodyPath").use { it.readBytes().decodeToString() }
                    val tag = send.getValue("t").jsonPrimitive.content
                    val field = when (tag) {
                        "response" -> "body"
                        "cache" -> "value"
                        else -> throw AssertionError("$where: unexpected body on event $tag")
                    }
                    send = JsonObject(send + (field to JsonPrimitive(fixture)))
                }
                driver.send(send)
                assertFalse("$where: core stopped", driver.stopped)
                assertEquals("$where: driver decode problems", emptyList<String>(), problems)
                assertEquals("$where: unknown effects", emptyList<Effect>(), unknownEffects)

                val expect = step["expect"]
                if (expect != null) {
                    assertEquals("$where: update", expect, lastUpdate)
                }
                val checks = step["check"]?.jsonArray.orEmpty()
                if (checks.isNotEmpty()) {
                    val whole = Json.parseToJsonElement(String(core.view(ByteArray(0))))
                    for (item in checks) {
                        val check = item.jsonObject
                        val at = check.getValue("at").jsonPrimitive.content
                        val wanted = check.getValue("is")
                        val actual = pointer(whole, at) ?: JsonNull
                        assertEquals("$where: pointer $at", wanted, actual)
                    }
                }
                val expectedSnapshot = step["snapshot"]
                if (expectedSnapshot != null) {
                    keptSnapshot = Json.parseToJsonElement(String(core.view("""{"part":"snapshot"}""".toByteArray())))
                    assertEquals("$where: snapshot", expectedSnapshot, keptSnapshot)
                }
            }
        } finally {
            core.close()
        }
    }

    private fun transcriptConfig(over: JsonElement?): JsonObject {
        val base = LinkedHashMap<String, JsonElement>()
        base["origin"] = JsonPrimitive("https://shop.example")
        base["client"] = buildJsonObject {
            put("platform", JsonPrimitive("android"))
            put("form", JsonPrimitive("phone"))
            put("vocab", JsonPrimitive(0))
            put("text_percent", JsonPrimitive(100))
        }
        if (over is JsonObject) {
            for ((key, value) in over) base[key] = value
        }
        return JsonObject(base)
    }

    private fun pointer(root: JsonElement, path: String): JsonElement? {
        if (path.isEmpty()) return root
        var current: JsonElement? = root
        for (raw in path.removePrefix("/").split('/')) {
            val part = raw.replace("~1", "/").replace("~0", "~")
            current = when (val node = current) {
                is JsonObject -> node[part]
                is JsonArray -> part.toIntOrNull()?.let { node.getOrNull(it) }
                else -> null
            }
            if (current == null) return null
        }
        return current
    }
}
