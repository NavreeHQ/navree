package com.navree.client.runtime

import com.navree.client.Fixtures
import com.navree.client.protocol.FormNode
import com.navree.client.protocol.NameValue
import com.navree.client.protocol.NavreeJson
import com.navree.client.protocol.Op
import com.navree.client.protocol.OpSerializer
import com.navree.client.protocol.Route
import com.navree.client.protocol.Screen
import com.navree.client.protocol.decodeEnvelope
import kotlinx.serialization.builtins.ListSerializer
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

class TreeOpsTest {
    /** Every case of the vectors file: a tree, ops, and the tree that must come out. */
    @Test
    fun the_behaviour_vectors_hold() {
        val cases = NavreeJson.parseToJsonElement(Fixtures.text("vectors/tree-ops.json")) as JsonArray
        assertTrue(cases.size >= 10)
        for (case in cases) {
            case as JsonObject
            val name = case.getValue("name").jsonPrimitive.content
            val before = NavreeJson.decodeFromJsonElement(Screen.serializer(), case.getValue("before").jsonObject)
            val after = NavreeJson.decodeFromJsonElement(Screen.serializer(), case.getValue("after").jsonObject)
            val ops = NavreeJson.decodeFromJsonElement(ListSerializer(OpSerializer), case.getValue("ops").jsonArray)
            assertEquals(name, after, ops.fold(before, TreeOps::apply))
        }
    }

    @Test
    fun only_five_ops_change_a_tree() {
        val ops = decodeEnvelope(Fixtures.envelope("ops-shipped")).ops + decodeEnvelope(Fixtures.envelope("ops-errors")).ops
        assertEquals(listOf(true, true, false, false, false, false), ops.map(TreeOps::changesTree))
        assertFalse(TreeOps.changesTree(Op.Reload))
    }
}

class FormsTest {
    private fun form(name: String): FormNode = decodeEnvelope(Fixtures.envelope(name)).screen!!.body as FormNode

    @Test
    fun a_form_sends_what_an_html_form_would() {
        val parts = Forms.collect(form("order-edit"))
        assertEquals(
            listOf("_token", "_method", "name", "phone", "email", "address", "city", "deliver_on", "deliver_at", "payment", "quantity", "discount", "gift", "notes"),
            parts.map { it.name },
        )
        assertEquals("a switch that is on sends its value", Part.Text("gift", "1"), parts.first { it.name == "gift" })
        assertTrue("a checkbox that is off sends nothing", parts.none { it.name == "invoice" })
        assertEquals(Part.Text("quantity", "2"), parts.first { it.name == "quantity" })
    }

    @Test
    fun what_the_user_typed_takes_the_place_of_what_the_tree_holds() {
        val edits = mapOf(
            "name" to FieldValue.Text("Simran Bhatia"),
            "gift" to FieldValue.Flag(false),
            "invoice" to FieldValue.Flag(true),
            "quantity" to FieldValue.Text("3"),
            "discount" to FieldValue.Number(12.5),
        )
        val parts = Forms.collect(form("order-edit"), edits).associate { it.name to (it as Part.Text).value }
        assertEquals("Simran Bhatia", parts["name"])
        assertNull(parts["gift"])
        assertEquals("1", parts["invoice"])
        assertEquals("3", parts["quantity"])
        assertEquals("12.5", parts["discount"])
    }

    @Test
    fun a_command_may_set_a_hidden_value_that_has_an_id_and_a_button_adds_its_own_pair() {
        val form = decodeEnvelope(Fixtures.text("corpus/screen-form.json")).screen!!.body as FormNode
        val parts = Forms.collect(form, hidden = mapOf("plan" to "pro", "name" to "Not a hidden value"), with = NameValue("intent", "draft"))
        assertEquals(Part.Text("_csrf", "tok"), parts[0])
        assertEquals(Part.Text("plan", "pro"), parts[1])
        assertEquals("a field is not a hidden value", Part.Text("name", ""), parts[2])
        assertEquals(Part.Text("intent", "draft"), parts.last())
    }

    @Test
    fun urlencoding_is_the_form_kind() {
        val parts = listOf(
            Part.Text("name", "ਹਰਪ੍ਰੀਤ & Co."),
            Part.Text("note", "line one\nline two"),
            Part.Text("a b", "1+1=2"),
            Part.Text("tilde", "~*-._"),
        )
        assertEquals(
            "name=%E0%A8%B9%E0%A8%B0%E0%A8%AA%E0%A9%8D%E0%A8%B0%E0%A9%80%E0%A8%A4+%26+Co." +
                "&note=line+one%0D%0Aline+two&a+b=1%2B1%3D2&tilde=%7E*-._",
            Forms.urlencoded(parts),
        )
        assertEquals("", Forms.urlencoded(emptyList()))
    }

