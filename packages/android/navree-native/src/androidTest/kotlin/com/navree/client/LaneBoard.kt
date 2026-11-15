package com.navree.client

import org.json.JSONArray
import org.json.JSONObject
import java.net.URLDecoder

/** The recorded board with a lane of as many cards as a test asks for, and what the server of such a board is told. */
internal object LaneBoard {
    /** The title of the card at this place of the first lane. */
    fun title(at: Int) = "Card $at of the first lane"

    /** What every card of the first lane says in its title, after its number. */
    const val TITLE_END = " of the first lane"

    /**
     * The recorded board with `cards` cards in its first lane. With `ticks`, each card also holds a form
     * with a switch ("Done"), which is not sent anywhere: a card that is ticked is a card that is ticked.
     */
    fun json(cards: Int, ticks: Boolean = false): String {
        val root = JSONObject(read("screen-board.json"))
        fun visit(node: Any?) {
            when (node) {
                is JSONObject -> {
                    if (node.optString("id") == "board-todo" && node.has("reorder")) {
                        val first = node.getJSONArray("children").getJSONObject(0)
                        val made = JSONArray()
                        for (at in 0 until cards) {
                            val copy = JSONObject(first.toString())
                            copy.put("id", "c-$at")
                            copy.put("key", "$at")
                            copy.put("title", title(at))
                            copy.getJSONObject("action").put("href", "/cards/c-$at")
                            if (ticks) copy.put("detail", JSONArray().put(tick(at)))
                            made.put(copy)
                        }
                        node.put("children", made)
                    }
                    node.keys().forEach { visit(node.get(it)) }
                }
                is JSONArray -> for (at in 0 until node.length()) visit(node.get(at))
            }
        }
        visit(root)
        return root.toString()
    }

    private fun tick(at: Int): JSONObject = JSONObject()
        .put("t", "form")
        .put("id", "tick-$at")
        .put("method", "post")
        .put("action", "/cards/tick")
        .put("hidden", JSONArray().put(JSONObject().put("name", "_csrf").put("value", "tok")))
        .put(
            "children",
            JSONArray().put(
                JSONObject()
                    .put("t", "field")
                    .put("name", "done")
                    .put("label", "Done")
                    .put("input", JSONObject().put("t", "toggle").put("on", false)),
            ),
        )

    /** The fields of a form that was posted, by name. */
    fun form(text: String): Map<String, String> = text.split('&').filter { it.isNotEmpty() }.associate { pair ->
        val parts = pair.split('=', limit = 2)
        URLDecoder.decode(parts[0], "UTF-8") to URLDecoder.decode(parts.getOrElse(1) { "" }, "UTF-8")
    }

    /** The test's own assets, by file name. */
    fun read(file: String): String =
        androidx.test.platform.app.InstrumentationRegistry.getInstrumentation().context.assets.open(file).use { it.readBytes().decodeToString() }
}
