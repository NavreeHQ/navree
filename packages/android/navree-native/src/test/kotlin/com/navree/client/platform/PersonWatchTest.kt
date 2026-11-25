package com.navree.client.platform

import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.File
import java.nio.file.Files
import java.security.MessageDigest

/**
 * Who the core says is signed in, heard each time it says so, and the mark
 * of whom what is kept on the device is kept for, in a folder of the test's
 * own. The client is told of no end of a session when one ran out and
 * somebody else signed in: this is how it knows to take away what was the
 * last person's, in the same run or at the next start.
 */
class PersonWatchTest {
    private val folder = Files.createTempDirectory("navree-person").toFile()
    private val file = File(folder, PersonWatch.MARK)
    private val shop = "https://shop.example:443"

    private val anjali = "Anjali Verma" to "anjali.verma@example.com"
    private val karan = "Karan Malhotra" to "karan.malhotra@example.com"
    private val pooja = "Pooja Khanna" to "pooja.khanna@example.com"

    /** Two accounts have this name, and neither has an email. */
    private val harpreet = "Harpreet Singh" to null

    @After
    fun clear() {
        folder.deleteRecursively()
    }

    /** The watch as one run of the app has it: what it remembers is read from the file, as a start of the app reads it. */
    private fun watch(origin: String = shop) = PersonWatch(file, origin)

    /** Hears a person as a core that gives no key names them: by name and email. */
    private fun PersonWatch.hear(who: Pair<String, String?>) = hear(null, who.first, who.second)

    /** Hears a person as a core that gives a key names them. */
    private fun PersonWatch.hear(key: String, who: Pair<String, String?>) = hear(key, who.first, who.second)

    /** Hears, and writes the mark down where the word says so, as the client does in the store's turn. */
    private fun PersonWatch.hearAndKeep(who: Pair<String, String?>): Heard = hear(who).also { if (it.write) keep(it) }.heard

    private fun PersonWatch.hearAndKeep(key: String, who: Pair<String, String?>): Heard = hear(key, who).also { if (it.write) keep(it) }.heard

    /** The mark the file holds, or null when there is no file. */
    private fun kept(): String? = if (file.isFile) file.readLines()[1] else null

    private fun mark(who: Pair<String, String?>) = PersonWatch.markOf(who.first, who.second)

    private fun sha(written: String) = MessageDigest.getInstance("SHA-256").digest(written.encodeToByteArray()).joinToString("") { "%02X".format(it) }

    @Test
    fun the_mark_of_a_person_is_made_from_name_and_email_the_way_every_client_makes_it() {
        assertEquals(sha("12:Anjali Verma+anjali.verma@example.com"), PersonWatch.markOf("Anjali Verma", "anjali.verma@example.com"))
        assertEquals(sha("12:Anjali Verma-"), PersonWatch.markOf("Anjali Verma", null))
        assertNotEquals("a person with no email and one whose email is empty are two", PersonWatch.markOf("Anjali Verma", null), PersonWatch.markOf("Anjali Verma", ""))
        assertNotEquals("a name is never taken for a name and an email", PersonWatch.markOf("Anjali+verma@example.com", null), PersonWatch.markOf("Anjali", "verma@example.com"))
        assertNotEquals(mark(anjali), mark(karan))
        assertEquals("sixty-four hex digits, from which neither is read", 64, mark(anjali).length)
        assertFalse(mark(anjali).contains("Anjali", ignoreCase = true))
    }

    @Test
    fun where_the_core_gives_no_key_two_people_with_the_same_name_and_no_email_are_one_and_a_person_who_is_renamed_is_another_once() {
        val watch = watch()
        assertEquals(Heard.First, watch.hearAndKeep("Karan Malhotra" to null))
        assertEquals("what cannot be told apart is not", Heard.Same, watch.hearAndKeep("Karan Malhotra" to null))

        val renamed = watch.hear("Karan S. Malhotra" to null)
        assertEquals("a new name is another person to the client", Heard.Another, renamed.heard)
        watch.changed(renamed)
        assertEquals("once: from then on it is the name remembered", Heard.Same, watch.hearAndKeep("Karan S. Malhotra" to null))
    }

