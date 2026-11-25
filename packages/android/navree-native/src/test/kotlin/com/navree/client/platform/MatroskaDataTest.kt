package com.navree.client.platform

import com.navree.client.design.FileRules.Video
import org.junit.After
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.ByteArrayOutputStream
import java.io.File
import java.nio.file.Files

class MatroskaDataTest {
    private val folder = Files.createTempDirectory("navree-matroska-test").toFile()

    @After
    fun clear() {
        folder.deleteRecursively()
    }

    private fun join(vararg parts: ByteArray): ByteArray = ByteArrayOutputStream().apply { parts.forEach(::write) }.toByteArray()

    /** An id as its bytes: one to four, as the id itself says. */
    private fun idBytes(value: Long): ByteArray {
        var length = 1
        while ((value ushr (8 * length)) != 0L) length++
        return ByteArray(length) { (value ushr (8 * (length - 1 - it))).toByte() }
    }

    /** A size in as few bytes as hold it. */
    private fun sizeBytes(value: Long): ByteArray {
        var length = 1
        while (value >= (1L shl (7 * length)) - 1L) length++
        val bytes = ByteArray(length) { (value ushr (8 * (length - 1 - it))).toByte() }
        bytes[0] = (bytes[0].toInt() or (0x80 ushr (length - 1))).toByte()
        return bytes
    }

    private fun element(id: Long, vararg parts: ByteArray): ByteArray {
        val body = join(*parts)
        return idBytes(id) + sizeBytes(body.size.toLong()) + body
    }

    /** An element that does not say its size: it holds what follows it. */
    private fun open(id: Long, vararg parts: ByteArray): ByteArray = idBytes(id) + byteArrayOf(0x01, -1, -1, -1, -1, -1, -1, -1) + join(*parts)

    private fun number(value: Int): ByteArray = byteArrayOf(value.toByte())

    private fun words(value: String): ByteArray = value.toByteArray(Charsets.UTF_8)

    private fun head(docType: String): ByteArray = element(
        EBML,
        element(0x4286, number(1)),
        element(0x42F7, number(1)),
        element(0x42F2, number(4)),
        element(0x42F3, number(8)),
        element(0x4282, words(docType)),
        element(0x4287, number(4)),
        element(0x4285, number(2)),
    )

    private fun info(vararg more: ByteArray): ByteArray =
        element(INFO, *more, element(0x2AD7B1, byteArrayOf(0x0F, 0x42, 0x40)), element(0x4D80, words("Charkha")), element(0x5741, words("Charkha")))

    private fun track(type: Int, codec: String, at: Int = 1): ByteArray =
        element(0xAE, element(0xD7, number(at)), element(0x73C5, number(at)), element(0x83, number(type)), element(0x86, words(codec)))

    private fun tracks(vararg entries: ByteArray): ByteArray = element(TRACKS, *entries)

    private val sound = words("SOUND-BYTES-AT-A-FIXED-OFFSET")

    /** What a cluster holds: when it starts, and one block of sound. */
    private fun blocks(): ByteArray = join(element(0xE7, number(0)), element(0xA3, byteArrayOf(0x81.toByte(), 0, 0, 0x80.toByte()), sound))

    private fun cluster(): ByteArray = element(CLUSTER, blocks())

    private fun segment(vararg parts: ByteArray): ByteArray = element(SEGMENT, *parts)

    private fun simpleTag(name: String, value: String, vararg inner: ByteArray): ByteArray =
        element(0x67C8, element(0x45A3, words(name)), element(0x4487, words(value)), *inner)

    private fun tag(vararg held: ByteArray): ByteArray = element(0x7373, element(0x63C0), *held)

    private fun tags(vararg held: ByteArray): ByteArray = element(TAGS, *held)

    private fun crc(first: Int): ByteArray = element(0xBF, byteArrayOf(first.toByte(), 0x22, 0x33, 0x44))

    private fun file(name: String, bytes: ByteArray): File = File(folder, name).apply { writeBytes(bytes) }

    private fun holds(bytes: ByteArray, part: ByteArray): Boolean =
        (0..bytes.size - part.size).any { at -> part.indices.all { bytes[at + it] == part[it] } }

