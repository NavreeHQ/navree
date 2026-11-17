package com.navree.client.core

import com.navree.client.Device
import com.navree.client.NavreeConfig
import com.navree.client.coreConfig
import com.navree.client.webViewToken
import com.navree.protocol.Navree
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/** A stand-in for the core: answers each event with what the test gives it, and each view from a table. */
private class Scripted(val answers: ArrayDeque<String>, val views: Map<String, String>) : Engine {
    val sent = ArrayList<JsonObject>()
    val asked = ArrayList<String>()

    override fun send(event: ByteArray): ByteArray {
        sent += Json.parseToJsonElement(event.decodeToString()).jsonObject
        return (answers.removeFirstOrNull() ?: """{"changed":[],"effects":[]}""").encodeToByteArray()
    }

    override fun view(query: ByteArray): ByteArray {
        val text = query.decodeToString()
        asked += text
        val part = if (text.isEmpty()) "all" else Json.parseToJsonElement(text).jsonObject["part"]!!.jsonPrimitive.content
        return (views[part] ?: "null").encodeToByteArray()
    }
}

class DriverTest {
    private val screen = """{"id":1,"url":"/orders","content":{"t":"native","screen":{"title":"Orders","body":{"t":"text","value":"Packed"}}},
        "loading":false,"refreshing":false,"cached":false,"status":200,"chrome":true,"overlays":[],"states":{},
        "forms":{"f":{"pending":false,"fields":{"f.q":{"name":"q","value":{"t":"text","text":"lota"},"errors":[]}}}},"regions":{},"lists":{}}"""

    private val nav = """{"stage":"ready","destinations":[{"label":"Orders","href":"/orders","items":[],"stack":[1]}],"selected":0,"modals":[],"front":1,"can_go_back":false,"window":{"width":"compact"}}"""

    @Test
    fun each_changed_part_is_read_back_and_then_the_effects_run_in_order() {
        val engine = Scripted(
            ArrayDeque(listOf("""{"changed":[{"t":"nav"},{"t":"screen","id":1}],"effects":[{"t":"haptic","kind":"success"},{"t":"timer","timer":2,"ms":100}]}""")),
            mapOf("nav" to nav, "screen" to screen),
        )
        val driver = Driver(engine)
        val ran = ArrayList<Effect>()
        driver.effects = EffectRunner { ran += it }
        driver.send(Events.start)

        assertEquals(Stage.Ready, driver.nav.stage)
        assertEquals(listOf(1L), driver.nav.destinations.single().stack)
        val shown = driver.screens[1]!!
        assertEquals("Orders", shown.screen!!.title)
        assertEquals(FieldValue.Text("lota"), shown.field("f", "f.q")!!.value)
        assertEquals(listOf(Effect.Haptic("success"), Effect.Timer(2, 100)), ran)
    }

    @Test
    fun a_screen_in_no_stack_is_forgotten() {
        val engine = Scripted(
            ArrayDeque(listOf("""{"changed":[{"t":"nav"},{"t":"screen","id":1}],"effects":[]}""", """{"changed":[{"t":"nav"}],"effects":[]}""")),
            mapOf("nav" to nav, "screen" to screen),
        )
        val driver = Driver(engine)
        driver.send(Events.start)
        assertTrue(1L in driver.screens)
        val emptied = Scripted(ArrayDeque(listOf("""{"changed":[{"t":"nav"}],"effects":[]}""")), mapOf("nav" to nav.replace("\"stack\":[1]", "\"stack\":[]")))
        val second = Driver(emptied)
        second.screens[1] = driver.screens[1]!!
        second.send(Events.back)
        assertNull(second.screens[1])
    }