    @Test
    fun another_person_named_with_no_sign_out_between_is_a_change_of_person() {
        val watch = watch()
        assertEquals("the first person named follows nobody", Heard.First, watch.hearAndKeep(anjali))
        assertEquals(mark(anjali), kept())
        assertEquals(Heard.Same, watch.hearAndKeep(anjali))
        // Her session ran out; while the core names nobody nothing is heard; then somebody else signed in.
        assertEquals(Heard.Another, watch.hear(karan).heard)
        assertEquals("and is the one named from then on", Heard.Same, watch.hear(karan).heard)
    }

    @Test
    fun the_mark_is_read_at_the_start_so_a_change_of_person_while_the_app_was_closed_is_seen() {
        watch().hearAndKeep(anjali)
        // The app is closed. Her session runs out, and the next start is somebody else's.
        assertEquals(Heard.Another, watch().hear(karan).heard)
        // Where it is hers again, nothing changed.
        val hers = watch().hear(anjali)
        assertEquals(Heard.Same, hers.heard)
        assertFalse("the mark kept is hers already", hers.write)
    }

    @Test
    fun the_mark_of_the_person_before_stays_until_every_step_has_finished_and_a_run_that_ends_in_the_middle_leaves_the_next_start_to_do_it_over() {
        val first = watch()
        first.hearAndKeep(anjali)
        val word = first.hear(karan)
        assertEquals(Heard.Another, word.heard)
        assertFalse("nothing is written for him yet", word.write)
        first.keep(word)
        assertEquals("the mark on the disk is still hers", mark(anjali), kept())
        // He is named again and again while her things are being taken away: nothing is written.
        assertEquals(Heard.Same, first.hearAndKeep(karan))
        assertEquals(mark(anjali), kept())

        // The app ends here. The next start remembers her, hears him, and does the change over.
        val second = watch()
        val again = second.hear(karan)
        assertEquals(Heard.Another, again.heard)
        // This time every step finishes.
        second.changed(again)
        assertEquals("what is kept on the device is kept for him now", mark(karan), kept())
        assertEquals(Heard.Same, watch().hear(karan).heard)
    }

    @Test
    fun when_yet_another_person_is_named_while_the_steps_run_the_mark_is_theirs_to_write() {
        val watch = watch()
        watch.hearAndKeep(anjali)
        val his = watch.hear(karan)
        val hers = watch.hear(pooja)
        assertEquals(Heard.Another, hers.heard)
        // The steps begun for him finish first: they say nothing of what was kept since for her.
        watch.changed(his)
        assertEquals(mark(anjali), kept())
        watch.changed(hers)
        assertEquals(mark(pooja), kept())
    }

    @Test
    fun a_sign_out_forgets_who_was_named_so_whoever_is_named_next_is_the_first() {
        val watch = watch()
        watch.hearAndKeep(anjali)
        // She signed out: the end of the session takes everything of hers, the mark with it.
        watch.signedOut()
        assertFalse("no mark is left on the disk", file.exists())
        assertEquals(Heard.First, watch.hearAndKeep(karan))
        assertEquals(mark(karan), kept())
        // And across a start: a run that ended signed out remembers nobody.
        watch.signedOut()
        assertEquals(Heard.First, watch().hear(anjali).heard)
    }

    @Test
    fun what_was_heard_before_a_sign_out_writes_nothing_after_it() {
        val watch = watch()
        val heard = watch.hear(anjali)
        assertTrue(heard.write)
        watch.signedOut()
        // The write was still waiting for its turn when she signed out.
        watch.keep(heard)
        assertFalse(file.exists())

        watch.hearAndKeep(anjali)
        val change = watch.hear(karan)
        watch.signedOut()
        // So were the steps of a change of person.
        watch.changed(change)
        assertFalse(file.exists())
        assertEquals(Heard.First, watch.hear(pooja).heard)
    }

    @Test
    fun a_mark_is_asked_to_be_written_once_and_again_only_when_it_was_not() {
        val watch = watch()
        val first = watch.hear(anjali)
        assertTrue(first.write)
        assertFalse("the write is on its way: it is not asked for a second time", watch.hear(anjali).write)
        watch.keep(first)
        assertEquals(mark(anjali), kept())
        assertFalse(watch.hear(anjali).write)
    }

