package com.navree.client.core

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.booleanOrNull
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.longOrNull
import kotlinx.serialization.json.put

/** The core as the driver needs it: JSON in, JSON out. [com.navree.core.Core] in the app; a stand-in in tests. */
interface Engine {
    fun send(event: ByteArray): ByteArray

    fun view(query: ByteArray): ByteArray
}

/** Carries out what the core asks. Whatever comes of an effect is a later event, given to [Driver.send]. */
fun interface EffectRunner {
    fun run(effect: Effect)
}

/**
 * What a core that has stopped answers every call with, word for word (`STOPPED`
 * in the core's `ffi.rs`). The client reads the answer's `stopped` mark; the
 * words are how a core built before the mark says the same.
 */
internal const val CORE_STOPPED = "the core stopped after a failure inside it and can't be used again"

/**
 * The loop of the runtime core: one event in, the parts that changed read
 * back into state the interface observes, then each effect carried out in
 * order. Call [send] from the main thread only.
 *
 * The client keeps no state of its own about navigation, trees or forms: what
 * is here is the core's view state, part by part, as it last said it.
 */
class Driver(private val engine: Engine) {
    var effects: EffectRunner = EffectRunner {}

    /** Failures of the core itself: an event that did not read, a part that did not decode. */
    var problems: (String) -> Unit = {}

    var app: AppPart? by mutableStateOf(null)
        private set

    var nav: NavPart by mutableStateOf(NavPart())
        private set

    /** Every screen some stack holds, by number. */
    val screens = mutableStateMapOf<Long, ScreenView>()

    var messages: List<Message> by mutableStateOf(emptyList())
        private set

    /**
     * The core stopped after a failure inside it: it answers nothing more, and
     * the app can only be started again. Set by the answer that says so, to an
     * event or to a view (its `stopped` mark, or the core's own words for it);
     * from then on nothing is sent to the core.
     */
    var stopped: Boolean by mutableStateOf(false)
        private set

    private var sending = false
    private val queued = ArrayDeque<JsonObject>()

    /**
     * Tells the core one thing that happened. An event sent while another is
     * being handled (an effect answered at once) waits its turn, so the core
     * always sees one event at a time, in order.
     */
    fun send(event: JsonObject) {
        // A core that stopped answers every call the same: nothing more is asked of it.
        if (stopped) return
        queued.addLast(event)
        if (sending) return
        sending = true
        try {
            while (queued.isNotEmpty() && !stopped) handle(queued.removeFirst())
        } finally {
            if (stopped) queued.clear()
            sending = false
        }
    }

    /** Reads every part again: at start, and after the interface was made again from nothing. */
    fun readAll() {
        val all = query(JsonObject(emptyMap())) as? JsonObject ?: return
        app = all["app"]?.let { decode(AppPart.serializer(), it) }
        nav = decode(NavPart.serializer(), all) ?: nav
        val kept = (all["screens"] as? JsonObject).orEmpty()
        screens.clear()
        for ((_, screen) in kept) decode(ScreenView.serializer(), screen)?.let { screens[it.id] = it }
        messages = all["messages"]?.let { decode(kotlinx.serialization.builtins.ListSerializer(Message.serializer()), it) }.orEmpty()
    }

    private fun handle(event: JsonObject) {
        val answer = runCatching { engine.send(event.toString().encodeToByteArray()) }
            .getOrElse { problems("the core failed: ${it.message}"); return }
        val update = runCatching { CoreJson.parseToJsonElement(answer.decodeToString()) as? JsonObject }.getOrNull()
        if (update == null) {
            problems("the core answered what does not read")
            return
        }
        if (stops(update)) {
            halt()
            return
        }
        (update["error"] as? JsonPrimitive)?.contentOrNull?.let {
            problems("the core refused ${event["t"]}: $it")
            return
        }
        for (change in (update["changed"] as? JsonArray).orEmpty()) {
            refresh(change as? JsonObject ?: continue)
            // A view found the core stopped: the rest of this answer is the answer of a core that is gone.
            if (stopped) return
        }
        for (effect in (update["effects"] as? JsonArray).orEmpty()) carryOut(Effect.read(effect))
    }