    @Test
    fun who_the_apps_part_names_is_said_when_the_part_is_read_back_before_any_effect_of_the_same_update() {
        val named = """{"name":"Charkha Supply Co.","home":"/","user":{"name":"Ravneet Kaur","email":"ravneet.kaur@example.com"}}"""
        val engine = Scripted(ArrayDeque(listOf("""{"changed":[{"t":"app"}],"effects":[{"t":"cache_clear"},{"t":"haptic","kind":"success"}]}""")), mapOf("app" to named))
        val driver = Driver(engine)
        val order = ArrayList<String>()
        driver.person = { user -> order += "the app names " + user?.name + ", " + user?.email }
        driver.effects = EffectRunner { effect -> order += "an effect: " + effect::class.simpleName }
        driver.send(Events.start)
        assertEquals(listOf("the app names Ravneet Kaur, ravneet.kaur@example.com", "an effect: CacheClear", "an effect: Haptic"), order)

        // An app part that names nobody is said too, as nobody; and a failure where it is heard is reported and the effects still run.
        val nobody = Scripted(ArrayDeque(listOf("""{"changed":[{"t":"app"}],"effects":[{"t":"cache_clear"}]}""")), mapOf("app" to """{"name":"Charkha Supply Co.","home":"/"}"""))
        val second = Driver(nobody)
        val problems = ArrayList<String>()
        val ran = ArrayList<Effect>()
        var heard = "not said"
        second.problems = { problems += it }
        second.person = { user ->
            heard = user?.name ?: "nobody"
            throw IllegalStateException("the disk is full")
        }
        second.effects = EffectRunner { ran += it }
        second.send(Events.start)
        assertEquals("nobody", heard)
        assertEquals(listOf<Effect>(Effect.CacheClear), ran)
        assertEquals(1, problems.size)
        assertTrue("named by its kind, not by what it held", "IllegalStateException" in problems.single() && "disk" !in problems.single())
    }

    @Test
    fun the_person_the_app_names_carries_the_cores_key_where_the_core_gives_one_and_what_this_build_does_not_know_is_passed_over() {
        // A core that gives a key, and says more of a person than this build knows.
        val keyed = """{"name":"Charkha Supply Co.","home":"/","user":{"name":"Harpreet Singh","key":"p2","since":"2026-11-17","roles":["staff"]}}"""
        val driver = Driver(Scripted(ArrayDeque(listOf("""{"changed":[{"t":"app"}],"effects":[]}""")), mapOf("app" to keyed)))
        val problems = ArrayList<String>()
        driver.problems = { problems += it }
        var said: Navree.User? = null
        driver.person = { said = it }
        driver.send(Events.start)
        assertEquals("p2", said?.key)
        assertEquals("Harpreet Singh", said?.name)
        assertNull(said?.email)
        assertEquals("nothing failed to decode", emptyList<String>(), problems)

        // A core that gives none: the field is not there, and the person is named all the same.
        val plain = Driver(Scripted(ArrayDeque(listOf("""{"changed":[{"t":"app"}],"effects":[]}""")), mapOf("app" to """{"name":"Charkha Supply Co.","home":"/","user":{"name":"Harpreet Singh"}}""")))
        var without: Navree.User? = null
        plain.person = { without = it }
        plain.send(Events.start)
        assertEquals("Harpreet Singh", without?.name)
        assertNull(without?.key)
    }

    @Test
    fun an_event_sent_while_one_is_handled_waits_its_turn() {
        val engine = Scripted(ArrayDeque(listOf("""{"changed":[],"effects":[{"t":"cache_read","key":"boot"}]}""")), emptyMap())
        val driver = Driver(engine)
        driver.effects = EffectRunner { effect ->
            if (effect is Effect.CacheRead) driver.send(Events.cache(effect.key, null))
            // The core has not been asked again yet: the first event is still being handled.
            assertEquals(1, engine.sent.size)
        }
        driver.send(Events.start)
        assertEquals(listOf("start", "cache"), engine.sent.map { it["t"]!!.jsonPrimitive.content })
    }