    @Test
    fun a_form_with_a_file_is_sent_as_multipart() {
        val file = PickedFile("bill \"nov\".pdf", "application/pdf", byteArrayOf(1, 2, 3))
        val parts = listOf(Part.Text("_token", "k3J9sQ"), Part.Text("note", "a\nb"), Part.File("bill", file))
        assertTrue(Forms.needsMultipart(parts))
        val payload = Forms.payload(parts, "XYZ")
        assertEquals("multipart/form-data; boundary=XYZ", payload.contentType)
        val expected = "--XYZ\r\nContent-Disposition: form-data; name=\"_token\"\r\n\r\nk3J9sQ\r\n" +
            "--XYZ\r\nContent-Disposition: form-data; name=\"note\"\r\n\r\na\r\nb\r\n" +
            "--XYZ\r\nContent-Disposition: form-data; name=\"bill\"; filename=\"bill %22nov%22.pdf\"\r\n" +
            "Content-Type: application/pdf\r\n\r\n\u0001\u0002\u0003\r\n--XYZ--\r\n"
        assertEquals(expected, String(payload.bytes, Charsets.ISO_8859_1))

        assertFalse(Forms.needsMultipart(parts.take(2)))
        assertEquals(Forms.URLENCODED, Forms.payload(parts.take(2), "XYZ").contentType)
    }

    @Test
    fun a_get_form_puts_its_fields_in_the_query_in_place_of_the_old_one() {
        assertEquals("/orders?status=paid", Forms.query("/orders?page=3#top", listOf(Part.Text("status", "paid"))))
        assertEquals("/orders", Forms.query("/orders?page=3", emptyList()))
    }

    @Test
    fun numbers_are_written_as_a_form_writes_them() {
        assertEquals("2", Forms.number(2.0))
        assertEquals("-7", Forms.number(-7.0))
        assertEquals("0.5", Forms.number(0.5))
    }
}

class OriginTest {
    private val origin = requireNotNull(Origin.parse("https://store.example.in"))

    @Test
    fun an_origin_must_be_https() {
        assertNull(Origin.parse("http://store.example.in"))
        assertNull(Origin.parse("ftp://store.example.in"))
        assertNull(Origin.parse("store.example.in"))
        assertNull(Origin.parse("https://user:secret@store.example.in"))
        assertNull("plain http to this machine is refused unless asked for", Origin.parse("http://10.0.2.2:8787"))
        assertEquals("http://10.0.2.2:8787", Origin.parse("http://10.0.2.2:8787", loopbackHttp = true)?.base)
        assertNull("and only to this machine", Origin.parse("http://store.example.in", loopbackHttp = true))
        assertEquals("https://store.example.in", Origin.parse("HTTPS://Store.Example.in:443/")?.base)
        assertEquals("https://store.example.in:8443", Origin.parse("https://store.example.in:8443")?.base)
    }

    @Test
    fun addresses_on_the_origin_come_back_as_a_path_and_query() {
        assertEquals(Target.Inside("/orders"), origin.resolve("/orders"))
        assertEquals(Target.Inside("/orders?q=a%20b"), origin.resolve("https://store.example.in/orders?q=a%20b#row"))
        assertEquals(Target.Inside("/orders"), origin.resolve("HTTPS://STORE.example.in:443/orders"))
        assertEquals(Target.Inside("/"), origin.resolve("https://store.example.in"))
        assertEquals(Target.Inside("/orders/1042/edit"), origin.resolve("edit", from = "/orders/1042/"))
        assertEquals(Target.Inside("/orders/new"), origin.resolve("new", from = "/orders/1042"))
        assertEquals(Target.Inside("/orders/"), origin.resolve("../", from = "/orders/1042/"))
        assertEquals(Target.Inside("/orders/1042?tab=items"), origin.resolve("?tab=items", from = "/orders/1042?tab=notes"))
        assertEquals(Target.Inside("/a"), origin.resolve("//store.example.in/a"))
        assertEquals("https://store.example.in/orders", origin.absolute("/orders"))
    }

