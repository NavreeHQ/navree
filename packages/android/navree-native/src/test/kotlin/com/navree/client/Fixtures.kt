package com.navree.client

import com.navree.protocol.Navree
import java.io.File

/**
 * Answers to read in tests, from the test classpath: the protocol's own
 * examples (`crates/navree-native/corpus`, read where the protocol keeps them)
 * and real answers recorded from the store demo (`fixtures/store-demo`).
 */
object Fixtures {
    fun text(path: String): String =
        requireNotNull(Fixtures::class.java.getResource("/$path")) { "No fixture at $path" }.readText()

    /** The JSON files beside a known one, by name. */
    private fun beside(known: String): List<String> {
        val url = requireNotNull(Fixtures::class.java.getResource("/$known")) { "No fixture $known" }
        return File(url.toURI()).parentFile.listFiles().orEmpty().filter { it.name.endsWith(".json") }.map { it.name }.sorted()
    }

    /** The protocol's example answers, by file name. */
    fun corpus(): List<String> = beside("from-the-future.json")

    /** The store demo's recorded answers, by file name. */
    fun store(): List<String> = beside("store-demo/orders.json")

    fun envelope(path: String): Navree.Envelope = Navree.json.decodeFromString(Navree.Envelope.serializer(), text(path))

    fun screen(path: String): Navree.Screen = requireNotNull(envelope(path).screen) { "$path has no screen" }
}
