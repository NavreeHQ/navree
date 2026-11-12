package com.navree.client.runtime

import com.navree.client.Fixtures
import com.navree.client.protocol.NATIVE_CONTENT_TYPE
import com.navree.client.protocol.SHELL_PATH

/**
 * The client with no device and no network: events go into the reducer,
 * its effects are kept, and fetches are answered from a table of addresses.
 */
class Sim(origin: String = "https://store.example.in") {
    val origin: Origin = requireNotNull(Origin.parse(origin))
    var state = ClientState(this.origin)
        private set

    /** Every effect so far, oldest first. */
    val effects = mutableListOf<Effect>()

    /** What each address answers with: a fixture's name, or a whole response. */
    val answers = mutableMapOf<String, (HttpRequest) -> HttpResponse>(
        SHELL_PATH to { native(SHELL_PATH, Fixtures.text("shell.json")) },
        "/" to fixture("home"),
        "/orders" to fixture("orders"),
        "/orders?after=1033" to fixture("orders-more"),
        "/orders/1042" to fixture("order"),
        "/orders/1042/edit" to fixture("order-edit"),
        "/orders/new" to fixture("order-new"),
        "/products" to fixture("products"),
        "/customers" to fixture("customers"),
        "/settings" to fixture("settings"),
        "/reports" to fixture("web"),
        "/sign-in" to fixture("sign-in"),
    )

    fun fixture(name: String): (HttpRequest) -> HttpResponse = { request -> native(request.url, Fixtures.envelope(name)) }

    fun send(event: Event): List<Effect> {
        val step = Reducer.reduce(state, event)
        state = step.state
        effects += step.effects
        return step.effects
    }

    /** The fetches asked for and not yet answered. */
    fun waiting(): List<Effect.Fetch> = effects.filterIsInstance<Effect.Fetch>().filter { it.request in state.pending }

    /** Answers one waiting fetch from the table. */
    fun answer(fetch: Effect.Fetch) {
        val answer = answers[fetch.call.url] ?: { HttpResponse.Answer(fetch.call.url, 404, "text/plain", "Not found") }
        send(Event.Answered(fetch.request, answer(fetch.call)))
    }

    /** Answers fetches until none is waiting. */
    fun settle() {
        var turns = 0
        while (true) {
            val next = waiting().firstOrNull() ?: return
            answer(next)
            check(++turns < 50) { "The client keeps asking: ${waiting().map { it.call.url }}" }
        }
    }

    /** Starts the app and lets the shell and the first screen arrive. */
    fun start(): Sim {
        send(Event.Started)
        settle()
        return this
    }

    val front: Entry get() = requireNotNull(state.front) { "No screen is in front" }

    /** The addresses of the stack in front, bottom first. */
    fun urls(): List<String> = state.frontStack.map { it.url }

    /** The last request sent, whatever became of it. */
    fun lastCall(): HttpRequest = effects.filterIsInstance<Effect.Fetch>().last().call

    companion object {
        fun native(url: String, body: String, status: Int = 200): HttpResponse =
            HttpResponse.Answer(url, status, "$NATIVE_CONTENT_TYPE; charset=utf-8", body)
    }
}
