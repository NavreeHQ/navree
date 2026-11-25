package com.navree.client.platform

import java.io.File
import java.security.SecureRandom

/**
 * The secret of this install of the app, which the runtime core makes its key
 * for a person with (`secret` in the core's configuration): so that nobody who
 * does not hold it can make a person's key from a guess of who they are, and
 * the person has another key in every install.
 *
 * It is 32 bytes from the system's secure random generator, written as hex.
 * It is made once, the first time the app starts, and kept in a file of the
 * app's own in the folder no backup takes, beside the mark of the person
 * ([PersonWatch]). A sign-out leaves it: one person has one key at every
 * start of this install, also after somebody signed out in between.
 *
 * A file that is not there, or does not hold a secret, is replaced by a new
 * secret. The keys made with the new one are other keys: the person named
 * next is then another person than the one whose mark is kept, and what was
 * kept goes, as it does for anybody else. A secret that cannot be written
 * down is used for as long as the app's process lives, and the same follows
 * at the next start.
 *
 * It is a secret: it is given to the core and to nothing else, and is never
 * written to a log.
 */
internal object InstallSecret {
    /** The file the secret is kept in, in the folder no backup takes. */
    const val FILE = "navree-install"

    /** How many bytes the secret has. */
    const val BYTES = 32

    private val lock = Any()

    /** The secrets that could not be written down, by their file: the same one for every session of this process. */
    private val unwritten = HashMap<String, String>()

    /**
     * The secret kept in [file], as hex; made and written down when there is
     * none that reads. Not a cheap call, since it reads the disk: once, as
     * the session is made.
     */
    fun of(file: File, random: SecureRandom = SecureRandom()): String = synchronized(lock) {
        runCatching { file.readText().trim() }.getOrNull()?.takeIf(::reads)?.let { return it }
        unwritten[file.path]?.let { return it }
        val bytes = ByteArray(BYTES)
        random.nextBytes(bytes)
        val made = bytes.joinToString("") { "%02x".format(it) }
        bytes.fill(0)
        try {
            WipeRules.put(file, made + "\n")
        } catch (_: Exception) {
            // The secret of this process alone: the next start makes another, and takes the person it then hears for another.
            unwritten[file.path] = made
        }
        made
    }

    /** Whether [text] is a secret the core takes: hex, two characters a byte, of at least [BYTES] bytes. */
    fun reads(text: String): Boolean =
        text.length >= 2 * BYTES && text.length % 2 == 0 && text.all { it in '0'..'9' || it in 'a'..'f' || it in 'A'..'F' }
}
