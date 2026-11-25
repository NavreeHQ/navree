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
 * made from the id the app's server gave the person where it gave one (kept
 * across changes of name and email, and distinct for two accounts of one name),
 * or from the name and email where no id was given (with answers that carry an
 * id and answers that carry none never sharing a key). A caller that gives no
 * key at all leaves the client the name and email, and two people are then told
 * apart only as far as those tell them. Either comes from the shell the app's
 * server answered; nothing a page says is heard. While the core names nobody,
 * nothing is heard: the person named before may sign in again and find
 * everything as it was. A sign-out forgets who was named ([signedOut]), since
 * it takes everything away itself, so whoever is named after one is the first.
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
 * Marks of the two kinds never match. A mark made from a name and email, by
 * a run whose core gave no key, says too little to be taken for the person a
 * key names: when a core that gives a key first names somebody where such a
 * mark is kept, that is another person, whatever their name and email, and
 * what was kept goes. So does it where a key's mark is kept and the core
 * names somebody with no key. Name and email are compared only where neither
 * side has a key.
 *
 * One mark is held to another, once. The core makes a person's key with a
 * secret of this install ([InstallSecret]), and such a key begins `k2:`; its
 * mark begins so too. A mark kept from before the install had a secret is the
 * mark of a plain key, which the core makes no more. While such a mark is
 * kept, the client says so to the core ([keepsPlainKey], `old_key` in the
 * core's configuration), and the core says beside the person it names what
 * their plain key was (`app.user_was`). Where the mark kept is the mark of
 * that key, the person named is the one what is kept is kept for: nothing is
 * taken away, and the mark of the key they have now is written in its place.
 * Any other mark of a plain key is another person's, as before.
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
     * Whether the mark the disk holds is the mark of a plain key: one the
     * core made before this install had a secret. Asked at the start, before
     * the core is made, it says whether the core is to say what a person's
     * plain key was (`old_key`).
     */
    val keepsPlainKey: Boolean get() = synchronized(lock) { kept?.startsWith(OF_KEY) == true }

    /**
     * Hears who the core names as signed in: by [key] where the core gives
     * one, by [name] and [email] where it gives none. Another person than the
     * one before, with no sign-out between, is a change of person: from now
     * the mark on the disk is not replaced until [changed]. For the first
     * person and the same one, the word says whether their mark is still to
     * be written down, once for each mark.
     *
     * A mark made from a key and one made from a name and email never match
     * ([markOfKey]): where one kind is remembered and the core names
     * somebody by the other, that is another person, also when the name and
     * email are the same.
     *
     * [was] is what the core says the person's plain key was
     * (`app.user_was`), where it says. Where the mark remembered is the mark
     * of that key, and the key given now is made with the install's secret,
     * the person is the same one, and the word says to write the mark of the
     * key they have now. It is compared and never kept.
     */
    fun hear(key: String?, name: String, email: String?, was: String? = null): Word = synchronized(lock) {
        val given = key?.takeIf { it.isNotEmpty() }
        val now = if (given != null) markOfKey(given) else markOf(name, email)
        val before = named
        named = now
        val theirs = before != null && before.startsWith(OF_KEY) && now.startsWith(OF_KEYED) &&
            was?.takeIf { it.isNotEmpty() }?.let { before == markOfKey(it) } == true
        val heard = when {
            before == null -> Heard.First
            before == now || theirs -> Heard.Same
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

        /** What a mark made from a plain key begins with: no mark made from a name and email does, being hex digits only. */
        private const val OF_KEY = "key:"

        /**
         * What a key the core made with the install's secret begins with, and
         * what the mark made from such a key begins with: so the mark of a
         * plain key, which a run before the install had a secret kept, is
         * told from it by how it begins.
         */
        private const val OF_KEYED = "k2:"

        /**
         * The mark of the person the core names by [key]: the same for the
         * same key and another for any other, and the key is not read from
         * it. It is told from a mark made of a name and email by how it
         * begins, and the mark of a key made with the install's secret from
         * the mark of a plain one.
         */
        fun markOfKey(key: String): String = (if (key.startsWith(OF_KEYED)) OF_KEYED else OF_KEY) + sha(key)

        private fun sha(written: String): String =
            MessageDigest.getInstance("SHA-256").digest(written.encodeToByteArray()).joinToString("") { "%02X".format(it) }
    }
}
