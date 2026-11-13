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
        assertTrue("a part this build does not draw goes to the web view", config["undrawable"]!!.jsonArray.any { it.jsonPrimitive.content == "input:file" })
        assertNull("no package name, no hand-over", config["app_id"])
    }

    @Test
    fun the_package_name_is_the_apps_id() {
        val config = coreConfig(NavreeConfig(origin = "https://store.example.in", name = "Store", seed = null), device, emptyList(), "in.example.store")
        assertEquals("in.example.store", config["app_id"]!!.jsonPrimitive.content)
    }

    @Test
    fun the_web_views_token_names_the_protocol_the_platform_and_the_library() {
        assertEquals("NavreeNative/1 (platform=android; lib=0.1.0)", webViewToken())
    }
}
