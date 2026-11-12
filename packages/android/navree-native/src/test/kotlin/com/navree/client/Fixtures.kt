package com.navree.client

import java.io.File

/** The example envelopes, read from the test classpath. */
object Fixtures {
    fun text(path: String): String =
        requireNotNull(Fixtures::class.java.getResource("/$path")) { "No fixture at $path" }.readText()

    /** The names of every file in a directory of fixtures. */
    fun names(directory: String): List<String> {
        val url = requireNotNull(Fixtures::class.java.getResource("/$directory")) { "No fixtures in $directory" }
        return File(url.toURI()).listFiles().orEmpty().filter { it.name.endsWith(".json") }.map { it.name }.sorted()
    }

    fun envelope(name: String): String = text("envelopes/$name.json")
}