    private fun text(bytes: ByteArray): String = String(bytes, Charsets.ISO_8859_1)

    @Test
    fun a_matroska_or_webm_file_that_holds_sound_alone_is_told_from_one_with_video() {
        val mka = file("Kirtan.mka", head("matroska") + segment(info(), tracks(track(2, "A_FLAC")), cluster()))
        val webm = file("Voice note.webm", head("webm") + segment(info(), tracks(track(2, "A_OPUS")), cluster()))
        val two = file("Two tracks.mka", head("matroska") + segment(info(), tracks(track(2, "A_OPUS", 1), track(2, "A_AAC", 2)), cluster()))
        for (alone in listOf(mka, webm, two)) {
            val found = MatroskaData.inspect(alone)
            assertTrue("${alone.name}: $found", found.soundOnly && found.clean)
        }
        assertEquals(2, MatroskaData.inspect(two).tracks)
        assertEquals(Video.Sound, VideoData.kindOf(mka, "audio/x-matroska"))
        assertEquals("whatever type it came under", Video.Sound, VideoData.kindOf(webm, "video/webm"))
        assertEquals(Video.Sound, VideoData.kindOf(webm, null))
        assertEquals(Video.Sound, VideoData.kindOf(two, "application/octet-stream"))

        val others = mapOf(
            "a video" to head("webm") + segment(info(), tracks(track(1, "V_VP9")), cluster()),
            "a video with its sound" to head("matroska") + segment(info(), tracks(track(1, "V_VP9", 1), track(2, "A_OPUS", 2)), cluster()),
            "sound and then a video" to head("matroska") + segment(info(), tracks(track(2, "A_OPUS", 1), track(1, "V_AV1", 2)), cluster()),
            "subtitles alone" to head("matroska") + segment(info(), tracks(track(0x11, "S_TEXT/UTF8")), cluster()),
            "a track of both" to head("matroska") + segment(info(), tracks(track(3, "A_OPUS")), cluster()),
            "no tracks" to head("matroska") + segment(info(), cluster()),
            "tracks with none in them" to head("matroska") + segment(info(), tracks(), cluster()),
            "a track called sound with a video's codec" to head("webm") + segment(info(), tracks(track(2, "V_VP8")), cluster()),
            "a track that does not say what it is" to
                head("webm") + segment(info(), tracks(element(0xAE, element(0xD7, number(1)), element(0x86, words("A_OPUS")))), cluster()),
            "a track that names no codec" to
                head("webm") + segment(info(), tracks(element(0xAE, element(0xD7, number(1)), element(0x83, number(2)))), cluster()),
            "a track that says sound and then video" to
                head("webm") + segment(info(), tracks(element(0xAE, element(0x83, number(2)), element(0x83, number(1)), element(0x86, words("A_OPUS")))), cluster()),
        )
        for ((what, bytes) in others) {
            val other = file("other.mkv", bytes)
            val found = MatroskaData.inspect(other)
            assertFalse("$what read as sound alone: $found", found.soundOnly)
            assertFalse("$what does read: $found", found.broken)
            assertEquals(what, Video.Other, VideoData.kindOf(other, "video/webm"))
            assertEquals("$what, named as sound", Video.Other, VideoData.kindOf(other, "audio/x-matroska"))
        }
    }

