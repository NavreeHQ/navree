package com.navree.client.platform

import org.junit.Assert.assertEquals
import org.junit.Test

/**
 * The cookies the answers to the client's own requests set, and which of
 * them go back into the one store when it is emptied because another person
 * is signed in. The store here is the test's own: what it holds for an
 * address, as it would send it.
 */
class OwnCookiesTest {
    private var clock = 1_000_000L
    private val own = OwnCookies { clock }
    private val later = 9_000_000L

    /** What the one store holds, by address: the web view writes here too, and nothing of that passes through [own]. */
    private val store = HashMap<String, String>()

    /** An answer of the app's server to one of the client's own requests sets a cookie: it goes into the store and is remembered. */
    private fun answerSets(name: String, value: String, path: String = "/", host: String = "shop.example", attributes: String = "; secure; httponly", expiresAt: Long = later) {
        val address = "https://$host/sign-in"
        val probe = "https://$host$path"
        own.set(OwnCookies.Given(address, probe, "$name=$value; path=$path$attributes", name, value, host, path, expiresAt))
        if (expiresAt > clock) hold(probe, name, value) else drop(probe, name)
    }

    private fun hold(address: String, name: String, value: String) {
        drop(address, name)
        store[address] = listOfNotNull(store[address]?.takeIf { it.isNotEmpty() }, "$name=$value").joinToString("; ")
    }

    private fun drop(address: String, name: String) {
        store[address] = store[address].orEmpty().split("; ").filter { it.isNotEmpty() && it.substringBefore('=') != name }.joinToString("; ")
    }

    /** The headers that go back into the store when it is emptied now. */
    private fun givenBack(): List<String> = own.stillHeld { store[it] }.map { it.header }

    @Test
    fun the_cookies_given_back_are_the_ones_the_clients_own_answers_set_each_as_it_was_set() {
        answerSets("session", "s-karan")
        answerSets("XSRF-TOKEN", "x-karan", attributes = "; secure")
        // The web view's side of the one store: a page's script set this, and no answer to the client's own requests did.
        hold("https://shop.example/", "chat", "c-anjali")
        hold("https://pay.example/", "portal", "p-anjali")
        assertEquals(listOf("session=s-karan; path=/; secure; httponly", "XSRF-TOKEN=x-karan; path=/; secure"), givenBack())
        assertEquals("each goes back for the address its answer set it for", listOf("https://shop.example/sign-in", "https://shop.example/sign-in"), own.stillHeld { store[it] }.map { it.address })
    }

    @Test
    fun a_later_answers_cookie_takes_the_place_of_an_earlier_one_of_its_name_path_and_domain() {
        // The session of the person before, then the sign-in of the one who is here now.
        answerSets("session", "s-anjali")
        answerSets("session", "s-karan")
        assertEquals(listOf("session=s-karan; path=/; secure; httponly"), givenBack())
        // The same name on another path, or at another host, is another cookie.
        answerSets("session", "s-admin", path = "/admin")
        answerSets("session", "s-pay", host = "pay.shop.example")
        assertEquals(3, givenBack().size)
    }

    @Test
    fun a_cookie_a_page_has_changed_since_the_answer_set_it_is_not_given_back() {
        answerSets("session", "s-karan")
        answerSets("theme", "dark", attributes = "")
        // A page's script wrote over one of them, and deleted nothing.
        hold("https://shop.example/", "theme", "light")
        assertEquals("what the store holds now is the page's, and what the answer set is no longer there to give back", listOf("session=s-karan; path=/; secure; httponly"), givenBack())
        // And one the web view deleted is not brought back.
        drop("https://shop.example/", "session")
        assertEquals(emptyList<String>(), givenBack())
    }

    @Test
    fun a_cookie_an_answer_took_away_or_that_has_run_out_is_not_given_back() {
        answerSets("session", "s-karan")
        answerSets("flash", "saved", expiresAt = clock + 60_000)
        // An answer expires the session's cookie: that is how a server takes one away.
        answerSets("session", "", expiresAt = 0)
        assertEquals(listOf("flash=saved; path=/; secure; httponly"), givenBack())
        clock += 120_000
        assertEquals("a minute's cookie is gone two minutes on, whatever the store still says", emptyList<String>(), givenBack())
    }

    @Test
    fun after_the_store_was_emptied_only_what_went_back_is_remembered() {
        answerSets("session", "s-karan")
        answerSets("theme", "dark", attributes = "")
        hold("https://shop.example/", "theme", "light")
        val back = own.stillHeld { store[it] }
        own.only(back)
        // The page's value is gone with the rest of the store; the answer's old value is not brought back the next time either.
        hold("https://shop.example/", "theme", "dark")
        assertEquals(listOf("session=s-karan; path=/; secure; httponly"), givenBack())
    }

    @Test
    fun after_a_sign_out_nothing_is_remembered() {
        answerSets("session", "s-karan")
        own.forget()
        assertEquals(emptyList<String>(), givenBack())
    }

    @Test
    fun only_so_many_are_remembered_and_the_oldest_go_first() {
        repeat(250) { answerSets("c$it", "v$it") }
        val back = givenBack()
        assertEquals(200, back.size)
        assertEquals("c50=v50; path=/; secure; httponly", back.first())
        assertEquals("c249=v249; path=/; secure; httponly", back.last())
    }
}
