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

    @Test
    fun a_blank_element_inside_any_part_that_is_read_is_emptied_with_every_checksum_over_it() {
        val header = element(EBML, element(0x42F2, number(4)), element(0x42F3, number(8)), element(0x4282, words("matroska")), stale("Ropar"))
        val seeks = element(SEEK_HEAD, crc(0x21), element(0x4DBB, element(0x53AB, idBytes(INFO)), element(0x53AC, number(40)), stale("Ludhiana")))
        val about = info(crc(0x22), stale("Amritsar"))
        val heard = element(0xE1, crc(0x25), element(0x9F, number(2)), stale("Jalandhar"))
        val audio = tracks(crc(0x23), element(0xAE, crc(0x24), element(0xD7, number(1)), element(0x83, number(2)), element(0x86, words("A_OPUS")), stale("Patiala"), heard))
        val cues = element(CUES, crc(0x26), element(0xBB, element(0xB3, number(0)), element(0xB7, element(0xF7, number(1)), element(0xF1, number(90)), stale("Bathinda"))))
        val shown = element(0x80, crc(0x28), element(0x85, words("Alaap")), element(0x437C, words("pan")), stale("Moga"))
        val chapters = element(CHAPTERS, crc(0x27), element(0x45B9, element(0xB6, element(0x73C4, number(1)), element(0x91, number(0)), shown)))
        // A part nothing is overwritten in: its blank element holds nothing.
        val untouched = element(SEEK_HEAD, crc(0x29), element(0x4DBB, element(0x53AB, idBytes(TAGS)), element(0x53AC, number(90))), element(0xEC, ByteArray(12)))
        val last = cluster()
        val raw = header + segment(crc(0x20), seeks, about, audio, cues, chapters, untouched, last)
        val source = file("Parts.mka", raw)
        val cleaned = File(folder, "Parts-clean.mka")

        val before = MatroskaData.inspect(source)
        assertTrue("$before", before.soundOnly)
        assertEquals("one in the header, a seek entry, the information, a track, its audio, a cue point and a chapter's display", 7, before.filledVoids)
        assertFalse(before.clean)
        assertEquals(Video.Sound, VideoData.kindOf(source, "audio/x-matroska"))

        assertEquals(before, MatroskaData.clean(source, cleaned))
        val bytes = cleaned.readBytes()
        assertEquals(raw.size, bytes.size)
        assertArrayEquals("the sound is where it was", last, bytes.copyOfRange(bytes.size - last.size, bytes.size))
        for (gone in listOf("Ropar", "Ludhiana", "Amritsar", "Patiala", "Jalandhar", "Bathinda", "Moga")) {
            assertFalse("$gone is still in it", text(bytes).contains(gone))
        }
        assertTrue("a chapter's name that names no place stays", text(bytes).contains("Alaap"))
        for (first in 0x20..0x28) {
            assertFalse("the checksum that begins $first still stands over what was changed", holds(bytes, crc(first)))
        }
        assertTrue("the checksum of a part nothing changed in stays", holds(bytes, crc(0x29)))
        val after = MatroskaData.inspect(cleaned)
        assertTrue("$after", after.soundOnly && after.clean)
    }

    @Test
    fun a_blank_element_met_after_a_cluster_of_no_known_size_is_emptied() {
        val audio = tracks(track(2, "A_OPUS"))
        val kept = tags(tag(simpleTag("TITLE", "Shabad")))
        // The blank part after the first cluster may be the segment's: nothing tells it from one of the cluster's own.
        val first = open(CLUSTER, crc(0x31), blocks(), stale("RECORDING_LOCATION Ludhiana"))
        val second = open(CLUSTER, crc(0x32), blocks(), element(0xEC, ByteArray(16)))
        val raw = head("webm") + segment(crc(0x30), info(), audio, first, kept, second)
        val source = file("After.webm", raw)
        val cleaned = File(folder, "After-clean.webm")
        val before = MatroskaData.inspect(source)
        assertTrue("$before", before.soundOnly)
        assertEquals(1, before.filledVoids)
        assertFalse(before.clean)
        MatroskaData.clean(source, cleaned)
        val bytes = cleaned.readBytes()
        assertEquals(raw.size, bytes.size)
        assertFalse(text(bytes).contains("Ludhiana"))
        assertFalse(text(bytes).contains("LOCATION"))
        assertTrue(text(bytes).contains("Shabad"))
        assertFalse("the segment's checksum stood over it", holds(bytes, crc(0x30)))
        assertFalse("and so did the cluster's", holds(bytes, crc(0x31)))
        assertTrue("the cluster after it was not changed", holds(bytes, crc(0x32)))
        val after = MatroskaData.inspect(cleaned)
        assertTrue("$after", after.soundOnly && after.clean)

        // As a recording written while it is made ends, with a blank part last of all.
        val end = stale("Amritsar")
        val live = head("webm") + open(SEGMENT, info(), audio, open(CLUSTER, blocks()), open(CLUSTER, blocks(), end))
        val recorded = file("Live.webm", live)
        val emptied = File(folder, "Live-clean.webm")
        assertEquals(1, MatroskaData.inspect(recorded).filledVoids)
        MatroskaData.clean(recorded, emptied)
        val written = emptied.readBytes()
        assertEquals(live.size, written.size)
        assertFalse(text(written).contains("Amritsar"))
        assertArrayEquals("everything before it is as it was", live.copyOf(live.size - end.size), written.copyOf(live.size - end.size))
        val then = MatroskaData.inspect(emptied)
        assertTrue("$then", then.soundOnly && then.clean)
    }

    @Test
    fun a_title_a_tracks_name_and_a_chapters_name_that_name_a_place_are_overwritten_as_a_tag_is() {
        val about = info(crc(0x40), element(0x7BA9, words("Kirtan at +30.9010+075.8573/")))
        val named = element(0xAE, element(0xD7, number(1)), element(0x83, number(2)), element(0x86, words("A_OPUS")), element(0x536E, words("Location: Ludhiana")))
        val plain = element(0xAE, element(0xD7, number(2)), element(0x83, number(2)), element(0x86, words("A_OPUS")), element(0x536E, words("Tabla")))
        val placed = element(0xB6, element(0x73C4, number(1)), element(0x91, number(0)), element(0x80, element(0x85, words("GPS Amritsar")), element(0x437C, words("pan"))))
        val unplaced = element(0xB6, element(0x73C4, number(2)), element(0x91, number(9)), element(0x80, crc(0x42), element(0x85, words("Alaap")), element(0x437C, words("pan"))))
        val chapters = element(CHAPTERS, element(0x45B9, crc(0x41), placed, unplaced))
        val last = cluster()
        val raw = head("matroska") + segment(about, tracks(crc(0x43), named, plain), chapters, last)
        val source = file("Named.mka", raw)
        val cleaned = File(folder, "Named-clean.mka")

        val before = MatroskaData.inspect(source)
        assertTrue("$before", before.soundOnly)
        assertEquals("the title, the first track's name and the first chapter's", 3, before.namedPlaces)
        assertEquals(0, before.placeTags)
        assertFalse(before.clean)

        assertEquals(before, MatroskaData.clean(source, cleaned))
        val bytes = cleaned.readBytes()
        assertEquals(raw.size, bytes.size)
        assertArrayEquals("the sound is where it was", last, bytes.copyOfRange(bytes.size - last.size, bytes.size))
        for (gone in listOf("30.9010", "Kirtan", "Ludhiana", "Location", "Amritsar", "GPS")) {
            assertFalse("$gone is still in it", text(bytes).contains(gone))
        }
        for (stays in listOf("Tabla", "Alaap", "Charkha")) {
            assertTrue("$stays names no place", text(bytes).contains(stays))
        }
        for (first in listOf(0x40, 0x41, 0x43)) {
            assertFalse("the checksum that begins $first still stands over what was changed", holds(bytes, crc(first)))
        }
        assertTrue("the checksum of the chapter's display nothing changed in stays", holds(bytes, crc(0x42)))
        val after = MatroskaData.inspect(cleaned)
        assertTrue("$after", after.soundOnly && after.clean)
        assertEquals(2, after.tracks)

        // A title too long to read goes as one that names a place does.
        val lengthy = head("matroska") + segment(info(element(0x7BA9, words("x".repeat(5000)))), tracks(track(2, "A_OPUS")), last)
        val titled = file("Titled.mka", lengthy)
        val untitled = File(folder, "Titled-clean.mka")
        assertEquals(1, MatroskaData.clean(titled, untitled).namedPlaces)
        assertEquals(lengthy.size, untitled.readBytes().size)
        assertFalse(text(untitled.readBytes()).contains("xxxx"))
        val read = MatroskaData.inspect(untitled)
        assertTrue("$read", read.soundOnly && read.clean)
    }

    @Test
    fun words_that_hold_coordinates_name_a_place_whatever_the_tag_is_called() {
        val places = listOf(
            "+30.9010+075.8573/",
            "made at +30.9010+075.8573+247.000/ in the evening",
            "+08.5241+076.9366",
            "+3054.06+07551.44/",
            "+305403.6+0755126.3/",
            "+30+075/",
            "+3054+07551+247/",
        )
        for (place in places) {
            assertTrue(place, MatroskaData.holdsCoordinates(place))
            assertTrue(place, MatroskaData.namesPlace(place))
        }
        for (none in listOf("", "Charkha Loom", "2026-11-25", "2026-11-25T18:30:00+05:30", "+91 98765 43210", "-7.03 dB", "Sector 17-22", "0.92+0.05", "+12.5")) {
            assertFalse(none, MatroskaData.holdsCoordinates(none))
        }
        assertTrue("a word a tag's name is known by", MatroskaData.namesPlace("Location: Ludhiana"))
        assertFalse(MatroskaData.namesPlace("Charkha Loom"))

        val comment = simpleTag("COMMENT", "Recorded at +30.9010+075.8573/ on the terrace")
        val dated = simpleTag("DATE_RECORDED", "2026-11-25")
        val gain = simpleTag("REPLAYGAIN_TRACK_GAIN", "-7.03 dB")
        val raw = head("matroska") + segment(info(), tracks(track(2, "A_OPUS")), tags(tag(comment, dated, gain)), cluster())
        val source = file("Comment.mka", raw)
        val cleaned = File(folder, "Comment-clean.mka")
        val before = MatroskaData.inspect(source)
        assertTrue("$before", before.soundOnly)
        assertEquals(1, before.placeTags)
        MatroskaData.clean(source, cleaned)
        val bytes = cleaned.readBytes()
        assertEquals(raw.size, bytes.size)
        for (gone in listOf("30.9010", "terrace", "COMMENT")) {
            assertFalse("$gone is still in it", text(bytes).contains(gone))
        }
        assertTrue(text(bytes).contains("2026-11-25") && text(bytes).contains("-7.03 dB"))
        val after = MatroskaData.inspect(cleaned)
        assertTrue("$after", after.soundOnly && after.clean)
    }

    @Test
    fun a_part_that_does_not_read_inside_any_part_that_is_read_breaks_the_file() {
        val audio = tracks(track(2, "A_OPUS"))
        val one = arrayOf(element(0x83, number(2)), element(0x86, words("A_OPUS")))
        // A part that says it holds sixteen bytes where its parent has two left.
        val past = byteArrayOf(0xEC.toByte(), 0x90.toByte(), 0, 0)
        var atom = element(0xB6, element(0x73C4, number(1)))
        repeat(40) { atom = element(0xB6, atom) }
        val broken = mapOf(
            "in a seek entry" to segment(element(SEEK_HEAD, element(0x4DBB, past)), info(), audio, cluster()),
            "in the information" to segment(element(INFO, element(0x2AD7B1, byteArrayOf(0x0F, 0x42, 0x40)), past), audio, cluster()),
            "in a track" to segment(info(), tracks(element(0xAE, *one, past)), cluster()),
            "in a track's audio" to segment(info(), tracks(element(0xAE, *one, element(0xE1, past))), cluster()),
            "in a cue point" to segment(info(), audio, element(CUES, element(0xBB, element(0xB7, past))), cluster()),
            "in a chapter's display" to segment(info(), audio, element(CHAPTERS, element(0x45B9, element(0xB6, element(0x80, past)))), cluster()),
            "in what a tag is for" to segment(info(), audio, tags(element(0x7373, element(0x63C0, past))), cluster()),
            "a title of no known size" to segment(element(INFO, open(0x7BA9, words("Kirtan"))), audio, cluster()),
            "chapters inside chapters, deeper than is read" to segment(info(), audio, element(CHAPTERS, element(0x45B9, atom)), cluster()),
        )
        for ((what, parts) in broken) {
            val source = file("broken-part.mka", head("matroska") + parts)
            val found = MatroskaData.inspect(source)
            assertTrue("$what read cleanly: $found", found.broken)
            assertFalse(what, found.soundOnly)
            assertEquals(what, Video.Other, VideoData.kindOf(source, "audio/x-matroska"))
            val out = File(folder, "broken-part-clean.mka")
            assertTrue(what, MatroskaData.clean(source, out).broken)
            assertFalse("$what was written out", out.exists())
        }
    }

    /** A blank element that still holds the words that stood there. */
    private fun stale(value: String): ByteArray = element(0xEC, words(value))

    private companion object {
        const val EBML = 0x1A45DFA3L
        const val SEGMENT = 0x18538067L
        const val SEEK_HEAD = 0x114D9B74L
        const val INFO = 0x1549A966L
        const val TRACKS = 0x1654AE6BL
        const val CLUSTER = 0x1F43B675L
        const val CUES = 0x1C53BB6BL
        const val ATTACHMENTS = 0x1941A469L
        const val CHAPTERS = 0x1043A770L
        const val TAGS = 0x1254C367L
    }
}
