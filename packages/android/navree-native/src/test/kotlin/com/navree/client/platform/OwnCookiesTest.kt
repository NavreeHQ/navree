package com.navree.client.platform

import org.junit.Assert.assertEquals
import org.junit.Test

/**
 * The cookies the answers to the client's own requests set, and which of
 * them go back into the one store when it is emptied and the person who is
 * signed in is to stay so. The store here is the test's own: what it holds
 * for an address, as it would send it.
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
        own.set(OwnCookies.Given(address, probe, "$name=$value; path=$path$attributes", name, value, host, path, expiresAt)) {
            if (expiresAt > clock) hold(probe, name, value) else drop(probe, name)
        }
    }

    /** The web view's side, or the store being given a cookie back: the store holds this for the address from now. */
    private fun hold(address: String, name: String, value: String) {
        drop(address, name)
        store[address] = listOfNotNull(store[address]?.takeIf { it.isNotEmpty() }, "$name=$value").joinToString("; ")
    }

    private fun drop(address: String, name: String) {
        store[address] = store[address].orEmpty().split("; ").filter { it.isNotEmpty() && it.substringBefore('=') != name }.joinToString("; ")
    }

    /** The store is emptied and given its own cookies back, as it is when another person is signed in: the headers that went back in. */
    private fun emptiedAndGivenBack(): List<String> {
        val back = own.giving { store[it] }
        store.clear()
        return own.giveBack(back) { hold(it.probe, it.name, it.value) }.map { it.header }
    }

    @Test
    fun the_cookies_given_back_are_the_ones_the_clients_own_answers_set_each_as_it_was_set() {
        answerSets("session", "s-karan")
        answerSets("XSRF-TOKEN", "x-karan", attributes = "; secure")
        // The web view's side of the one store: a page's script set this, and no answer to the client's own requests did.
        hold("https://shop.example/", "chat", "c-anjali")
        hold("https://pay.example/", "portal", "p-anjali")
        assertEquals(listOf("session=s-karan; path=/; secure; httponly", "XSRF-TOKEN=x-karan; path=/; secure"), emptiedAndGivenBack())
        assertEquals("what the web view's side put there is gone, at every origin", mapOf("https://shop.example/" to "session=s-karan; XSRF-TOKEN=x-karan"), store)
    }

    @Test
    fun a_later_answers_cookie_takes_the_place_of_an_earlier_one_of_its_name_path_and_domain() {
        // The session of the person before, then the sign-in of the one who is here now.
        answerSets("session", "s-anjali")
        answerSets("session", "s-karan")
        // The same name on another path, or at another host, is another cookie.
        answerSets("session", "s-admin", path = "/admin")
        answerSets("session", "s-pay", host = "pay.shop.example")
        assertEquals(
            listOf("session=s-karan; path=/; secure; httponly", "session=s-admin; path=/admin; secure; httponly", "session=s-pay; path=/; secure; httponly"),
            emptiedAndGivenBack(),
        )
    }

    @Test
    fun a_cookie_a_page_has_changed_since_the_answer_set_it_is_not_given_back() {
        answerSets("session", "s-karan")
        answerSets("theme", "dark", attributes = "")
        // A page's script wrote over one of them.
        hold("https://shop.example/", "theme", "light")
        assertEquals("what the store held was the page's, and what the answer set was no longer there to give back", listOf("session=s-karan; path=/; secure; httponly"), emptiedAndGivenBack())
        // What was not given back is not remembered: it does not come back the next time either, whatever the store says.
        hold("https://shop.example/", "theme", "dark")
        assertEquals(listOf("session=s-karan; path=/; secure; httponly"), emptiedAndGivenBack())
        // And one the web view deleted is not brought back.
        drop("https://shop.example/", "session")
        assertEquals(emptyList<String>(), emptiedAndGivenBack())
    }

    @Test
    fun a_cookie_an_answer_took_away_or_that_has_run_out_is_not_given_back() {
        answerSets("session", "s-karan")
        answerSets("flash", "saved", expiresAt = clock + 60_000)
        // An answer expires the session's cookie: that is how a server takes one away.
        answerSets("session", "", expiresAt = 0)
        assertEquals(listOf("flash=saved; path=/; secure; httponly"), emptiedAndGivenBack())
        clock += 120_000
        assertEquals("a minute's cookie is gone two minutes on, whatever the store still says", emptyList<String>(), emptiedAndGivenBack())
    }

    @Test
    fun what_was_taken_out_while_the_store_stood_empty_goes_back_in_and_what_was_written_there_meanwhile_does_not() {
        answerSets("session", "s-karan")
        hold("https://shop.example/", "chat", "c-anjali")
        // What pages stored is about to be taken away: the store is emptied first, and stays empty while that is done.
        own.takeOut { store[it] }
        store.clear()
        // A service worker which that started writes a cookie, with no request the client is asked about.
        hold("https://shop.example/", "written", "by-a-worker")
        assertEquals("the store held no session while that was done", "written=by-a-worker", store["https://shop.example/"])
        assertEquals(listOf("session=s-karan; path=/; secure; httponly"), emptiedAndGivenBack())
        assertEquals("and the worker's cookie went with the second emptying", mapOf("https://shop.example/" to "session=s-karan"), store)
        // Nothing is out any more: the next emptying gives back what the store holds of the client's own, and no more.
        drop("https://shop.example/", "session")
        assertEquals(emptyList<String>(), emptiedAndGivenBack())
    }

    @Test
    fun what_goes_back_is_what_the_answers_have_left_by_the_time_the_store_says_it_is_empty() {
        answerSets("session", "s-first")
        answerSets("flash", "saved")
        // It is decided what goes back, and the store is asked to empty itself. It is slow to answer.
        val back = own.giving { store[it] }
        // Meanwhile an answer sets the session's cookie anew, another sets one that was not there, and another takes one away.
        answerSets("session", "s-second")
        answerSets("workspace", "w-7")
        answerSets("flash", "", expiresAt = 0)
        // Now the store says it is empty: the emptying took the later cookies with the rest.
        store.clear()
        val given = own.giveBack(back) { hold(it.probe, it.name, it.value) }.map { it.header }
        assertEquals("the later cookie and not the one it took the place of, the new one, and not the one an answer took away", listOf("session=s-second; path=/; secure; httponly", "workspace=w-7; path=/; secure; httponly"), given)
        assertEquals(mapOf("https://shop.example/" to "session=s-second; workspace=w-7"), store)
    }

    @Test
    fun a_cookie_set_while_the_store_stood_empty_goes_back_with_the_ones_taken_out() {
        answerSets("session", "s-karan")
        own.takeOut { store[it] }
        store.clear()
        // The queue waited no longer, and an answer to one of the client's own requests came in: it set a cookie in the empty store.
        answerSets("XSRF-TOKEN", "x-karan", attributes = "; secure")
        assertEquals(listOf("session=s-karan; path=/; secure; httponly", "XSRF-TOKEN=x-karan; path=/; secure"), emptiedAndGivenBack())
    }

    @Test
    fun after_a_sign_out_nothing_is_remembered_and_nothing_is_out() {
        answerSets("session", "s-karan")
        own.takeOut { store[it] }
        own.forget()
        store.clear()
        assertEquals(emptyList<String>(), emptiedAndGivenBack())
    }

    @Test
    fun only_so_many_are_remembered_and_the_oldest_go_first() {
        repeat(250) { answerSets("c$it", "v$it") }
        val back = emptiedAndGivenBack()
        assertEquals(200, back.size)
        assertEquals("c50=v50; path=/; secure; httponly", back.first())
        assertEquals("c249=v249; path=/; secure; httponly", back.last())
    }
}