    @Test
    fun a_file_that_does_not_read_cleanly_is_never_sound_alone() {
        val audio = tracks(track(2, "A_OPUS"))
        val whole = head("matroska") + segment(info(), audio, cluster())
        val broken = mapOf(
            "bytes after the first four that are no EBML" to byteArrayOf(0x1A, 0x45, 0xDF.toByte(), 0xA3.toByte()) + ByteArray(512) { (it * 31 + 7).toByte() },
            "another kind of EBML file" to head("ebmlish") + segment(info(), audio, cluster()),
            "a file cut short" to whole.copyOf(whole.size - 5),
            "bytes after the segment" to whole + byteArrayOf(0),
            "a second segment" to whole + segment(info(), tracks(track(1, "V_VP9")), cluster()),
            "tracks of no known size" to head("matroska") + segment(info(), open(TRACKS, track(2, "A_OPUS"))),
            "something that is no part of a segment" to head("matroska") + segment(info(), audio, element(0x4DBB, ByteArray(4)), cluster()),
            "an id longer than four bytes" to head("matroska") + segment(info(), audio, byteArrayOf(0x08, 0x11, 0x22, 0x33, 0x44, 0x80.toByte())),
            "a track that runs past its tracks" to
                head("matroska") + segment(info(), idBytes(TRACKS) + sizeBytes(4) + byteArrayOf(0xAE.toByte(), 0x90.toByte(), 0, 0), cluster()),
            "no segment" to head("matroska"),
            "something between the header and the segment" to head("matroska") + info() + segment(info(), audio, cluster()),
            "ids said to be longer than are read" to
                element(EBML, element(0x42F2, number(5)), element(0x4282, words("matroska"))) + segment(info(), audio, cluster()),
        )
        for ((what, bytes) in broken) {
            val source = file("broken.mka", bytes)
            val found = MatroskaData.inspect(source)
            assertTrue("$what read cleanly: $found", found.broken)
            assertFalse(what, found.soundOnly)
            assertFalse(what, found.clean)
            assertEquals(what, Video.Other, VideoData.kindOf(source, "audio/x-matroska"))
            val out = File(folder, "broken-clean.mka")
            assertTrue(what, MatroskaData.clean(source, out).broken)
            assertFalse("$what was written out", out.exists())
        }
    }

    @Test
    fun a_tag_that_names_a_place_and_what_is_attached_are_overwritten_in_place_and_the_sound_does_not_move() {
        val title = simpleTag("TITLE", "Charkha Loom")
        val place = simpleTag("RECORDING_LOCATION", "Ludhiana +30.9010+075.8573/")
        val artist = simpleTag("ARTIST", "Gurpreet Kaur", simpleTag("LOCATION-eng", "Amritsar"))
        // Longer than a blank element whose size is written in one byte can be.
        val lengthy = simpleTag("com.apple.quicktime.location.ISO6709", "+31.6340+074.8723/" + "x".repeat(200))
        val picture = byteArrayOf(0xFF.toByte(), 0xD8.toByte(), 0xFF.toByte()) + words(" Exif GPSLatitude Patiala")
        val drawn = element(0x67C8, element(0x45A3, words("COVER")), element(0x4485, picture))
        val cover = element(0x61A7, element(0x466E, words("cover.jpg")), element(0x4660, words("image/jpeg")), element(0x465C, picture), element(0x46AE, number(7)))
        val notes = element(0x61A7, element(0x466E, words("Bathinda.txt")), element(0x4660, words("text/plain")), element(0x465C, words("recorded at Bathinda")))
        val last = cluster()
        val raw = head("matroska") + segment(info(), tracks(track(2, "A_OPUS")), tags(tag(title, place, artist, lengthy, drawn)), element(ATTACHMENTS, cover, notes), last)
        val source = file("Kirtan.mka", raw)
        val cleaned = File(folder, "Kirtan-clean.mka")

        val before = MatroskaData.inspect(source)
        assertTrue("$before", before.soundOnly)
        assertEquals("the place, the place inside the artist's tag, the long one and the picture", 4, before.placeTags)
        assertEquals(2, before.attachments)
        assertFalse(before.clean)
        assertEquals("sound alone, with a place still to take out", Video.Sound, VideoData.kindOf(source, "audio/x-matroska"))

        assertEquals(before, MatroskaData.clean(source, cleaned))
        val bytes = cleaned.readBytes()
        assertEquals("each part is overwritten by one of its own length", raw.size, bytes.size)
        assertArrayEquals("the sound is where it was", last, bytes.copyOfRange(bytes.size - last.size, bytes.size))
        val left = text(bytes)
        for (gone in listOf("Ludhiana", "30.9010", "Amritsar", "31.6340", "Patiala", "GPSLatitude", "cover.jpg", "Bathinda", "LOCATION")) {
            assertFalse("$gone is still in it", left.contains(gone))
        }
        assertTrue("a tag that names no place stays", left.contains("TITLE") && left.contains("Charkha Loom"))
        assertTrue("and so does the tag a place stood inside", left.contains("ARTIST") && left.contains("Gurpreet Kaur"))

        val after = MatroskaData.inspect(cleaned)
        assertTrue("what was written reads cleanly, as sound alone, with nothing left to take out: $after", after.soundOnly && after.clean)
        assertEquals(Video.Sound, VideoData.kindOf(cleaned, "audio/x-matroska"))
        assertFalse(cleaned.inputStream().use { PictureData.packetNamesPlace(it) })
    }