    @Test
    fun an_effect_that_fails_is_reported_and_the_effects_after_it_still_run() {
        val engine = Scripted(
            ArrayDeque(listOf("""{"changed":[],"effects":[{"t":"fetch","request":1,"method":"get","url":"https://shop.example/","headers":[["x-navree-region","बिक्री"]]},{"t":"haptic","kind":"success"}]}""")),
            emptyMap(),
        )
        val driver = Driver(engine)
        val problems = ArrayList<String>()
        driver.problems = { problems += it }
        val ran = ArrayList<Effect>()
        driver.effects = EffectRunner { effect ->
            if (effect is Effect.Fetch) throw IllegalArgumentException("Unexpected char")
            ran += effect
        }
        driver.send(Events.start)
        assertEquals(listOf(Effect.Haptic("success")), ran)
        assertEquals(1, problems.size)
        assertTrue("named by its kind, not by what it held", "Fetch" in problems.single() && "बिक्री" !in problems.single())
    }

    @Test
    fun an_effect_that_runs_out_of_memory_is_reported_and_the_rest_still_run() {
        val engine = Scripted(ArrayDeque(listOf("""{"changed":[],"effects":[{"t":"copy","text":"CS-220480"},{"t":"haptic","kind":"success"}]}""")), emptyMap())
        val driver = Driver(engine)
        val problems = ArrayList<String>()
        driver.problems = { problems += it }
        val ran = ArrayList<Effect>()
        driver.effects = EffectRunner { effect ->
            if (effect is Effect.Copy) throw OutOfMemoryError("Failed to allocate")
            ran += effect
        }
        driver.send(Events.start)
        assertEquals(listOf(Effect.Haptic("success")), ran)
        assertEquals(1, problems.size)
    }

    @Test
    fun a_core_that_refuses_an_event_changes_nothing() {
        val engine = Scripted(ArrayDeque(listOf("""{"error":"the event does not read"}""")), emptyMap())
        val driver = Driver(engine)
        val problems = ArrayList<String>()
        driver.problems = { problems += it }
        driver.send(Events.start)
        assertEquals(Stage.Starting, driver.nav.stage)
        assertEquals(1, problems.size)
    }

