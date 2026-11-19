package com.navree.client.core

import com.navree.protocol.Navree
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Assume.assumeTrue
import org.junit.Test
import java.io.File

/**
 * Every conversation recorded in the runtime core's `transcripts/` folder,
 * checked on the JVM against the client's decoders: every change and effect in
 * every expected update is one this build knows, and every referenced fixture
 * is present alongside the transcripts.
 */
class TranscriptsTest {
    private val folder = File("../../../crates/navree-native-core/transcripts")

    private val sixNew = listOf(
        "person-key-steady.json",
        "person-key-other-id.json",
        "session-ended-outside.json",
        "sign-in-cover.json",
        "sign-in-another-person.json",
        "sign-in-shell-fails.json",
    )

    @Test
    fun every_effect_and_fixture_of_the_cores_transcripts_decodes_in_the_client() {
        assumeTrue("the core's transcripts are beside the client", folder.isDirectory)
        val files = folder.listFiles { file -> file.isFile && file.extension == "json" }.orEmpty().sortedBy { it.name }
        assertTrue("transcripts found", files.isNotEmpty())
        val names = files.map { it.name }.toSet()
        for (expected in sixNew) {
            assertTrue("$expected is among the core's transcripts", expected in names)
        }
        val fixtures = HashSet<String>()
        for (file in files) {
            val root = Json.parseToJsonElement(file.readText()).jsonObject
            val steps = root.getValue("steps").jsonArray
            assertTrue("${file.name} has steps", steps.isNotEmpty())
            steps.forEachIndexed { index, element ->
                val step = element.jsonObject
                step["body"]?.jsonPrimitive?.content?.let { fixtures += it }
                val expect = step["expect"]?.jsonObject ?: return@forEachIndexed
                val changed = expect.getValue("changed").jsonArray
                for (change in changed) {
                    val tag = change.jsonObject.getValue("t").jsonPrimitive.content
                    assertTrue(
                        "${file.name} step ${index + 1}: unknown change $tag",
                        tag in setOf("app", "nav", "screen", "form", "rows", "redraw", "messages"),
                    )
                }
                val effects = expect.getValue("effects").jsonArray
                for (raw in effects) {
                    val decoded = Effect.read(raw)
                    assertFalse(
                        "${file.name} step ${index + 1}: unknown effect $raw",
                        decoded is Effect.Unknown,
                    )
                }
            }
        }
        val fixturesDir = File(folder, "fixtures")
        for (relative in fixtures.sorted()) {
            val fixture = File(fixturesDir, relative)
            assertTrue("missing transcript fixture $relative", fixture.isFile)
            if (relative.startsWith("store/") && relative.endsWith(".json") && relative != "store/shell.json") {
                val envelope = Navree.json.decodeFromString(Navree.Envelope.serializer(), fixture.readText())
                assertTrue(
                    "$relative decodes as a non-empty envelope",
                    envelope.screen != null || envelope.web != null || envelope.ops.isNotEmpty(),
                )
            }
        }
    }
}