    @Test
    fun a_file_of_sound_that_names_no_place_is_copied_as_it_is() {
        val raw = head("webm") +
            segment(info(), tracks(track(2, "A_VORBIS")), tags(tag(simpleTag("TITLE", "Shabad"), simpleTag("ARTIST", "Harpreet Singh"))), element(0xEC, ByteArray(40)), cluster())
        val source = file("Shabad.webm", raw)
        val found = MatroskaData.inspect(source)
        assertTrue("$found", found.soundOnly && found.clean)
        val copy = File(folder, "Shabad-copy.webm")
        assertEquals(found, MatroskaData.clean(source, copy))
        assertArrayEquals(raw, copy.readBytes())
    }

    @Test
    fun the_checksum_of_a_part_that_had_something_overwritten_is_overwritten_too() {
        val changed = tag(crc(0x12), simpleTag("ARTIST", "Gurpreet Kaur", crc(0x13), simpleTag("LOCATION", "Moga")))
        val untouched = tag(crc(0x15), simpleTag("TITLE", "Charkha Loom"))
        val raw = head("matroska") + segment(crc(0x10), info(crc(0x14)), tracks(track(2, "A_OPUS")), tags(crc(0x11), changed, untouched), cluster())
        val source = file("Checked.mka", raw)
        val cleaned = File(folder, "Checked-clean.mka")
        assertEquals(1, MatroskaData.inspect(source).placeTags)
        MatroskaData.clean(source, cleaned)
        val bytes = cleaned.readBytes()
        assertEquals(raw.size, bytes.size)
        assertFalse(text(bytes).contains("Moga"))
        for (first in listOf(0x10, 0x11, 0x12, 0x13)) {
            assertFalse("the checksum that begins $first still stands over what was changed", holds(bytes, crc(first)))
        }
        for (first in listOf(0x14, 0x15)) {
            assertTrue("the checksum that begins $first stood over nothing that changed", holds(bytes, crc(first)))
        }
        val after = MatroskaData.inspect(cleaned)
        assertTrue("$after", after.soundOnly && after.clean)
    }

    @Test
    fun a_blank_element_that_still_holds_what_stood_there_is_emptied() {
        val stale = element(0xEC, words("RECORDING_LOCATION Ludhiana"))
        val between = element(0xEC, words("Amritsar"))
        val inTags = tags(element(0xEC, words("Patiala")), tag(simpleTag("TITLE", "Charkha Loom"), element(0xEC, words("Jalandhar"))))
        val raw = head("matroska") + between + segment(info(), tracks(track(2, "A_OPUS")), stale, element(0xEC, ByteArray(24)), inTags, cluster())
        val source = file("Blanked.mka", raw)
        val cleaned = File(folder, "Blanked-clean.mka")
        val before = MatroskaData.inspect(source)
        assertTrue("$before", before.soundOnly)
        assertEquals("the one that holds nothing is not counted", 4, before.filledVoids)
        assertFalse(before.clean)
        MatroskaData.clean(source, cleaned)
        val bytes = cleaned.readBytes()
        assertEquals(raw.size, bytes.size)
        for (gone in listOf("Ludhiana", "Amritsar", "Patiala", "Jalandhar")) {
            assertFalse("$gone is still in it", text(bytes).contains(gone))
        }
        assertTrue(text(bytes).contains("Charkha Loom"))
        val after = MatroskaData.inspect(cleaned)
        assertTrue("$after", after.soundOnly && after.clean)
    }

