package com.navree.client.platform

import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.File
import java.nio.file.Files

/**
 * The secret of an install, which the core makes its key for a person with:
 * made once, in a folder of the test's own, and the same at every start.
 */
class InstallSecretTest {
    private val folder = Files.createTempDirectory("navree-install").toFile()
    private val file = File(folder, InstallSecret.FILE)

    @After
    fun clear() {
        folder.deleteRecursively()
    }

    @Test
    fun the_secret_of_an_install_is_made_once_and_is_the_same_at_every_start() {
        val made = InstallSecret.of(file)
        assertEquals("thirty-two bytes, two hex digits each", 2 * InstallSecret.BYTES, made.length)
        assertTrue("the secret made is not one the core takes", InstallSecret.reads(made))
        assertEquals("the next start made another secret", made, InstallSecret.of(file))
        assertEquals("what is written down is not the secret", made, file.readText().trim())
        assertFalse("the file it was written beside is left behind", File(folder, InstallSecret.FILE + ".new").exists())
        // Another install has another.
        assertNotEquals("two installs have one secret", made, InstallSecret.of(File(folder, "another-install")))
        // The mark of the person stands beside it, and a sign-out, which takes that mark away, leaves the secret.
        val watch = PersonWatch(File(folder, PersonWatch.MARK), "https://shop.example:443")
        watch.hear("k2:17d27bf8db683479c652471e31c96ac7d03e690c41c4f94f56ca99de886e389e", "Ravneet Kaur", null).also { if (it.write) watch.keep(it) }
        watch.signedOut()
        assertEquals("a sign-out took the install's secret away", made, InstallSecret.of(file))
    }

    @Test
    fun a_file_that_holds_no_secret_is_replaced_by_a_new_one_which_stays() {
        file.writeText("not a secret")
        val made = InstallSecret.of(file)
        assertTrue(InstallSecret.reads(made))
        assertEquals(made, InstallSecret.of(file))
        // Thirty-one bytes are too few.
        file.writeText("0a".repeat(31))
        val longer = InstallSecret.of(file)
        assertTrue(InstallSecret.reads(longer))
        assertNotEquals("0a".repeat(31), longer)
        // One that reads is kept as it is, in whichever case its digits are.
        file.writeText("0A".repeat(32) + "\n")
        assertEquals("0A".repeat(32), InstallSecret.of(file))
    }

    @Test
    fun a_secret_that_cannot_be_written_down_is_the_same_for_as_long_as_the_process_lives() {
        // Its folder is not there: nothing can be written into it.
        val nowhere = File(File(folder, "gone"), InstallSecret.FILE)
        val made = InstallSecret.of(nowhere)
        assertTrue(InstallSecret.reads(made))
        assertFalse(nowhere.exists())
        assertEquals("a second session of the process was given another secret", made, InstallSecret.of(nowhere))
    }

    @Test
    fun what_the_core_takes_for_a_secret_is_hex_of_thirty_two_bytes_or_more() {
        assertTrue(InstallSecret.reads("0a".repeat(32)))
        assertTrue(InstallSecret.reads("0a".repeat(48)))
        assertFalse("thirty-one bytes are taken for a secret", InstallSecret.reads("0a".repeat(31)))
        assertFalse("half a byte is taken for a secret", InstallSecret.reads("0a".repeat(32) + "0"))
        assertFalse(InstallSecret.reads("zz" + "0a".repeat(31)))
        assertFalse(InstallSecret.reads(""))
    }
}