    @Test
    fun a_mark_kept_at_another_origin_is_not_compared_with() {
        watch().hearAndKeep(anjali)
        // The same build is pointed at another server, where somebody else is signed in.
        val elsewhere = watch("https://staging.shop.example:443")
        assertEquals("nobody was named at this origin before", Heard.First, elsewhere.hearAndKeep(karan))
        assertEquals("and what is kept from now on is kept for him, there", listOf("https://staging.shop.example:443", mark(karan)), file.readLines())
    }

    @Test
    fun a_mark_that_does_not_read_remembers_nobody() {
        file.writeText(shop + "\n")
        assertEquals(Heard.First, watch().hear(anjali).heard)
        file.writeText("\n\n")
        assertEquals(Heard.First, watch().hear(anjali).heard)
        // Something is there under the mark's name, and it does not read as a file.
        file.delete()
        assertTrue(file.mkdir())
        assertEquals(Heard.First, watch().hear(anjali).heard)
    }

    @Test
    fun two_people_of_one_name_and_no_email_are_told_apart_by_the_key_the_core_gives() {
        val watch = watch()
        assertEquals(Heard.First, watch.hearAndKeep("p1", harpreet))
        assertEquals(PersonWatch.markOfKey("p1"), kept())
        assertEquals(Heard.Same, watch.hearAndKeep("p1", harpreet))
        // The other account of that name signs in, with no sign-out between.
        val other = watch.hear("p2", harpreet)
        assertEquals("the same name, no email, and another person", Heard.Another, other.heard)
        assertEquals("the mark on the disk is still the first one's", PersonWatch.markOfKey("p1"), kept())
        watch.changed(other)
        assertEquals(PersonWatch.markOfKey("p2"), kept())
        // And across a start of the app.
        assertEquals(Heard.Same, watch().hear("p2", harpreet).heard)
        assertEquals(Heard.Another, watch().hear("p1", harpreet).heard)
    }

    @Test
    fun a_person_the_core_names_by_key_is_the_same_person_under_a_new_name_or_email() {
        val watch = watch()
        watch.hearAndKeep("p1", anjali)
        assertEquals("where the core says who it is, a new name is no new person", Heard.Same, watch.hearAndKeep("p1", "Anjali V. Sharma" to "anjali.sharma@example.com"))
        assertEquals(PersonWatch.markOfKey("p1"), kept())
        assertEquals(Heard.Same, watch().hear("p1", "Anjali Sharma" to null).heard)
    }

    @Test
    fun the_mark_of_a_key_is_told_from_one_made_of_a_name_and_says_nothing_of_the_key() {
        val mark = PersonWatch.markOfKey("p1")
        assertEquals("what it begins with, and sixty-four hex digits", "key:" + sha("p1"), mark)
        assertNotEquals(mark, PersonWatch.markOfKey("p2"))
        // A key that reads as the mark of a name and email is still the mark of a key.
        assertNotEquals(mark(anjali), PersonWatch.markOfKey(mark(anjali)))
        // And no mark made of a name and email begins as a key's does: it is hex digits only.
        assertFalse(PersonWatch.markOf("key:", null).startsWith("key:"))
    }

    @Test
    fun a_mark_made_from_name_and_email_is_never_the_mark_of_a_key_so_the_first_key_names_another_person_even_under_the_same_name_and_email() {
        // A run whose core gave no key remembered her by name and email.
        watch().hearAndKeep(anjali)
        assertEquals(mark(anjali), kept())
        // The next run's core gives a key, and names somebody of the very same name and email.
        val watch = watch()
        val word = watch.hear("p1", anjali)
        assertEquals("a name and email say too little to be taken for the person a key names", Heard.Another, word.heard)
        assertFalse("nothing is written for the key yet", word.write)
        watch.keep(word)
        assertEquals("the mark on the disk stays the one made from the name and email until what was kept is gone", mark(anjali), kept())
        // A run that ends here hears the same again, and does it over.
        assertEquals(Heard.Another, watch().hear("p1", anjali).heard)
        // Every step finished: from now the key is what is remembered, and it is the same person each time it is given.
        watch.changed(word)
        assertEquals(PersonWatch.markOfKey("p1"), kept())
        assertEquals(Heard.Same, watch().hear("p1", anjali).heard)
        // And the key decides from then on: another account of her name and email is another person.
        assertEquals(Heard.Another, watch().hear("p2", anjali).heard)
    }