    /** Whether an answer of the core says it has stopped for good: the mark, or the words a core without the mark says it in. */
    private fun stops(answer: JsonObject): Boolean =
        (answer["stopped"] as? JsonPrimitive)?.booleanOrNull == true || (answer["error"] as? JsonPrimitive)?.contentOrNull == CORE_STOPPED

    private fun halt() {
        if (stopped) return
        stopped = true
        queued.clear()
        problems("the core stopped after a failure inside it: the app must be started again")
    }

    /**
     * Carries out one effect. One that fails, in any way (an error of the
     * system included, such as running out of memory for one picture or one
     * answer), is reported and the rest go on: no single effect, from a page
     * the core keeps or from a live answer, ends the app. What failed is named
     * by its kind only, never by what it held.
     */
    private fun carryOut(effect: Effect) {
        try {
            effects.run(effect)
        } catch (e: Throwable) {
            problems("an effect failed: ${effect::class.simpleName}: ${e::class.simpleName}")
        }
    }

    private fun refresh(change: JsonObject) {
        fun s(name: String) = (change[name] as? JsonPrimitive)?.contentOrNull
        fun l(name: String) = (change[name] as? JsonPrimitive)?.longOrNull
        when (s("t")) {
            "app" -> app = query(part("app"))?.let { decode(AppPart.serializer(), it) }
            "nav" -> {
                query(part("nav"))?.let { decode(NavPart.serializer(), it) }?.let { nav = it }
                // A screen in no stack is gone.
                val held = nav.held()
                screens.keys.retainAll(held)
            }
            "screen" -> l("id")?.let(::readScreen)
            "form" -> {
                val screen = l("screen") ?: return
                val form = s("form") ?: return
                val view = query(
                    buildJsonObject {
                        put("part", "form")
                        put("screen", screen)
                        put("form", form)
                    },
                )?.let { decode(FormView.serializer(), it) }
                val current = screens[screen] ?: return readScreen(screen)
                screens[screen] = current.copy(forms = if (view == null) current.forms - form else current.forms + (form to view))
            }
            // New rows, or rows named again: the whole screen read again is always right.
            "rows", "redraw" -> l("screen")?.let(::readScreen)
            "messages" -> messages = query(part("messages"))
                ?.let { decode(kotlinx.serialization.builtins.ListSerializer(Message.serializer()), it) }.orEmpty()
            else -> readAll()
        }
    }

    /**
     * What the core keeps for a launch after the system ended the app, as
     * JSON text to store as it is, or null when there is nothing to keep.
     * A snapshot that says `person_unknown` (the app's shell has not yet named
     * anybody in this launch, so it holds nothing of a person) is nothing to
     * keep: the one already stored stays. A core that does not send the field
     * sends snapshots that are always kept.
     */
    fun snapshot(): String? {
        val snapshot = query(part("snapshot")) ?: return null
        val unknown = ((snapshot as? JsonObject)?.get("person_unknown") as? JsonPrimitive)?.contentOrNull == "true"
        return if (unknown) null else snapshot.toString()
    }

    private fun readScreen(id: Long) {
        val view = query(
            buildJsonObject {
                put("part", "screen")
                put("id", id)
            },
        )?.let { decode(ScreenView.serializer(), it) }
        if (view == null) screens.remove(id) else screens[id] = view
    }

    private fun part(name: String) = buildJsonObject { put("part", name) }

    private fun query(query: JsonObject): JsonElement? {
        val bytes = runCatching { engine.view(if (query.isEmpty()) ByteArray(0) else query.toString().encodeToByteArray()) }
            .getOrElse { problems("the core failed to answer a view: ${it.message}"); return null }
        val answer = runCatching { CoreJson.parseToJsonElement(bytes.decodeToString()) }.getOrNull() ?: return null
        if (answer is JsonNull) return null
        // An answer that carries an `error` is the core's refusal whatever else it holds: no part of the view state has that key.
        if (answer is JsonObject && answer["error"] != null) {
            if (stops(answer)) halt() else problems("the core refused a view: ${answer["error"]}")
            return null
        }
        return answer
    }

    private fun <T> decode(serializer: kotlinx.serialization.KSerializer<T>, element: JsonElement): T? =
        runCatching { CoreJson.decodeFromJsonElement(serializer, element) }
            .onFailure { problems("a part of the view state did not decode: ${it.message}") }
            .getOrNull()
}
