package com.navree.client.platform

import java.io.File
import java.security.MessageDigest

/** What hearing who the core names came to. */
internal enum class Heard {
    /** The first person named since the app was first started here, or since somebody last signed out. */
    First,

    /** The person who was named before. */
    Same,

    /** Another person than the one named before, with nobody signed out between. */
    Another,
}

/**
 * Who the core last said is signed in, so that a change of person nobody
 * signed out for is seen. A session can run out, or be ended by a server that
 * says nothing of it to the client, and somebody else can then sign in: the
 * core empties what it keeps and says no more than that. What the client keeps
 * of its own for a person is the client's to take away then, and this is how
 * it knows when.
 *
 * Two people are told apart by the key the core gives the person it names:
 * the same for one person and another for two accounts, whatever their names
 * (the core makes it from the id the app's server gave the person, where it
 * gave one). A core that gives no key leaves the client what the app's part
 * of the view state says of a person, the name and email, and two people are
 * then told apart only as far as those tell them. Either comes from the
 * shell the app's server answered; nothing a page says is heard. While the
 * core names nobody, nothing is heard: the person named before may sign in
 * again and find everything as it was. A sign-out forgets who was named
 * ([signedOut]), since it takes everything away itself, so whoever is named
 * after one is the first.
 *
 * What is remembered is a mark, made from the key ([markOfKey]) or, without
 * one, from the name and email ([markOf]): it tells two people apart and
 * nothing of either is read from it. It is kept in a file no backup takes,
 * beside the origin it was heard at, and read when the client starts, so a
 * start sees a change of person that happened while the app was closed. A
 * mark kept for another origin than this run's is not compared with: what a
 * device keeps for a person it keeps at an origin, and the pages and requests
 * of another origin are given none of it.
 *
 * A mark made from a name and email, by a run whose core gave no key, is
 * still the mark on the disk when a core that gives one first names
 * somebody. That person is the one remembered when their name and email make
 * the mark that is kept: the mark of their key is then written in its place
 * and nothing is taken away, so a new core by itself changes nothing for the
 * person who is signed in.
 *
 * The mark on the disk is whom what is kept on the device is kept for. When
 * another person is named it stays the mark of the person before until every
 * step that takes their things away has finished ([changed]); an app that
 * ends in the middle, or a step that fails, leaves it, and the next start
 * hears another person than the one it remembers and does all of it over.
 */
internal class PersonWatch(private val file: File, private val origin: String) {
    private val lock = Any()

    /** The mark of the person named last: in this run, or, read from the disk, in the one before. */
    private var named: String?

    /** The mark the disk holds for this origin. */
    private var kept: String?

    /** The mark that is on its way to the disk. */
    private var writing: String? = null

    /** A change of person has not finished: the disk holds the mark of the person before, and goes on holding it. */
    private var owed = false

    /** Grows with every change of person and every sign-out: what was begun before one says nothing after it. */
    private var turn = 0L

    init {
        val lines = runCatching { file.readLines() }.getOrDefault(emptyList())
        val read = lines.getOrNull(1)?.trim()?.takeIf { it.isNotEmpty() && lines.first().trim() == origin }
        named = read
        kept = read
    }

    /** What was heard: who it was to the one before, the mark of the person named, and whether that mark is to be written down ([keep]). */
    class Word(val heard: Heard, val mark: String, internal val turn: Long, val write: Boolean)

    /**
     * Hears who the core names as signed in: by [key] where the core gives
     * one, by [name] and [email] where it gives none. Another person than the
     * one before, with no sign-out between, is a change of person: from now
     * the mark on the disk is not replaced until [changed]. For the first
     * person and the same one, the word says whether their mark is still to
     * be written down, once for each mark.
     *
     * Where the mark remembered was made from a name and email and this is
     * the first key, the person is the same when their name and email make
     * that mark, and the mark of the key is the one written down from now.
     * Where the mark remembered was made from a key and the core now gives
     * none, the two cannot be compared, and the person is taken for another.
     */
    fun hear(key: String?, name: String, email: String?): Word = synchronized(lock) {
        val given = key?.takeIf { it.isNotEmpty() }
        val now = if (given != null) markOfKey(given) else markOf(name, email)
        val before = named
        named = now
        val heard = when {
            before == null -> Heard.First
            before == now -> Heard.Same
            given != null && !before.startsWith(OF_KEY) && before == markOf(name, email) -> Heard.Same
            else -> Heard.Another
        }
        if (heard == Heard.Another) {
            owed = true
            turn++
        }
        val write = !owed && now != kept && now != writing
        if (write) writing = now
        Word(heard, now, turn, write)
    }

    /**
     * Writes down the mark of the person who is signed in: what is kept from
     * now on is kept for them. Nothing is written when somebody signed out or
     * another person was named since the word was heard, nor while a change
     * of person has not finished. Not on the main thread.
     */
    fun keep(word: Word) {
        synchronized(lock) {
            if (writing == word.mark) writing = null
            if (owed || word.turn != turn || word.mark == kept) return
            put(word.mark)
        }
    }

    /**
     * Every step that takes away what was kept for the person before has
     * finished: the mark of the person who is here now takes the place of
     * theirs. Nothing is written when somebody signed out or yet another
     * person was named since: then the mark is the later change's to write,
     * or nobody's. Not on the main thread.
     */
    fun changed(word: Word) {
        synchronized(lock) {
            if (word.turn != turn) return
            owed = false
            put(word.mark)
        }
    }

    /** Somebody signed out: the sign-out takes everything of theirs away, the mark with it, and whoever is named next is the first. */
    fun signedOut() {
        synchronized(lock) {
            named = null
            kept = null
            writing = null
            owed = false
            turn++
            runCatching { file.delete() }
        }
    }

    /** A mark that could not be written is not the one kept: the next time the person is named it is written again. */
    private fun put(mark: String) {
        if (runCatching { WipeRules.put(file, origin + "\n" + mark + "\n") }.isSuccess) kept = mark
    }

    companion object {
        /** The file the mark is kept in, in the folder no backup takes. */
        const val MARK = "navree-person"

        /**
         * The mark of a person: the same for the same name and email, another
         * for any other, and neither is read from it. It is the SHA-256, in
         * hex, of the name's length, a colon, the name, and then `-` for a
         * person with no email or `+` and the email, as every Navree client
         * writes it: so a person with no email and one whose email is empty
         * are two, and no name can be taken for a name and an email.
         */
        fun markOf(name: String, email: String?): String = sha(name.length.toString() + ":" + name + if (email == null) "-" else "+$email")

        /** What a mark made from a key begins with: no mark made from a name and email does, being hex digits only. */
        private const val OF_KEY = "key:"

        /**
         * The mark of the person the core names by [key]: the same for the
         * same key and another for any other, and the key is not read from
         * it. It is told from a mark made of a name and email by how it
         * begins.
         */
        fun markOfKey(key: String): String = OF_KEY + sha(key)

        private fun sha(written: String): String =
            MessageDigest.getInstance("SHA-256").digest(written.encodeToByteArray()).joinToString("") { "%02X".format(it) }
    }
}