    @Test
    fun a_segment_and_clusters_of_no_known_size_are_read() {
        val audio = tracks(track(2, "A_OPUS"))
        // As a recording written while it is made ends: inside its last cluster.
        val recorded = file("Recorded.webm", head("webm") + open(SEGMENT, info(), audio, open(CLUSTER, blocks()), open(CLUSTER, blocks(), blocks())))
        val found = MatroskaData.inspect(recorded)
        assertTrue("$found", found.soundOnly && found.clean)
        assertEquals(Video.Sound, VideoData.kindOf(recorded, "video/webm"))

        // Tags after the clusters: each cluster ends where the next part of the segment begins.
        val raw = head("webm") + open(SEGMENT, info(), audio, open(CLUSTER, blocks()), open(CLUSTER, blocks()), tags(tag(simpleTag("LOCATION", "Jalandhar"))))
        val source = file("Tagged.webm", raw)
        val before = MatroskaData.inspect(source)
        assertTrue("$before", before.soundOnly)
        assertEquals(1, before.placeTags)
        val cleaned = File(folder, "Tagged-clean.webm")
        MatroskaData.clean(source, cleaned)
        val bytes = cleaned.readBytes()
        assertEquals(raw.size, bytes.size)
        assertFalse(text(bytes).contains("Jalandhar"))
        val start = raw.size - tags(tag(simpleTag("LOCATION", "Jalandhar"))).size
        assertArrayEquals("everything before the tags is as it was", raw.copyOf(start), bytes.copyOf(start))
        val after = MatroskaData.inspect(cleaned)
        assertTrue("$after", after.soundOnly && after.clean)
        assertEquals(Video.Sound, VideoData.kindOf(cleaned, "video/webm"))

        // A video track named after a cluster of no known size is still found.
        val late = file("Late.webm", head("webm") + open(SEGMENT, info(), audio, open(CLUSTER, blocks()), tracks(track(1, "V_VP8", 2))))
        assertFalse(MatroskaData.inspect(late).soundOnly)
        assertEquals(Video.Other, VideoData.kindOf(late, "audio/webm"))

        // A second file after such a cluster is no part of the first.
        val second = file("Second.webm", head("webm") + open(SEGMENT, info(), audio, open(CLUSTER, blocks()), head("webm"), segment(info(), tracks(track(1, "V_VP8")))))
        assertTrue(MatroskaData.inspect(second).broken)
        assertEquals(Video.Other, VideoData.kindOf(second, "audio/webm"))

        // Inside a cluster of no known size, a part that does not say its size does not read.
        val unsized = file("Unsized.webm", head("webm") + open(SEGMENT, info(), audio, open(CLUSTER, open(0xA0, blocks()))))
        assertTrue(MatroskaData.inspect(unsized).broken)
    }

    @Test
    fun a_part_of_any_length_is_overwritten_by_a_blank_one_of_the_same_length() {
        val last = cluster()
        // Across the length where the blank element's size no longer fits one byte.
        for (length in 90..150) {
            val raw = head("matroska") + segment(info(), tracks(track(2, "A_OPUS")), tags(tag(simpleTag("LOCATION", "L".repeat(length)))), last)
            val source = file("Length.mka", raw)
            val cleaned = File(folder, "Length-clean.mka")
            assertEquals(1, MatroskaData.clean(source, cleaned).placeTags)
            val bytes = cleaned.readBytes()
            assertEquals("a value of $length letters", raw.size, bytes.size)
            assertArrayEquals("a value of $length letters", last, bytes.copyOfRange(bytes.size - last.size, bytes.size))
            assertFalse("a value of $length letters", text(bytes).contains("LLLL"))
            val after = MatroskaData.inspect(cleaned)
            assertTrue("a value of $length letters: $after", after.soundOnly && after.clean)
        }
    }

    private companion object {
        const val EBML = 0x1A45DFA3L
        const val SEGMENT = 0x18538067L
        const val INFO = 0x1549A966L
        const val TRACKS = 0x1654AE6BL
        const val CLUSTER = 0x1F43B675L
        const val ATTACHMENTS = 0x1941A469L
        const val TAGS = 0x1254C367L
    }
}