    @Test
    fun anything_else_leaves_the_app_or_is_refused() {
        val outside = listOf(
            "https://navree.com/docs",
            "http://store.example.in/orders",
            "https://store.example.in:8443/orders",
            "https://store.example.in.evil.example/orders",
            "https://store.example.in@evil.example/orders",
            "//evil.example/orders",
            "mailto:help@example.in",
            "tel:+919814012345",
        )
        for (href in outside) assertTrue("$href leaves the app", origin.resolve(href) is Target.Outside)

        val refused = listOf(
            "javascript:alert(1)", "JaVaScRiPt:alert(1)", "data:text/html,<script>1</script>", "file:///data/data/x",
            "content://x", "intent://scan/#Intent;scheme=zxing;end", "blob:https://store.example.in/1", "about:blank",
            "/\\evil.example", "\\\\evil.example", "/orders\u0000", "/or\nders", "#top", "", "  ", "/../../etc/passwd",
            "https://", "/has space",
        )
        for (href in refused) assertEquals("'$href' is refused", Target.Refused, origin.resolve(href))
        assertFalse(origin.owns("https://evil.example"))
        assertTrue(origin.owns("https://store.example.in/reports"))
    }

    @Test
    fun routes_match_whole_paths() {
        assertTrue(Routes.matches("/orders/new", "/orders/new"))
        assertTrue(Routes.matches("/orders/new", "/orders/new?from=home"))
        assertTrue(Routes.matches("/orders/*/edit", "/orders/1042/edit"))
        assertFalse(Routes.matches("/orders/*/edit", "/orders/1042"))
        assertFalse(Routes.matches("/orders/*/edit", "/orders/1042/items/edit"))
        assertFalse(Routes.matches("/orders/new", "/orders/newest"))
        assertTrue(Routes.matches("/settings/**", "/settings/store/tax"))
        assertTrue(Routes.matches("/settings/**", "/settings"))
        assertTrue(Routes.matches("/", "/"))
        assertFalse(Routes.matches("/", "/orders"))

        val routes = listOf(Route("/orders/new"), Route("/orders/*"))
        assertEquals("the first route that matches wins", "/orders/new", Routes.find(routes, "/orders/new")?.match)
        assertEquals("/orders/*", Routes.find(routes, "/orders/7")?.match)
        assertNull(Routes.find(routes, "/customers"))
    }
}

class CoverageAndDescriptorTest {
    @Test
    fun the_fixtures_this_client_draws_are_drawn_whole() {
        for (name in listOf("orders", "order", "order-edit", "order-new", "home", "products", "customers", "customers-empty", "settings", "sign-in")) {
            val screen = decodeEnvelope(Fixtures.envelope(name)).screen!!
            assertEquals("$name is all native", emptyList<String>(), Coverage.missing(screen))
        }
    }

    @Test
    fun the_client_says_what_it_is_on_every_request() {
        val descriptor = ClientDescriptor(
            os = "16",
            form = "phone",
            app = "1.4.0",
            dark = true,
            highContrast = false,
            textScale = 1.3f,
            locale = "en-IN",
            timeZone = "Asia/Kolkata",
            caps = listOf("biometrics", "push"),
        )
        assertEquals(
            "platform=android; os=16; form=phone; app=1.4.0; vocab=1; scheme=dark; contrast=normal; text=1.3; " +
                "locale=en-IN; tz=Asia/Kolkata; caps=biometrics+push",
            descriptor.header(),
        )
        assertEquals(
            mapOf(
                "X-Navree-Native" to "1",
                "X-Navree-Client" to descriptor.header(),
                "Accept" to "application/vnd.navree.native+json",
            ),
            descriptor.headers(),
        )
        assertEquals("a value cannot smuggle in another pair", "os=16formtv", descriptor.copy(os = "16; form=tv").header().split("; ")[1])
        assertTrue(descriptor.copy(textScale = 2f).header().contains("text=2.0;"))
    }

    @Test
    fun the_tree_cache_keeps_the_newest_and_forgets_the_oldest() {
        var cache = TreeCache(limit = 2)
        cache = cache.put("/a", Screen(title = "A")).put("/b", Screen(title = "B")).put("/a", Screen(title = "A again")).put("/c", Screen(title = "C"))
        assertEquals(listOf("/a", "/c"), cache.trees.keys.toList())
        assertEquals("A again", cache["/a"]?.title)
        assertTrue(cache.clear().trees.isEmpty())
    }
}