    @Test
    fun a_mark_made_from_name_and_email_is_another_person_when_the_first_key_comes_with_another_name_or_email() {
        watch().hearAndKeep(anjali)
        // Her session ran out while the app was being updated, and he signed in.
        val word = watch().hear("p2", karan)
        assertEquals(Heard.Another, word.heard)
        assertFalse(word.write)
        assertEquals("her mark stays until what was kept for her is gone", mark(anjali), kept())
    }

    @Test
    fun the_mark_of_a_key_is_never_the_mark_of_a_name_and_email_so_a_person_named_with_no_key_after_one_is_another_even_under_the_same_name_and_email() {
        watch().hearAndKeep("p1", anjali)
        val word = watch().hear(anjali)
        assertEquals(Heard.Another, word.heard)
        assertEquals("what was kept under the key stays marked so until it is gone", PersonWatch.markOfKey("p1"), kept())
    }

    @Test
    fun name_and_email_are_compared_only_where_neither_side_has_a_key() {
        // Neither the mark kept nor the core has a key: the same name and email are the same person.
        watch().hearAndKeep(anjali)
        assertEquals(Heard.Same, watch().hear(anjali).heard)
        assertEquals(Heard.Another, watch().hear(karan).heard)
        // Both have a key: the same name and email under two keys are two people, and two names under one key are one.
        file.delete()
        watch().hearAndKeep("p1", anjali)
        assertEquals(Heard.Another, watch().hear("p2", anjali).heard)
        assertEquals(Heard.Same, watch().hear("p1", karan).heard)
    }

    @Test
    fun a_key_with_nothing_in_it_is_no_key() {
        val word = watch().hear("", anjali)
        assertEquals(Heard.First, word.heard)
        assertEquals("she is remembered by name and email", mark(anjali), word.mark)
    }

    private val ravneet = "Ravneet Kaur" to "ravneet.kaur@example.com"

    /**
     * Her key as a run before the install had a secret was given it, and as
     * the core makes it with the secret; and those of another account of her
     * name and email. They are the keys the core's conversations
     * `person-key-steady.json` and `person-key-was.json` record.
     */
    private val ravneetPlain = "1c54d473261f6f17cb6193202ce56588716e85757946078ac43d03b22f93a35e"
    private val ravneetKeyed = "k2:17d27bf8db683479c652471e31c96ac7d03e690c41c4f94f56ca99de886e389e"
    private val otherPlain = "1bc3dcdc8ebf5d231237007ccc4903b602f64185813b65bc5b6d1f5f798fcab7"
    private val otherKeyed = "k2:6bd1a2cdf43aaf682ec4d5da88fc8ca12d82976ffebb660e10e75f894e0c82d7"

    /** Hears a person as a core that has the install's secret names them, with what it says their plain key was. */
    private fun PersonWatch.hear(key: String, who: Pair<String, String?>, was: String?) = hear(key, who.first, who.second, was)

    @Test
    fun the_mark_of_a_key_made_with_the_installs_secret_is_told_from_the_mark_of_a_plain_key() {
        val plain = PersonWatch.markOfKey(ravneetPlain)
        val keyed = PersonWatch.markOfKey(ravneetKeyed)
        assertTrue(plain.startsWith("key:"))
        assertTrue("the mark of a key made with the secret begins as the mark of a plain one", keyed.startsWith("k2:"))
        assertEquals("one key has two marks", keyed, PersonWatch.markOfKey(ravneetKeyed))
        assertNotEquals(keyed, PersonWatch.markOfKey(otherKeyed))
        assertFalse("the key is read from its mark", keyed.contains(ravneetKeyed.removePrefix("k2:"), ignoreCase = true))
        assertFalse("a mark made of a name and email begins as a key's", mark(ravneet).startsWith("k2:"))

        // Whether a start asks the core what a person's plain key was: only where the mark it reads is of a plain key.
        assertFalse("nothing is kept at a first start", watch().keepsPlainKey)
        watch().hearAndKeep(ravneetPlain, ravneet)
        assertTrue("a start that reads the mark of a plain key does not say so", watch().keepsPlainKey)
        assertFalse("a mark kept at another origin is asked about", watch("https://other.example:443").keepsPlainKey)
        file.delete()
        watch().hearAndKeep(ravneetKeyed, ravneet)
        assertFalse("a start that reads the mark of a key made with the secret asks for a plain one", watch().keepsPlainKey)
        file.delete()
        watch().hearAndKeep(ravneet)
        assertFalse("a mark made of a name and email is taken for the mark of a plain key", watch().keepsPlainKey)
    }