    @Test
    fun an_event_answered_with_an_error_is_refused_whatever_the_error_is_written_as_and_whatever_else_the_answer_holds() {
        for (error in listOf(""""the event does not read"""", """{"what":"the event does not read"}""", "null", "7")) {
            val engine = Scripted(ArrayDeque(listOf("""{"error":$error,"changed":[{"part":"nav"}],"effects":[{"t":"exit"}]}""")), emptyMap())
            val driver = Driver(engine)
            val problems = ArrayList<String>()
            val carried = ArrayList<Effect>()
            driver.problems = { problems += it }
            driver.effects = EffectRunner { carried += it }
            driver.send(Events.start)
            assertEquals("no effect of a refused answer is carried out: $error", emptyList<Effect>(), carried)
            assertEquals("and no part of the view is asked for: $error", emptyList<String>(), engine.asked)
            assertEquals(error, 1, problems.size)
        }
    }

    @Test
    fun a_view_answered_with_an_error_is_refused_whatever_else_the_answer_holds() {
        // The answer reads as a place to be in, and says it is an error: it is the error.
        val refused = """{"error":"the query does not read: missing field","stage":"ready","destinations":[{"label":"Orders","href":"/orders","items":[],"stack":[1]}],"front":1}"""
        val engine = Scripted(ArrayDeque(listOf("""{"changed":[{"t":"nav"}],"effects":[]}""")), mapOf("nav" to refused))
        val driver = Driver(engine)
        val problems = ArrayList<String>()
        driver.problems = { problems += it }
        driver.send(Events.start)
        assertEquals("nothing of the refused answer is drawn", Stage.Starting, driver.nav.stage)
        assertTrue(driver.nav.destinations.isEmpty())
        assertTrue("the refusal is reported: $problems", problems.any { "refused a view" in it })
    }

    private val stoppedAnswer = """{"error":"the core stopped after a failure inside it and can't be used again","stopped":true}"""

    @Test
    fun a_core_that_says_it_has_stopped_is_asked_nothing_more() {
        val engine = Scripted(ArrayDeque(listOf(stoppedAnswer)), emptyMap())
        val driver = Driver(engine)
        val ran = ArrayList<Effect>()
        driver.effects = EffectRunner { ran += it }
        assertFalse(driver.stopped)
        driver.send(Events.start)
        assertTrue("the mark is read", driver.stopped)
        driver.send(Events.back)
        driver.send(Events.foreground)
        assertEquals("no event is sent to a core that stopped", 1, engine.sent.size)
        assertTrue(ran.isEmpty())
    }

    @Test
    fun a_stop_is_read_from_a_view_as_from_an_event_and_from_the_cores_words_where_the_mark_is_missing() {
        // A view answered by a core that stopped meanwhile.
        val viewed = Driver(Scripted(ArrayDeque(listOf("""{"changed":[{"t":"nav"}],"effects":[{"t":"haptic","kind":"success"}]}""")), mapOf("nav" to stoppedAnswer)))
        val ran = ArrayList<Effect>()
        viewed.effects = EffectRunner { ran += it }
        viewed.send(Events.start)
        assertTrue(viewed.stopped)
        assertEquals("nothing of the answer the stop came in is drawn", Stage.Starting, viewed.nav.stage)
        assertTrue("nor are the effects that came with it carried out", ran.isEmpty())
        // A core built before the mark says it in its words alone.
        val worded = Driver(Scripted(ArrayDeque(listOf("""{"error":"$CORE_STOPPED"}""")), emptyMap()))
        worded.send(Events.start)
        assertTrue(worded.stopped)
        // An input that merely does not read is only an error: the core goes on.
        val refusing = Scripted(ArrayDeque(listOf("""{"error":"the event does not read: unknown variant"}""")), emptyMap())
        val going = Driver(refusing)
        going.send(Events.start)
        going.send(Events.back)
        assertFalse(going.stopped)
        assertEquals(2, refusing.sent.size)
    }

    @Test
    fun the_words_a_stopped_core_is_known_by_are_the_cores_own() {
        // The unit tests run in the client's own folder; the core's source is three folders up in the same tree.
        val source = java.io.File("../../../crates/navree-native-core/src/ffi.rs")
        org.junit.Assume.assumeTrue("the core's source is beside the client", source.isFile)
        val words = Regex("const STOPPED: &str =\\s*\"([^\"]+)\"").find(source.readText())?.groupValues?.get(1)
        assertEquals(words, CORE_STOPPED)
    }

    @Test
    fun a_notice_is_read_by_the_cores_word_and_one_this_build_does_not_know_is_still_a_notice() {
        val messages = """[{"id":1,"tone":"warning","notice":"saved_not_refreshed"},{"id":2,"tone":"danger","notice":"said_by_a_newer_core"},{"id":3,"tone":"neutral","text":"Packed"},{"id":4,"tone":"danger","notice":"failed"}]"""
        val driver = Driver(Scripted(ArrayDeque(listOf("""{"changed":[{"t":"messages"}],"effects":[]}""")), mapOf("messages" to messages)))
        driver.send(Events.start)
        assertEquals(
            listOf(Notice.SavedNotRefreshed, Notice.Unknown, null, Notice.Failed),
            driver.messages.map { it.notice },
        )
        assertEquals("a message with words of its own has no notice", "Packed", driver.messages[2].text)
        // An announcement names its notice the same way.
        assertEquals(Effect.Announce(null, Notice.Loaded, 20), Effect.read(Json.parseToJsonElement("""{"t":"announce","notice":"loaded","count":20}""")))
        assertEquals(Notice.Unknown, (Effect.read(Json.parseToJsonElement("""{"t":"announce","notice":"said_by_a_newer_core"}""")) as Effect.Announce).notice)
    }

    @Test
    fun every_notice_the_core_can_say_has_a_word_this_build_knows() {
        val known = Notice.entries.filter { it != Notice.Unknown }.map { it.wire }
        for (word in known) assertEquals(word, Notice.read(word).wire)
        assertEquals(Notice.Unknown, Notice.read("said_by_a_newer_core"))
        // The unit tests run in the client's own folder; the core's source is three folders up in the same tree.
        val source = java.io.File("../../../crates/navree-native-core/src/schema.rs")
        org.junit.Assume.assumeTrue("the core's source is beside the client", source.isFile)
        val list = Regex("fn notice\\(\\) -> Value \\{\\s*words\\(&\\[(.*?)\\]\\)", RegexOption.DOT_MATCHES_ALL).find(source.readText())?.groupValues?.get(1)
        val said = Regex("\"([a-z_]+)\"").findAll(list.orEmpty()).map { it.groupValues[1] }.toList()
        assertTrue("the core's list of notices was found: $said", said.size >= 10)
        assertEquals("the core's notices, each known to this build by its word", said.sorted(), known.sorted())
    }

    @Test
    fun rows_named_again_read_their_screen_again() {
        val engine = Scripted(
            ArrayDeque(listOf("""{"changed":[{"t":"nav"}],"effects":[]}""", """{"changed":[{"t":"redraw","screen":1,"id":"inbox","rows":["c-88"],"gone":[]}],"effects":[]}""")),
            mapOf("nav" to nav, "screen" to screen),
        )
        val driver = Driver(engine)
        driver.send(Events.start)
        assertNull(driver.screens[1])
        driver.send(Events.refresh(1))
        assertEquals("Orders", driver.screens[1]!!.screen!!.title)
    }

    @Test
    fun a_snapshot_taken_before_the_shell_named_anybody_is_not_kept() {
        val unknown = Driver(Scripted(ArrayDeque(), mapOf("snapshot" to """{"version":1,"screens":[],"person_unknown":true}""")))
        assertNull("the stored snapshot stays", unknown.snapshot())
        val known = Driver(Scripted(ArrayDeque(), mapOf("snapshot" to """{"version":1,"screens":[],"person_unknown":false}""")))
        assertEquals("""{"version":1,"screens":[],"person_unknown":false}""", known.snapshot())
    }

    @Test
    fun the_snapshot_is_handed_over_as_the_core_wrote_it() {
        val driver = Driver(Scripted(ArrayDeque(), mapOf("snapshot" to """{"version":1,"screens":[]}""")))
        assertEquals("""{"version":1,"screens":[]}""", driver.snapshot())
        assertNull(Driver(Scripted(ArrayDeque(), emptyMap())).snapshot())
    }

    @Test
    fun a_form_change_replaces_only_that_form() {
        val form = """{"pending":true,"fields":{"f.q":{"name":"q","value":{"t":"text","text":"lotas"},"errors":["Too long"]}}}"""
        val engine = Scripted(
            ArrayDeque(listOf("""{"changed":[{"t":"nav"},{"t":"screen","id":1}],"effects":[]}""", """{"changed":[{"t":"form","screen":1,"form":"f"}],"effects":[]}""")),
            mapOf("nav" to nav, "screen" to screen, "form" to form),
        )
        val driver = Driver(engine)
        driver.send(Events.start)
        driver.send(Events.field(1, "f", "f.q", FieldValue.Text("lotas")))
        val held = driver.screens[1]!!.forms["f"]!!
        assertTrue(held.pending)
        assertEquals(listOf("Too long"), held.fields["f.q"]!!.errors)
    }
}

class EventsTest {
    @Test
    fun the_hand_over_push_tabs_and_a_restored_launch_take_the_cores_shapes() {
        assertEquals("""{"t":"random","request":3,"bytes":"0a1b"}""", Events.random(3, "0a1b").toString())
        assertEquals("""{"t":"push_token","token":"tok"}""", Events.pushToken("tok").toString())
        assertEquals("""{"t":"tab","screen":1,"id":"parts","index":1}""", Events.tab(1, "parts", 1).toString())
        assertEquals("""{"t":"restore","snapshot":{"v":1}}""", Events.restore(Json.parseToJsonElement("""{"v":1}""")).toString())
        assertEquals("""{"t":"choose","screen":1,"table":"ledger","on":true,"across":true}""", Events.choose(1, "ledger", null, true, across = true).toString())
        val tapped = Events.act(1, Navree.Action.Navigate("/sleep"), now = 0, from = "pillar-sleep")
        assertEquals("the part tapped, for the screen it opens to grow from", "pillar-sleep", tapped["from"]!!.jsonPrimitive.content)
    }

    @Test
    fun a_tap_carries_the_action_as_it_came_with_its_form_overlay_and_time() {
        val event = Events.act(3, Navree.Action.Submit(form = null), form = "refund", overlay = "refund", now = 1793990000)
        assertEquals("act", event["t"]!!.jsonPrimitive.content)
        assertEquals("submit", event["action"]!!.jsonObject["t"]!!.jsonPrimitive.content)
        assertEquals("refund", event["form"]!!.jsonPrimitive.content)
        assertEquals("1793990000", event["now"]!!.jsonPrimitive.content)
    }

    @Test
    fun field_values_take_the_cores_shapes() {
        assertEquals("""{"t":"text","text":"10"}""", FieldValue.Text("10").json().toString())
        assertEquals("""{"t":"flag","on":true}""", FieldValue.Flag(true).json().toString())
        assertEquals("""{"t":"many","values":["open","packed"]}""", FieldValue.Many(listOf("open", "packed")).json().toString())
        assertEquals("""{"t":"none"}""", FieldValue.None.json().toString())
    }

    @Test
    fun an_answer_is_handed_over_with_its_navree_headers() {
        val event = Events.response(5, 303, null, "/orders/1", listOf("x-navree-shell" to "9f3c"), "")
        assertEquals("/orders/1", event["location"]!!.jsonPrimitive.content)
        assertEquals("x-navree-shell", event["headers"]!!.jsonArray[0].jsonArray[0].jsonPrimitive.content)
        assertNull("an empty body is left out", event["body"])
    }

    @Test
    fun a_device_result_says_its_fields_as_text_or_its_refusal() {
        val done = Events.deviceDone(1, listOf("completed" to "true"))
        assertEquals("true", done["ok"]!!.jsonPrimitive.content)
        assertEquals("completed", (done["fields"] as JsonArray)[0].jsonObject["name"]!!.jsonPrimitive.content)
        assertEquals("cancelled", Events.deviceRefused(2, "cancelled")["refusal"]!!.jsonPrimitive.content)
    }
}

class EffectTest {
    private fun read(text: String) = Effect.read(Json.parseToJsonElement(text))

    @Test
    fun a_fetch_is_read_whole() {
        val fetch = read(
            """{"t":"fetch","request":5,"method":"post","url":"https://shop.example/orders/1/refund",
            "headers":[["x-navree-native","1"],["content-type","application/x-www-form-urlencoded"]],
            "body":{"t":"form","text":"amount=10"},"timeout_ms":60000,"xsrf":true}""",
        ) as Effect.Fetch
        assertEquals(5L, fetch.request)
        assertEquals("post", fetch.method)
        assertEquals(listOf("x-navree-native" to "1", "content-type" to "application/x-www-form-urlencoded"), fetch.headers)
        assertEquals(Body.Form("amount=10"), fetch.body)
        assertEquals(60_000L, fetch.timeoutMs)
        assertTrue(fetch.xsrf)
    }

    @Test
    fun a_multipart_body_keeps_its_parts_in_order() {
        val fetch = read(
            """{"t":"fetch","request":6,"method":"post","url":"https://shop.example/upload","headers":[],
            "body":{"t":"multipart","parts":[{"t":"text","name":"note","value":"corner"},{"t":"file","name":"photo","token":"pick-1","filename":"corner.jpg","type":"image/jpeg"}]},"timeout_ms":60000}""",
        ) as Effect.Fetch
        assertEquals(listOf(Part.Text("note", "corner"), Part.File("photo", "pick-1", "corner.jpg", "image/jpeg")), (fetch.body as Body.Multipart).parts)
    }

    @Test
    fun every_effect_of_the_core_is_read() {
        assertEquals(Effect.OpenUrl("https://pay.example/invoice/7", inside = true), read("""{"t":"open_url","url":"https://pay.example/invoice/7","inside":true}"""))
        assertEquals(Effect.SignIn("https://shop.example/auth/google/redirect", "shop", true), read("""{"t":"sign_in","url":"https://shop.example/auth/google/redirect","callback":"shop","ephemeral":true}"""))
        assertEquals(Effect.Copy("CS-220480", sensitive = true), read("""{"t":"copy","text":"CS-220480","sensitive":true}"""))
        assertEquals(Effect.CancelTimer(1), read("""{"t":"cancel_timer","timer":1}"""))
        assertEquals(Effect.CacheWrite("page:/orders", null), read("""{"t":"cache_write","key":"page:/orders"}"""))
        assertEquals(Effect.CacheClear, read("""{"t":"cache_clear"}"""))
        assertEquals(Effect.Announce(null, Notice.FieldsNeedAttention, 2), read("""{"t":"announce","notice":"fields_need_attention","count":2}"""))
        assertEquals(Effect.StreamOpen(1, "https://shop.example/broadcasting/stream?topics=orders", listOf("x-navree-native" to "1")), read("""{"t":"stream_open","stream":1,"url":"https://shop.example/broadcasting/stream?topics=orders","headers":[["x-navree-native","1"]]}"""))
        assertEquals(Effect.Exit, read("""{"t":"exit"}"""))
        assertEquals(Effect.Show(1), read("""{"t":"message","id":1,"tone":"success","text":"Refunded ₹10.00","action":"Undo"}"""))
        val device = read("""{"t":"device","call":4,"device":{"f":"confirm","reason":"Refund ₹1,00,000"},"posts":true,"options":{"publicKey":{"challenge":"c1"}}}""") as Effect.Device
        assertEquals("c1", device.options!!["publicKey"]!!.jsonObject["challenge"]!!.jsonPrimitive.content)
        assertTrue(device.posts)
        assertEquals(Effect.Random(1, 48), read("""{"t":"random","request":1,"bytes":48}"""))
        assertEquals(Effect.SessionEnding, read("""{"t":"session_ending"}"""))
        assertEquals(Effect.ForgetSession, read("""{"t":"forget_session"}"""))
        assertEquals(Effect.Unknown("hologram"), read("""{"t":"hologram"}"""))
    }
}

class ConfigTest {
    private val device = Device(os = "15", tablet = false, app = "1.0.0", dark = true, contrast = "normal", textPercent = 200, locale = "pa-IN", tz = "Asia/Kolkata")

    @Test
    fun the_cores_configuration_says_who_is_asking_and_what_this_build_does() {
        val config = coreConfig(NavreeConfig(origin = "https://store.example.in/", name = "Store", seed = null), device, listOf("clipboard", "share"))
        assertEquals("https://store.example.in", config["origin"]!!.jsonPrimitive.content)
        val client = config["client"]!!.jsonObject
        assertEquals("android", client["platform"]!!.jsonPrimitive.content)
        assertEquals("phone", client["form"]!!.jsonPrimitive.content)
        assertEquals("dark", client["scheme"]!!.jsonPrimitive.content)
        assertEquals("200", client["text_percent"]!!.jsonPrimitive.content)
        assertEquals("0", client["vocab"]!!.jsonPrimitive.content)
        assertEquals(listOf("clipboard", "share"), client["caps"]!!.jsonArray.map { it.jsonPrimitive.content })
        assertEquals("Android's back goes to the first destination before it leaves", "true", config["back_to_first"]!!.jsonPrimitive.content)
        assertNull("a release build takes no plain http", config["loopback_http"])
        val undrawable = config["undrawable"]!!.jsonArray.map { it.jsonPrimitive.content }
        assertEquals("only the web part is told as one this build does not draw", listOf("web"), undrawable)
        assertFalse("a file field is drawn", "input:file" in undrawable)
        assertFalse("a colour field is drawn", "input:colour" in undrawable)
        assertNull("no package name, no hand-over", config["app_id"])
    }

    @Test
    fun the_most_a_file_may_have_is_the_apps_to_set_and_is_a_hundred_mebibytes_unless_it_does() {
        assertEquals(100L * 1024 * 1024, NavreeConfig(origin = "https://store.example.in", name = "Store", seed = null).mostFileBytes)
        assertEquals(5_000_000L, NavreeConfig(origin = "https://store.example.in", name = "Store", seed = null, mostFileBytes = 5_000_000).mostFileBytes)
    }

    @Test
    fun the_package_name_is_the_apps_id() {
        val config = coreConfig(NavreeConfig(origin = "https://store.example.in", name = "Store", seed = null), device, emptyList(), "in.example.store")
        assertEquals("in.example.store", config["app_id"]!!.jsonPrimitive.content)
    }

    @Test
    fun the_screen_option_tells_the_window_what_it_allows_on_each_system() {
        assertEquals(com.navree.client.ScreenPlan(secure = false, recentsPicture = true), com.navree.client.ScreenPrivacy.None.plan(36))
        assertEquals(com.navree.client.ScreenPlan(secure = false, recentsPicture = false), com.navree.client.ScreenPrivacy.HiddenInRecents.plan(33))
        assertEquals("before Android 13 the recent-apps picture goes only with the secure flag", com.navree.client.ScreenPlan(secure = true, recentsPicture = false), com.navree.client.ScreenPrivacy.HiddenInRecents.plan(32))
        assertEquals(com.navree.client.ScreenPlan(secure = true, recentsPicture = false), com.navree.client.ScreenPrivacy.NoCapture.plan(26))
        assertEquals(com.navree.client.ScreenPrivacy.None, com.navree.client.ScreenPrivacy.of(""))
        assertEquals(com.navree.client.ScreenPrivacy.HiddenInRecents, com.navree.client.ScreenPrivacy.of("hidden_in_recents"))
        assertEquals(com.navree.client.ScreenPrivacy.NoCapture, com.navree.client.ScreenPrivacy.of("no_capture"))
        assertEquals("an option this build does not know keeps the most out", com.navree.client.ScreenPrivacy.NoCapture, com.navree.client.ScreenPrivacy.of("blur"))
    }

    @Test
    fun the_other_origins_the_app_allows_are_the_cores_too() {
        val config = coreConfig(NavreeConfig(origin = "https://store.example.in", name = "Store", seed = null, also = listOf("https://*.store.example.in")), device, emptyList())
        assertEquals(listOf("https://*.store.example.in"), config["also"]!!.jsonArray.map { it.jsonPrimitive.content })
        assertNull("none unless named", coreConfig(NavreeConfig(origin = "https://store.example.in", name = "Store", seed = null), device, emptyList())["also"])
    }

    @Test
    fun the_web_views_token_names_the_protocol_the_platform_and_the_library() {
        assertEquals("NavreeNative/1 (platform=android; lib=0.1.0)", webViewToken())
    }
}
