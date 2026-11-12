package com.navree.client.runtime

import com.navree.client.Fixtures
import com.navree.client.protocol.ButtonNode
import com.navree.client.protocol.Command
import com.navree.client.protocol.Commands
import com.navree.client.protocol.ListNode
import com.navree.client.protocol.Navigate
import com.navree.client.protocol.Present
import com.navree.client.protocol.RowNode
import com.navree.client.protocol.SHELL_PATH
import com.navree.client.protocol.Submit
import com.navree.client.protocol.Tone
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class ReducerTest {
    private fun Sim.go(href: String, present: Present? = null) {
        send(Event.Acted(front.key, Navigate(href, present)))
        settle()
    }

    private fun Sim.body(entry: Entry = front): String = String(lastCall().body!!.bytes, Charsets.UTF_8).also { check(entry.key >= 0) }

    private fun Sim.shellFetches(): Int = effects.count { it is Effect.Fetch && it.call.url == SHELL_PATH }

    // Starting

    @Test
    fun starting_fetches_the_shell_then_the_first_destination() {
        val sim = Sim()
        val first = sim.send(Event.Started)
        assertEquals(listOf(HttpRequest("GET", SHELL_PATH)), first.map { (it as Effect.Fetch).call })
        assertEquals(Load.Loading, sim.state.shellLoad)

        sim.answer(sim.waiting().single())
        assertEquals(Load.Ready, sim.state.shellLoad)
        assertEquals(5, sim.state.tabs.size)
        assertEquals("/", sim.front.url)
        assertEquals(Content.Blank, sim.front.content)
        assertTrue(sim.front.loading)

        sim.settle()
        assertEquals("Store", sim.front.screen?.title)
        assertFalse(sim.front.loading)
        assertEquals("3f9a1c07b2de", sim.state.build)
        assertNotNull(sim.state.cache["/"])
        assertFalse("with one screen of the first destination, back is the system's", sim.state.canGoBack)
    }

    @Test
    fun a_shell_that_cannot_be_fetched_fails_the_start_and_can_be_tried_again() {
        val sim = Sim()
        sim.answers[SHELL_PATH] = { HttpResponse.Failed(Failure.Offline) }
        sim.start()
        assertEquals(Load.Failed(Failure.Offline), sim.state.shellLoad)
        assertNull(sim.state.shell)

        sim.answers[SHELL_PATH] = { Sim.native(SHELL_PATH, Fixtures.text("shell.json")) }
        sim.start()
        assertEquals(Load.Ready, sim.state.shellLoad)
        assertEquals("Store", sim.front.screen?.title)
    }

    @Test
    fun a_server_with_no_native_shell_is_one_destination_in_the_web_view() {
        val sim = Sim()
        sim.answers[SHELL_PATH] = { HttpResponse.Answer(SHELL_PATH, 404, "text/html", "<h1>Not found</h1>") }
        sim.answers["/"] = { HttpResponse.Answer("/", 200, "text/html; charset=utf-8", "<html></html>") }
        sim.start()
        assertEquals(1, sim.state.tabs.size)
        assertEquals(Content.Web("/", listOf("html")), sim.front.content)
    }

    // Destinations and stacks

    @Test
    fun each_destination_keeps_its_own_stack() {
        val sim = Sim().start()
        sim.send(Event.DestinationChosen(1))
        sim.settle()
        assertEquals("/orders", sim.front.url)
        sim.go("/orders/1042")
        assertEquals(listOf("/orders", "/orders/1042"), sim.urls())

        sim.send(Event.DestinationChosen(0))
        assertEquals(listOf("/"), sim.urls())
        sim.send(Event.DestinationChosen(1))
        assertEquals("coming back finds the stack as it was left", listOf("/orders", "/orders/1042"), sim.urls())

        sim.send(Event.DestinationChosen(1))
        assertEquals("choosing the destination in front goes back to its first screen", listOf("/orders"), sim.urls())
    }

    @Test
    fun a_link_to_a_destination_selects_it_instead_of_stacking_a_copy() {
        val sim = Sim().start()
        sim.go("/products")
        assertEquals(2, sim.state.selected)
        assertEquals(listOf("/products"), sim.urls())
        assertEquals(1, sim.state.tabs[0].stack.size)
    }

    @Test
    fun back_pops_then_returns_to_the_first_destination_then_leaves() {
        val sim = Sim().start()
        sim.send(Event.DestinationChosen(1))
        sim.settle()
        sim.go("/orders/1042")
        assertTrue(sim.state.canGoBack)

        sim.send(Event.BackPressed)
        assertEquals(listOf("/orders"), sim.urls())
        assertTrue("another destination than the first still has somewhere to go back to", sim.state.canGoBack)

        sim.send(Event.BackPressed)
        assertEquals(0, sim.state.selected)
        assertFalse(sim.state.canGoBack)

        assertEquals(listOf(Effect.Leave), sim.send(Event.BackPressed))
    }

    @Test
    fun the_last_tree_for_an_address_is_shown_at_once_while_a_fresh_one_is_fetched() {
        val sim = Sim().start()
        sim.go("/orders/1042")
        sim.send(Event.BackPressed)

        sim.send(Event.Acted(sim.front.key, Navigate("/orders/1042")))
        assertEquals("Order #1042", sim.front.screen?.title)
        assertTrue(sim.front.loading)
        assertEquals("/orders/1042", sim.waiting().single().call.url)
        sim.settle()
        assertFalse(sim.front.loading)
    }

    @Test
    fun a_build_the_client_has_not_seen_drops_every_cached_tree_and_asks_for_the_shell_again() {
        val sim = Sim().start()
        sim.go("/orders/1042")
        assertEquals(setOf("/", "/orders/1042"), sim.state.cache.trees.keys)

        sim.answers["/customers"] = { Sim.native(it.url, Fixtures.envelope("customers").replace("3f9a1c07b2de", "77aa00bb11cc")) }
        sim.send(Event.DestinationChosen(3))
        sim.answer(sim.waiting().single { it.call.url == "/customers" })
        assertEquals("77aa00bb11cc", sim.state.build)
        assertEquals(setOf("/customers"), sim.state.cache.trees.keys)
        assertEquals(2, sim.shellFetches())
    }

    @Test
    fun an_answer_that_is_no_longer_wanted_changes_nothing() {
        val sim = Sim().start()
        sim.send(Event.Refreshed(sim.front.key))
        val first = sim.waiting().single()
        sim.send(Event.Refreshed(sim.front.key))
        val before = sim.state
        sim.send(Event.Answered(first.request, Sim.native("/", Fixtures.envelope("orders"))))
        assertEquals(before, sim.state)
        sim.settle()
        assertEquals("Store", sim.front.screen?.title)
    }

    // Presenting

    @Test
    fun a_route_of_the_shell_presents_its_address_over_the_app() {
        val sim = Sim().start()
        sim.go("/orders")
        val create = sim.front.screen!!.actions[0] as ButtonNode
        sim.send(Event.Pressed(sim.front.key, create))
        sim.settle()
        assertEquals(Present.Sheet, sim.state.modals.single().present)
        assertEquals("/orders/new", sim.front.url)
        assertEquals("New order", sim.front.screen?.title)
        assertEquals("the list is still under it", "/orders", sim.state.tabs[1].stack.single().url)

        sim.send(Event.BackPressed)
        assertTrue(sim.state.modals.isEmpty())
        assertEquals("/orders", sim.front.url)
    }

    @Test
    fun a_screen_with_no_chrome_says_so_from_its_route() {
        val sim = Sim().start()
        sim.go("/sign-in")
        assertEquals(Present.Full, sim.state.modals.single().present)
        assertFalse(sim.front.chrome)
    }

    // Leaving the origin

    @Test
    fun what_leaves_the_origin_opens_outside_and_what_cannot_be_read_goes_nowhere() {
        val sim = Sim().start()
        val key = sim.front.key
        assertEquals(listOf(Effect.OpenOutside("https://navree.com/docs")), sim.send(Event.Acted(key, Navigate("https://navree.com/docs"))))
        assertEquals(listOf(Effect.OpenOutside("tel:+919814012345")), sim.send(Event.Acted(key, Navigate("tel:+919814012345"))))
        assertEquals(
            "an address on the origin that is to open outside is made whole",
            listOf(Effect.OpenOutside("https://store.example.in/orders/1042/invoice")),
            sim.send(Event.Acted(key, Navigate("/orders/1042/invoice", Present.External))),
        )
        val before = sim.state
        for (bad in listOf("javascript:alert(1)", "file:///etc/passwd", "intent://x#Intent;end", "/\\evil.example", "#top", "")) {
            assertEquals("`$bad` does nothing", emptyList<Effect>(), sim.send(Event.Acted(key, Navigate(bad))))
        }
        assertEquals(before, sim.state)
    }

    // The web view

    @Test
    fun a_page_the_server_could_not_lower_is_shown_in_the_web_view() {
        val sim = Sim().start()
        sim.go("/reports")
        assertEquals(Content.Web("/reports", listOf("slot:chart", "tag:canvas")), sim.front.content)
    }

    @Test
    fun a_screen_with_a_part_the_client_cannot_draw_is_shown_whole_in_the_web_view() {
        val sim = Sim().start()
        sim.answers["/future"] = sim.fixture("newer-vocabulary")
        sim.go("/future")
        val content = sim.front.content as Content.Web
        assertEquals("/future", content.url)
        assertEquals(listOf("client:kind:hologram", "client:kind:timeline", "client:input:emoji"), content.why)
    }

    @Test
    fun a_link_tapped_in_the_web_view_becomes_a_native_screen() {
        val sim = Sim().start()
        sim.go("/reports")
        sim.send(Event.LinkFollowed(sim.front.key, "https://store.example.in/orders/1042"))
        sim.settle()
        assertEquals(listOf("/", "/reports", "/orders/1042"), sim.urls())
        assertEquals("Order #1042", sim.front.screen?.title)

        val outside = sim.send(Event.LinkFollowed(sim.front.key, "https://elsewhere.example/page"))
        assertEquals(listOf(Effect.OpenOutside("https://elsewhere.example/page")), outside)
        assertEquals(3, sim.urls().size)
    }

    // Forms

    private fun Sim.openEdit() {
        send(Event.DestinationChosen(1))
        settle()
        go("/orders/1042")
        go("/orders/1042/edit")
        check(state.modals.size == 1) { "The edit form is presented" }
    }

    @Test
    fun a_form_posts_what_a_browser_would_and_shows_the_errors_it_gets_back() {
        val sim = Sim().start()
        sim.openEdit()
        sim.answers["/orders/1042"] = { request ->
            if (request.method == "POST") Sim.native(request.url, Fixtures.envelope("ops-errors"), 422) else Sim.native(request.url, Fixtures.envelope("order"))
        }
        val key = sim.front.key
        sim.send(Event.FieldChanged(key, "order-form", "name", FieldValue.Text("")))
        sim.send(Event.FieldChanged(key, "order-form", "gift", FieldValue.Flag(false)))
        sim.send(Event.Acted(key, Submit("order-form")))

        val call = sim.lastCall()
        assertEquals("POST", call.method)
        assertEquals("/orders/1042", call.url)
        assertEquals(Forms.URLENCODED, call.body?.contentType)
        assertEquals(
            "_token=k3J9sQ&_method=patch&name=&phone=98140+1234&email=harpreet.kaur%40example.in" +
                "&address=House+214%2C+Sarabha+Nagar&city=ludhiana&deliver_on=2026-11-14&deliver_at=10%3A30" +
                "&payment=upi&quantity=2&discount=10&notes=",
            sim.body(),
        )
        assertTrue("order-form" in sim.front.local.busy)

        sim.send(Event.Acted(key, Submit("order-form")))
        assertEquals("a form that is waiting is not sent twice", 1, sim.waiting().size)

        sim.settle()
        val local = sim.front.local
        assertTrue(local.busy.isEmpty())
        assertEquals(listOf("Enter the customer's name"), local.errors("order-form", "name"))
        assertEquals(listOf("Enter a 10-digit mobile number"), local.errors("order-form", "phone"))
        assertEquals("order-name", local.focus?.target)
        assertEquals(Tone.Danger, sim.state.messages.single().tone)
        assertEquals("the form is still up, with what was typed", FieldValue.Text(""), local.value("order-form", "name"))
        assertEquals(1, sim.state.modals.size)

        sim.send(Event.FieldChanged(key, "order-form", "name", FieldValue.Text("Harpreet")))
        assertTrue("changing a field takes its error away", sim.front.local.errors("order-form", "name").isEmpty())
        assertFalse(sim.front.local.errors("order-form", "phone").isEmpty())
    }

    @Test
    fun a_post_from_a_presented_form_closes_it_and_freshens_what_is_under_it() {
        val sim = Sim().start()
        sim.openEdit()
        val details = sim.state.tabs[1].stack.last().key
        sim.answers["/orders/1042"] = { request ->
            if (request.method == "POST") Sim.native(request.url, Fixtures.envelope("ops-saved")) else Sim.native(request.url, Fixtures.envelope("order"))
        }
        sim.send(Event.Acted(sim.front.key, Submit("order-form")))
        sim.answer(sim.waiting().single())

        assertTrue("the form has gone", sim.state.modals.isEmpty())
        assertEquals("the details screen is the same one, not a copy", details, sim.front.key)
        assertEquals(listOf("/orders", "/orders/1042"), sim.urls())
        assertTrue(sim.front.loading)
        assertEquals("/orders/1042", sim.waiting().single().call.url)
        val message = sim.state.messages.single()
        assertEquals("Order saved", message.text)
        assertEquals("Undo", message.action?.label)
        assertTrue("the list under the details is out of date", sim.state.tabs[1].stack.first().stale)
        assertTrue("a write drops the cache", "/orders" !in sim.state.cache.trees)

        sim.settle()
        sim.send(Event.BackPressed)
        assertEquals("an out-of-date screen is fetched again when it comes to the front", "/orders", sim.waiting().single().call.url)
    }

    @Test
    fun a_post_from_a_pushed_form_goes_back_to_the_screen_it_names_or_takes_the_forms_place() {
        val sim = Sim().start()
        sim.send(Event.DestinationChosen(1))
        sim.settle()
        sim.go("/orders/1042")
        sim.go("/orders/1042/edit", Present.Push)
        assertEquals(listOf("/orders", "/orders/1042", "/orders/1042/edit"), sim.urls())
        sim.answers["/orders/1042"] = { request ->
            if (request.method == "POST") Sim.native(request.url, Fixtures.envelope("ops-saved")) else Sim.native(request.url, Fixtures.envelope("order"))
        }
        sim.send(Event.Acted(sim.front.key, Submit("order-form")))
        sim.settle()
        assertEquals("back to the details, which were already there", listOf("/orders", "/orders/1042"), sim.urls())

        sim.send(Event.BackPressed)
        sim.settle()
        sim.go("/orders/new", Present.Push)
        sim.answers["/orders"] = { request ->
            if (request.method == "POST") {
                Sim.native(request.url, "{\"navree\":1,\"ops\":[{\"op\":\"navigate\",\"href\":\"/orders/1042\"}]}")
            } else {
                Sim.native(request.url, Fixtures.envelope("orders"))
            }
        }
        sim.send(Event.Acted(sim.front.key, Submit("new-order-form")))
        sim.settle()
        assertEquals("the new order takes the form's place", listOf("/orders", "/orders/1042"), sim.urls())
    }

    @Test
    fun a_post_answered_with_a_screen_shows_that_screen_without_fetching_it() {
        val sim = Sim().start()
        sim.openEdit()
        // The server redirected the post to the details page and the transport followed.
        sim.answers["/orders/1042"] = { Sim.native("/orders/1042", Fixtures.envelope("order").replace("Ready for pickup", "Saved a moment ago")) }
        sim.send(Event.Acted(sim.front.key, Submit("order-form")))
        sim.answer(sim.waiting().single())
        assertTrue(sim.state.modals.isEmpty())
        assertEquals("/orders/1042", sim.front.url)
        assertFalse(sim.front.loading)
        assertTrue(sim.waiting().isEmpty())
        assertNotNull(sim.front.screen!!.walk().firstOrNull { it is com.navree.client.protocol.BannerNode && it.title == "Saved a moment ago" })
    }

    @Test
    fun a_get_form_goes_to_its_address_with_the_fields_as_the_query() {
        val sim = Sim().start()
        sim.go("/orders")
        val key = sim.front.key
        sim.send(Event.Acted(key, Commands(listOf(Command.Open("order-filters")))))
        assertTrue("order-filters" in sim.front.local.opened)
        sim.send(Event.FieldChanged(key, "filter-form", "status", FieldValue.Text("paid")))
        sim.send(Event.FieldChanged(key, "filter-form", "city", FieldValue.Many(listOf("ludhiana", "delhi"))))
        sim.answers["/orders?status=paid&city=ludhiana&city=delhi"] = sim.fixture("orders")
        sim.send(Event.Acted(key, Submit("filter-form")))
        assertEquals(HttpRequest("GET", "/orders?status=paid&city=ludhiana&city=delhi"), sim.lastCall())
        assertTrue("the screen shows that it is waiting", sim.front.loading)
        sim.settle()
        assertEquals("the same screen, at its new address", key, sim.front.key)
        assertEquals("/orders?status=paid&city=ludhiana&city=delhi", sim.front.url)
        assertFalse(sim.front.loading)
        assertTrue("the sheet closed", sim.front.local.opened.isEmpty())
        assertEquals("what was chosen is still chosen", FieldValue.Text("paid"), sim.front.local.value("filter-form", "status"))
    }

    // Asking first

    @Test
    fun a_destructive_button_that_reaches_the_server_asks_first() {
        val sim = Sim().start()
        sim.go("/orders")
        val row = sim.front.screen!!.walk().filterIsInstance<RowNode>().first()
        val cancel = row.menu.last() as ButtonNode
        val key = sim.front.key

        assertTrue(sim.send(Event.Pressed(key, cancel)).isEmpty())
        assertEquals(cancel, sim.front.local.confirm?.button)
        sim.send(Event.ConfirmAnswered(key, accepted = false))
        assertNull(sim.front.local.confirm)
        assertTrue(sim.waiting().isEmpty())

        sim.send(Event.Pressed(key, cancel))
        sim.send(Event.ConfirmAnswered(key, accepted = true))
        assertEquals("POST", sim.lastCall().method)
        assertEquals("/orders/1042/cancel", sim.lastCall().url)
        assertEquals("_token=k3J9sQ", sim.body())
    }

    @Test
    fun a_destructive_button_that_opens_the_pages_own_dialog_is_not_asked_about_twice() {
        val sim = Sim().start()
        sim.go("/orders/1042")
        val key = sim.front.key
        val screen = sim.front.screen!!
        val open = screen.actions.last() as ButtonNode
        sim.send(Event.Pressed(key, open))
        assertNull(sim.front.local.confirm)
        assertEquals(listOf("cancel-order"), sim.front.local.opened)

        sim.send(Event.BackPressed)
        assertTrue("back closes the dialog before it leaves the screen", sim.front.local.opened.isEmpty())
        assertEquals("/orders/1042", sim.front.url)

        sim.send(Event.Pressed(key, open))
        val dialog = screen.find("cancel-order") as com.navree.client.protocol.DialogNode
        val confirm = dialog.actions.last() as ButtonNode
        sim.send(Event.Pressed(key, confirm, confirmed = true))
        assertEquals("/orders/1042/cancel", sim.lastCall().url)
        assertTrue("the dialog closes when its request is sent", sim.front.local.opened.isEmpty())
    }

    // Commands

    @Test
    fun commands_change_local_state_and_never_ask_the_server() {
        val sim = Sim().start()
        sim.go("/orders/1042")
        val key = sim.front.key
        val run = { commands: List<Command> -> sim.send(Event.Acted(key, Commands(commands))) }

        assertEquals(listOf(Effect.Copy("1042")), run(listOf(Command.Copy("1042"), Command.Toast(Tone.Info, "Copied"))))
        assertEquals("Copied", sim.state.messages.last().text)

        run(listOf(Command.Hide("pickup")))
        assertTrue(sim.front.local.isHidden("pickup"))
        run(listOf(Command.Toggle("pickup")))
        assertFalse(sim.front.local.isHidden("pickup"))
        run(listOf(Command.Set("pickup", "hidden", true)))
        assertTrue(sim.front.local.isHidden("pickup"))
        run(listOf(Command.Show("pickup")))
        assertFalse(sim.front.local.isHidden("pickup"))

        run(listOf(Command.Text("order-status", "Packed")))
        assertEquals("Packed", sim.front.local.texts["order-status"])
        run(listOf(Command.Open("order-notes")))
        assertTrue(sim.front.local.isOpen(sim.front.screen!!.find("order-notes")!!))
        run(listOf(Command.Close("order-notes")))
        assertFalse(sim.front.local.isOpen(sim.front.screen!!.find("order-notes")!!))
        run(listOf(Command.Set("order-status", "active", true), Command.Flip("order-status", "selected")))
        assertEquals(setOf("active", "selected"), sim.front.local.states["order-status"])
        run(listOf(Command.Flip("order-status", "selected")))
        assertEquals(setOf("active"), sim.front.local.states["order-status"])
        run(listOf(Command.Focus("order-status")))
        assertEquals("order-status", sim.front.local.focus?.target)
        run(listOf(Command.Unknown("fly"), Command.Count("x", 10)))

        assertTrue("none of that asked for anything", sim.waiting().isEmpty())
    }

    @Test
    fun ops_from_the_server_change_the_tree_and_the_local_state() {
        val sim = Sim().start()
        sim.go("/orders/1042")
        val key = sim.front.key
        sim.answers["/orders/1042/ship"] = sim.fixture("ops-shipped")
        val ship = sim.front.screen!!.actions[3] as ButtonNode
        sim.send(Event.Pressed(key, ship))
        sim.settle()
        val screen = sim.front.screen!!
        assertEquals("Shipped", (screen.find("order-status") as com.navree.client.protocol.BadgeNode).text)
        assertNull(screen.find("pickup"))
        assertEquals("Marked as shipped", sim.state.messages.single().text)
        assertEquals("the cache holds the tree as it now is", screen, sim.state.cache["/orders/1042"])
    }

    // Search and the next page

    @Test
    fun typing_in_search_asks_only_when_told_to_and_keeps_the_screen() {
        val sim = Sim().start()
        sim.go("/orders")
        val key = sim.front.key
        sim.answers["/orders?q=harpreet+kaur"] = sim.fixture("orders")
        sim.send(Event.SearchChanged(key, "harpreet kaur"))
        assertTrue(sim.waiting().isEmpty())

        sim.send(Event.SearchRun(key))
        assertEquals("/orders?q=harpreet+kaur", sim.waiting().single().call.url)
        assertEquals(key, sim.front.key)
        assertNotNull("the list stays while the answer is on its way", sim.front.screen)
        sim.settle()
        assertEquals("harpreet kaur", sim.front.local.search)

        sim.send(Event.SearchRun(key))
        assertTrue("the same query is not asked for twice", sim.waiting().isEmpty())

        sim.send(Event.SearchChanged(key, ""))
        sim.send(Event.SearchRun(key))
        assertEquals("/orders", sim.waiting().single().call.url)
    }

    @Test
    fun the_next_page_of_a_list_is_asked_for_once_and_added_to_it() {
        val sim = Sim().start()
        sim.go("/orders")
        val key = sim.front.key
        sim.send(Event.EndReached(key, "orders-list"))
        sim.send(Event.EndReached(key, "orders-list"))
        assertEquals("/orders?after=1033", sim.waiting().single().call.url)
        assertTrue("orders-list" in sim.front.local.loadingMore)

        sim.settle()
        val list = sim.front.screen!!.find("orders-list") as ListNode
        assertEquals(13, list.children.size)
        assertNull("the last page has no page after it", list.more)
        assertTrue(sim.front.local.loadingMore.isEmpty())

        sim.send(Event.EndReached(key, "orders-list"))
        assertTrue(sim.waiting().isEmpty())
    }

    @Test
    fun a_next_page_that_leaves_the_address_as_it_was_ends_the_list() {
        val sim = Sim().start()
        sim.go("/orders")
        sim.answers["/orders?after=1033"] = {
            Sim.native(it.url, "{\"navree\":1,\"ops\":[{\"op\":\"prepend\",\"target\":\"orders-list\",\"nodes\":[{\"t\":\"row\",\"title\":\"One more\"}]}]}")
        }
        sim.send(Event.EndReached(sim.front.key, "orders-list"))
        sim.settle()
        val list = sim.front.screen!!.find("orders-list") as ListNode
        assertEquals(11, list.children.size)
        assertNull(list.more)
    }

    // Failures

    @Test
    fun a_screen_that_cannot_be_fetched_says_so_and_one_that_cannot_be_refreshed_keeps_what_it_shows() {
        val sim = Sim().start()
        sim.answers["/orders/7"] = { HttpResponse.Failed(Failure.Offline) }
        sim.go("/orders/7")
        assertEquals(Failure.Offline, sim.front.failure)
        assertEquals(Content.Blank, sim.front.content)
        assertTrue("a screen with nothing on it says it in the screen, not in a passing message", sim.state.messages.isEmpty())

        sim.send(Event.BackPressed)
        sim.answers["/"] = { HttpResponse.Answer("/", 503, "text/html", "Down for maintenance") }
        sim.send(Event.Refreshed(sim.front.key))
        assertTrue(sim.front.refreshing)
        sim.settle()
        assertFalse(sim.front.refreshing)
        assertEquals("Store", sim.front.screen?.title)
        assertEquals(Failure.Server, sim.state.messages.single().notice)
    }

    @Test
    fun an_answer_from_another_protocol_is_refused() {
        val sim = Sim().start()
        sim.answers["/orders/8"] = { Sim.native(it.url, "{\"navree\":2,\"screen\":{\"title\":\"From the future\"}}") }
        sim.go("/orders/8")
        assertEquals(Failure.Protocol, sim.front.failure)
    }

    @Test
    fun an_app_that_is_too_old_is_told_to_update() {
        val sim = Sim().start()
        sim.answers["/orders"] = sim.fixture("upgrade")
        sim.go("/orders")
        assertEquals("1.2.0", sim.state.upgrade?.minApp)
        val url = sim.state.upgrade!!.url!!
        assertEquals(listOf(Effect.OpenOutside(url)), sim.send(Event.Opened(url)))
    }

    // Addresses from outside

    @Test
    fun an_address_from_outside_opens_only_when_the_shell_knows_it() {
        val sim = Sim().start()
        sim.send(Event.Opened("https://store.example.in/orders/new"))
        assertEquals("/orders/new", sim.front.url)
        assertEquals(1, sim.state.modals.size)
        sim.send(Event.BackPressed)

        sim.send(Event.Opened("https://store.example.in/customers"))
        assertEquals(3, sim.state.selected)

        val before = sim.state
        sim.send(Event.Opened("https://store.example.in/orders/1042"))
        sim.send(Event.Opened("https://evil.example/orders/new"))
        sim.send(Event.Opened("javascript:alert(1)"))
        assertEquals(before, sim.state)
    }

    @Test
    fun an_address_that_arrives_before_the_shell_waits_for_it() {
        val sim = Sim()
        sim.send(Event.Opened("https://store.example.in/customers"))
        assertEquals("/customers", sim.state.launch)
        sim.start()
        assertEquals(3, sim.state.selected)
        assertNull(sim.state.launch)
    }

    // Messages

    @Test
    fun a_message_goes_when_shown_and_its_action_posts() {
        val sim = Sim().start()
        sim.openEdit()
        sim.answers["/orders/1042"] = { request ->
            if (request.method == "POST") Sim.native(request.url, Fixtures.envelope("ops-saved")) else Sim.native(request.url, Fixtures.envelope("order"))
        }
        sim.send(Event.Acted(sim.front.key, Submit("order-form")))
        sim.settle()
        val message = sim.state.messages.single()
        sim.send(Event.MessageActed(message.id))
        assertTrue(sim.state.messages.isEmpty())
        assertEquals(HttpRequest("POST", "/orders/1042/undo", Forms.payload(emptyList())), sim.lastCall())
    }

    @Test
    fun signing_in_from_a_screen_with_no_chrome_asks_for_the_shell_again() {
        val sim = Sim().start()
        sim.go("/sign-in")
        sim.answers["/sign-in"] = { request ->
            if (request.method == "POST") {
                Sim.native(request.url, "{\"navree\":1,\"ops\":[{\"op\":\"navigate\",\"href\":\"/\"}]}")
            } else {
                Sim.native(request.url, Fixtures.envelope("sign-in"))
            }
        }
        val before = sim.shellFetches()
        sim.send(Event.Acted(sim.front.key, Submit("sign-in-form")))
        sim.settle()
        assertEquals(before + 1, sim.shellFetches())
        assertTrue(sim.state.modals.isEmpty())
        assertEquals("/", sim.front.url)
    }

    // The server's own demo

    private fun store(): Sim {
        val sim = Sim()
        fun real(name: String): (HttpRequest) -> HttpResponse = { Sim.native(it.url, Fixtures.text("store-demo/$name.json")) }
        sim.answers[SHELL_PATH] = { Sim.native(SHELL_PATH, Fixtures.text("store-demo/shell.json")) }
        sim.answers["/"] = real("dashboard")
        sim.answers["/orders"] = real("orders")
        sim.answers["/orders/1"] = real("order")
        sim.answers["/orders/1/steps"] = real("order-steps")
        sim.answers["/orders/200000/peek"] = real("order-peek")
        sim.answers["/inventory"] = real("inventory")
        return sim.start()
    }

    @Test
    fun the_real_demo_starts_in_the_web_view_and_its_orders_are_native() {
        val sim = store()
        assertEquals(3, sim.state.tabs.size)
        assertTrue("the dashboard has a chart, so it is the web view", sim.front.content is Content.Web)
        sim.send(Event.DestinationChosen(1))
        sim.settle()
        assertEquals("Orders", sim.front.screen?.title)
        assertNotNull(sim.front.screen!!.find("orders"))
    }

    @Test
    fun a_row_of_the_real_orders_table_peeks_into_a_sheet() {
        val sim = store()
        sim.send(Event.DestinationChosen(1))
        sim.settle()
        val key = sim.front.key
        sim.send(Event.Acted(key, Submit(form = "@/orders/200000/peek")))
        assertEquals(HttpRequest("GET", "/orders/200000/peek"), sim.lastCall())
        sim.settle()
        assertEquals("the screen keeps its address: the answer was ops, not a screen", "/orders", sim.front.url)
        assertEquals(listOf("order"), sim.front.local.opened)
        val sheet = sim.front.screen!!.overlay("order") as com.navree.client.protocol.SheetNode
        assertEquals("Order CS-20481", sheet.title)
        assertTrue(sheet.children.isNotEmpty())
        sim.send(Event.BackPressed)
        assertTrue(sim.front.local.opened.isEmpty())
    }

    @Test
    fun a_region_that_is_to_be_fetched_later_is_fetched_once_and_filled() {
        val sim = store()
        val page = "{\"navree\":1,\"screen\":{\"title\":\"Order\",\"body\":{\"t\":\"stack\",\"children\":[" +
            "{\"t\":\"text\",\"value\":\"Steps\"},{\"t\":\"region\",\"id\":\"steps\",\"src\":\"/orders/1/steps\",\"lazy\":true}]}}}"
        sim.answers["/orders/9"] = { Sim.native(it.url, page) }
        sim.send(Event.Acted(sim.front.key, Navigate("/orders/9")))
        sim.answer(sim.waiting().single { it.call.url == "/orders/9" })
        assertEquals("the region is asked for once the screen is there", "/orders/1/steps", sim.waiting().single().call.url)
        assertTrue(sim.front.screen!!.regions().single().children.isEmpty())
        sim.settle()
        assertTrue("what came back is in the region", sim.front.screen!!.regions().single().children.isNotEmpty())
        assertEquals(1, sim.effects.count { it is Effect.Fetch && it.call.url == "/orders/1/steps" })

        val invalidated = "{\"navree\":1,\"ops\":[{\"op\":\"invalidate\",\"target\":\"steps\"}]}"
        sim.answers["/orders/9/nudge"] = { Sim.native(it.url, invalidated) }
        sim.send(Event.Acted(sim.front.key, com.navree.client.protocol.Request("post", "/orders/9/nudge")))
        sim.settle()
        assertEquals("told it is out of date, the region alone is fetched again", 2, sim.effects.count { it is Effect.Fetch && it.call.url == "/orders/1/steps" })
        assertEquals(1, sim.effects.count { it is Effect.Fetch && it.call.url == "/orders/9" })
    }

    @Test
    fun the_reducer_gives_the_same_answer_for_the_same_state_and_event() {
        val sim = Sim().start()
        val event = Event.Acted(sim.front.key, Navigate("/orders/1042"))
        assertEquals(Reducer.reduce(sim.state, event), Reducer.reduce(sim.state, event))
    }
}