    @Test
    fun a_mark_kept_of_a_plain_key_is_the_person_whose_plain_key_the_core_says_it_was_and_the_mark_of_the_key_they_have_now_takes_its_place() {
        // A run before the install had a secret remembered her by her plain key.
        watch().hearAndKeep(ravneetPlain, ravneet)
        assertEquals(PersonWatch.markOfKey(ravneetPlain), kept())
        // The first start after the update: the core names her by the key it makes with the secret, and says what her key was.
        val watch = watch()
        val word = watch.hear(ravneetKeyed, ravneet, ravneetPlain)
        assertEquals("the person the mark was kept for is taken for another when her key is first made with the secret", Heard.Same, word.heard)
        assertEquals(PersonWatch.markOfKey(ravneetKeyed), word.mark)
        assertTrue("the mark of the key she has now is not written down", word.write)
        assertEquals("nothing is written before the store's turn", PersonWatch.markOfKey(ravneetPlain), kept())
        watch.keep(word)
        assertEquals(PersonWatch.markOfKey(ravneetKeyed), kept())
        assertFalse("the mark is written twice", watch.hear(ravneetKeyed, ravneet, ravneetPlain).write)
        // The starts after read the mark of her new key, ask for no plain one, and hear her as the same with or without it.
        assertFalse(watch().keepsPlainKey)
        assertEquals(Heard.Same, watch().hear(ravneetKeyed, ravneet).heard)
        assertEquals(Heard.Same, watch().hear(ravneetKeyed, ravneet, ravneetPlain).heard)
        // And under the key she has now a new name is still her, and another account is not.
        assertEquals(Heard.Same, watch().hear(ravneetKeyed, "Ravneet K. Sandhu" to null).heard)
        assertEquals(Heard.Another, watch().hear(otherKeyed, ravneet, otherPlain).heard)
    }

    @Test
    fun a_mark_kept_of_a_plain_key_is_another_persons_when_the_core_says_another_plain_key_or_none() {
        watch().hearAndKeep(ravneetPlain, ravneet)
        // Her session ran out, and another account of her name and email signs in at the first start after the update.
        val other = watch().hear(otherKeyed, ravneet, otherPlain)
        assertEquals("another account is left what was kept for her", Heard.Another, other.heard)
        assertFalse(other.write)
        assertEquals("her mark stays until what was kept for her is gone", PersonWatch.markOfKey(ravneetPlain), kept())
        // A core that does not say what her key was leaves the two marks two people's.
        assertEquals(Heard.Another, watch().hear(ravneetKeyed, ravneet).heard)
        assertEquals(Heard.Another, watch().hear(ravneetKeyed, ravneet, "").heard)
        // What the core says a key was holds only the mark of a plain key to a key made with the secret: not one plain key to another,
        assertEquals(Heard.Another, watch().hear(otherPlain, ravneet, ravneetPlain).heard)
        // not a mark made of a name and email,
        file.delete()
        watch().hearAndKeep(ravneet)
        assertEquals(Heard.Another, watch().hear(ravneetKeyed, ravneet, ravneetPlain).heard)
        // and not one key made with the secret to another.
        file.delete()
        watch().hearAndKeep(otherKeyed, ravneet)
        assertEquals(Heard.Another, watch().hear(ravneetKeyed, ravneet, otherKeyed).heard)
        // After a sign-out nobody is remembered, and whoever is named is the first.
        val out = watch()
        out.signedOut()
        assertFalse(out.keepsPlainKey)
        assertEquals(Heard.First, out.hear(ravneetKeyed, ravneet, ravneetPlain).heard)
    }

    @Test
    fun the_keys_the_cores_transcript_records_for_a_client_that_kept_a_plain_key_are_one_person_to_the_client() {
        val folder = File("../../../crates/navree-native-core/transcripts")
        org.junit.Assume.assumeTrue("the core's transcripts are beside the client", folder.isDirectory)
        val root = kotlinx.serialization.json.Json.parseToJsonElement(File(folder, "person-key-was.json").readText()) as kotlinx.serialization.json.JsonObject
        // Each step that names a person: their key, and what the core says their plain key was.
        val named = (root["steps"] as kotlinx.serialization.json.JsonArray).mapNotNull { step ->
            val checks = (step as kotlinx.serialization.json.JsonObject)["check"] as? kotlinx.serialization.json.JsonArray ?: return@mapNotNull null
            fun at(pointer: String) = checks.firstNotNullOfOrNull { item ->
                val check = item as kotlinx.serialization.json.JsonObject
                if ((check["at"] as? kotlinx.serialization.json.JsonPrimitive)?.content == pointer) (check["is"] as? kotlinx.serialization.json.JsonPrimitive)?.content else null
            }
            at("/app/user/key")?.let { key -> key to at("/app/user_was") }
        }
        assertEquals("two people are named in person-key-was.json", 2, named.size)
        val (hers, another) = named
        assertEquals(ravneetKeyed to ravneetPlain, hers)
        assertEquals(otherKeyed to otherPlain, another)
        // The release before kept her plain key, as person-key-steady.json records it: she is the same person, and the other account is not.
        watch().hearAndKeep(ravneetPlain, ravneet)
        assertEquals(Heard.Same, watch().hear(hers.first, ravneet, hers.second).heard)
        assertEquals(Heard.Another, watch().hear(another.first, ravneet, another.second).heard)
    }

    @Test
    fun the_person_keys_recorded_in_the_cores_transcripts_give_the_marks_the_client_keeps() {
        val folder = File("../../../crates/navree-native-core/transcripts")
        org.junit.Assume.assumeTrue("the core's transcripts are beside the client", folder.isDirectory)
        fun keysIn(name: String): List<String> {
            val root = kotlinx.serialization.json.Json.parseToJsonElement(File(folder, name).readText())
            val steps = root.let { it as kotlinx.serialization.json.JsonObject }["steps"] as kotlinx.serialization.json.JsonArray
            return steps.mapNotNull { step ->
                val checks = (step as kotlinx.serialization.json.JsonObject)["check"] as? kotlinx.serialization.json.JsonArray ?: return@mapNotNull null
                checks.firstNotNullOfOrNull { item ->
                    val obj = item as kotlinx.serialization.json.JsonObject
                    if ((obj["at"] as? kotlinx.serialization.json.JsonPrimitive)?.content == "/app/user/key") {
                        (obj["is"] as? kotlinx.serialization.json.JsonPrimitive)?.content
                    } else {
                        null
                    }
                }
            }
        }
        val steady = keysIn("person-key-steady.json")
        assertEquals("six key checks in person-key-steady.json", 6, steady.size)
        val firstRun = watch()
        assertEquals(Heard.First, firstRun.hearAndKeep(steady[0], "Ravneet Kaur" to "ravneet.kaur@example.com"))
        assertEquals("renamed under the same id keeps one mark", Heard.Same, firstRun.hearAndKeep(steady[1], "Harpreet Singh" to "harpreet.singh@example.com"))
        val withoutId = firstRun.hear(steady[2], "Ravneet Kaur" to "ravneet.kaur@example.com")
        assertEquals("an answer without an id is not merged with an id-bearing account", Heard.Another, withoutId.heard)
        firstRun.changed(withoutId)
        assertEquals("and stays one mark across launches without an id", Heard.Same, watch().hearAndKeep(steady[3], "Ravneet Kaur" to "ravneet.kaur@example.com"))
        assertEquals(Heard.Same, watch().hearAndKeep(steady[4], "Ravneet Kaur" to "ravneet.kaur@example.com"))
        val withIdAgain = watch().hear(steady[5], "Ravneet Kaur" to "ravneet.kaur@example.com")
        assertEquals("moving from no id to an id starts the id's mark", Heard.Another, withIdAgain.heard)
        assertEquals(PersonWatch.markOfKey(steady[0]), withIdAgain.mark)

        file.delete()
        val other = keysIn("person-key-other-id.json")
        assertEquals("three distinct keys in person-key-other-id.json", 3, other.distinct().size)
        val between = watch()
        assertEquals(Heard.First, between.hearAndKeep(other[0], harpreet))
        val middle = between.hear(other[1], harpreet)
        assertEquals(Heard.Another, middle.heard)
        between.changed(middle)
        val secondAccount = between.hear(other[2], harpreet)
        assertEquals(Heard.Another, secondAccount.heard)
    }
}
