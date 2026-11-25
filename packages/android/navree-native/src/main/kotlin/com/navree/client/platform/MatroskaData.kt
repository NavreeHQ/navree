package com.navree.client.platform

import java.io.File
import java.io.OutputStream
import java.io.RandomAccessFile

/**
 * What a Matroska or WebM file holds, read from its own bytes as EBML: whether
 * it is sound alone, and the parts of such a file that can say where it was
 * made. Plain Kotlin over files and bytes, so every rule is tested without a
 * device.
 *
 * A file is sound alone when it reads cleanly from its first byte to its
 * last, has at least one track, and every track is audio (`TrackType` 2 with a
 * `CodecID` that begins `A_`). A file that does not read cleanly is
 * [Found.broken], and is never sound alone.
 *
 * It reads cleanly when all of this holds:
 * - Its EBML header names `matroska` or `webm`, with ids of at most four
 *   bytes and sizes of at most eight.
 * - One `Segment` follows the header, with nothing between them but `Void`,
 *   and runs to the end of the file.
 * - The segment holds nothing but the parts a segment has: `SeekHead`,
 *   `Info`, `Tracks`, `Cluster`, `Cues`, `Attachments`, `Chapters`, `Tags`,
 *   `Void` and `CRC-32`.
 * - Every id and size reads and stays inside its parent: in the EBML header,
 *   in the segment, in `SeekHead`, `Info`, `Tracks`, `Cues`, `Chapters`,
 *   `Attachments` and `Tags`, and in every part inside those that holds
 *   other parts, down to the parts that hold a value: a tag and its simple
 *   tags, and the parts [HOLDERS] lists by the Matroska specification (a
 *   seek entry, a track and its audio, a cue point, an edition, a chapter
 *   and its display, a tag's targets). A part that is overwritten whole is
 *   not read further in. No part stands more than 32 deep inside one of
 *   those.
 * - Only a `Segment` and a `Cluster` say no size. A `Segment` of no known
 *   size runs to the end of the file, and a `Cluster` of no known size to the
 *   next part of the segment, as a recording written while it is made has
 *   them; anything else of no known size does not read.
 *
 * In a file that is sound alone, [clean] overwrites in place, as a `Void`
 * element of the very same byte length, so no offset in the file moves:
 * - every `SimpleTag` whose `TagName` names a place
 *   ([VideoData.isLocationKeyName]: `RECORDING_LOCATION`, `LOCATION-eng`, a
 *   latitude, a city), whose `TagString` holds coordinates in its first 4096
 *   bytes ([holdsCoordinates]), or whose `TagBinary` begins as a picture or
 *   as Exif data does, with the tags inside it;
 * - the segment's `Title`, a track's `Name` and a chapter's `ChapString`
 *   whose words name a place ([namesPlace]): they hold one of the words a
 *   tag's name is known by (`Location: Ludhiana`, and so `City Lights` too),
 *   or coordinates. One longer than 4096 bytes is not read and goes as well;
 * - every `AttachedFile`, since a cover picture can hold the place it was
 *   taken and sound needs nothing that is attached;
 * - every `Void` that still holds bytes of what stood there before, wherever
 *   parts are read: in the EBML header, before the segment, in the segment,
 *   in each part of it that holds others, and in a `Cluster` of no known
 *   size, where a `Void` may be the segment's as well as the cluster's;
 * - the `CRC-32` of each part that had something in it overwritten, up to
 *   the segment's, since it no longer holds.
 *
 * What is not read: the sound itself; anything inside a `Cluster` that says
 * its size, a `Void` there among it; what an `AttachedFile` holds, which goes
 * whole; and the value of a part that holds a value, but for those named
 * above. So these stay as they are: a place written in plain words (a town,
 * a street) in a title, a name or a tag that names none; the names of files
 * in `Info`; a language; and a codec's private bytes, where a Vorbis or a
 * FLAC stream keeps comments of its own. A part this client does not know to
 * hold others is taken to hold a value.
 */
internal object MatroskaData {
    private const val EBML = 0x1A45DFA3L
    private const val DOC_TYPE = 0x4282L
    private const val MAX_ID_LENGTH = 0x42F2L
    private const val MAX_SIZE_LENGTH = 0x42F3L
    private const val SEGMENT = 0x18538067L
    private const val SEEK_HEAD = 0x114D9B74L
    private const val INFO = 0x1549A966L
    private const val TITLE = 0x7BA9L
    private const val TRACKS = 0x1654AE6BL
    private const val TRACK_ENTRY = 0xAEL
    private const val TRACK_TYPE = 0x83L
    private const val TRACK_NAME = 0x536EL
    private const val CODEC_ID = 0x86L
    private const val CLUSTER = 0x1F43B675L
    private const val CUES = 0x1C53BB6BL
    private const val ATTACHMENTS = 0x1941A469L
    private const val ATTACHED_FILE = 0x61A7L
    private const val CHAPTERS = 0x1043A770L
    private const val CHAPTER_DISPLAY = 0x80L
    private const val CHAP_STRING = 0x85L
    private const val TAGS = 0x1254C367L
    private const val TAG = 0x7373L
    private const val SIMPLE_TAG = 0x67C8L
    private const val TAG_NAME = 0x45A3L
    private const val TAG_STRING = 0x4487L
    private const val TAG_BINARY = 0x4485L
    private const val VOID = 0xECL
    private const val CRC_32 = 0xBFL

    /** The `TrackType` of a track of sound. */
    private const val AUDIO = 2L

    /** The ids a cluster of no known size ends at: those of the file's own two parts and of the parts of a segment. */
    private val ENDS_CLUSTER = setOf(EBML, SEGMENT, SEEK_HEAD, INFO, TRACKS, CLUSTER, CUES, ATTACHMENTS, CHAPTERS, TAGS)

    /**
     * The parts that hold other parts, inside the EBML header, `SeekHead`,
     * `Info`, `Tracks`, `Cues`, `Chapters` and a `Tag`, by the ids the
     * Matroska specification gives them: what each holds is read as parts in
     * turn. Any other part there holds a value (a number, words, bytes),
     * which is not read as parts.
     */
    private val HOLDERS: Set<Long> = setOf(
        0x4281L, // DocTypeExtension
        0x4DBBL, // Seek
        0x6924L, // ChapterTranslate
        TRACK_ENTRY,
        0x41E4L, // BlockAdditionMapping
        0x6624L, // TrackTranslate
        0xE0L, // Video
        0x55B0L, // Colour
        0x55D0L, // MasteringMetadata
        0x7670L, // Projection
        0xE1L, // Audio
        0xE2L, // TrackOperation
        0xE3L, // TrackCombinePlanes
        0xE4L, // TrackPlane
        0xE9L, // TrackJoinBlocks
        0x6D80L, // ContentEncodings
        0x6240L, // ContentEncoding
        0x5034L, // ContentCompression
        0x5035L, // ContentEncryption
        0x47E7L, // ContentEncAESSettings
        0xBBL, // CuePoint
        0xB7L, // CueTrackPositions
        0xDBL, // CueReference
        0x45B9L, // EditionEntry
        0x4520L, // EditionDisplay
        0xB6L, // ChapterAtom
        0x8FL, // ChapterTrack
        CHAPTER_DISPLAY,
        0x6944L, // ChapProcess
        0x6911L, // ChapProcessCommand
        0x63C0L, // Targets
    )

    /** The parts that hold words a person wrote, outside the tags: the segment's title, a track's name and a chapter's name. */
    private val WORDS: Set<Long> = setOf(TITLE, TRACK_NAME, CHAP_STRING)

    private val EXIF_HEADER = "Exif\u0000\u0000".toByteArray(Charsets.ISO_8859_1)

    /**
     * A place written as ISO 6709 writes one: a latitude and a longitude,
     * each with its sign and nothing between them, in degrees, in degrees
     * and minutes, or in degrees, minutes and seconds. Either both have a
     * fraction (`+30.9010+075.8573`), or the slash that ends such a string
     * follows them, after a height or none (`+30+075/`, `+3054+07551+247/`).
     */
    private val COORDINATES = Regex(
        """[+-]\d{2}(?:\d{2}){0,2}\.\d+[+-]\d{3}(?:\d{2}){0,2}\.\d+""" +
            """|[+-]\d{2}(?:\d{2}){0,2}(?:\.\d+)?[+-]\d{3}(?:\d{2}){0,2}(?:\.\d+)?(?:[+-]\d+(?:\.\d+)?)?/""",
    )

    /** The most elements read in one file: a recording of hours has fewer, and a file made to keep the client busy is given up. */
    private const val MOST_ELEMENTS = 2_000_000

    /** How deep a part may stand inside others. */
    private const val MAX_DEPTH = 32

    /** The most bytes of a name or of words that are read. A longer name, title or chapter's name is taken to name a place. */
    private const val MOST_NAME = 4096

    /** The most bytes a `Void` holds when its size is written in one byte. */
    private const val MOST_SHORT_VOID = 126L

    private const val BUFFER = 64 * 1024

    /** Whether these first bytes begin an EBML file, which a Matroska or WebM file is. */
    fun isEbml(head: ByteArray): Boolean =
        head.size >= 4 && head[0] == 0x1A.toByte() && head[1] == 0x45.toByte() && head[2] == 0xDF.toByte() && head[3] == 0xA3.toByte()

    /** Whether words hold a place written as coordinates ([COORDINATES]). */
    fun holdsCoordinates(words: String): Boolean = COORDINATES.containsMatchIn(words)

    /**
     * Whether the words of a title or a name name a place: one of them is a
     * word a tag's name is known by ([VideoData.isLocationKeyName]), or they
     * hold coordinates.
     */
    fun namesPlace(words: String): Boolean = VideoData.isLocationKeyName(words) || holdsCoordinates(words)

    /** What [inspect] found in a Matroska or WebM file, or what [clean] overwrote in one. */
    data class Found(
        /** The tracks the file has. */
        val tracks: Int = 0,
        /** Those of them that are sound. */
        val soundTracks: Int = 0,
        /** Tags that name a place, hold coordinates or hold a picture. */
        val placeTags: Int = 0,
        /** Titles, tracks' names and chapters' names that name a place. */
        val namedPlaces: Int = 0,
        /** Files attached to it. */
        val attachments: Int = 0,
        /** `Void` elements that still hold bytes of what stood there. */
        val filledVoids: Int = 0,
        /** The file did not read cleanly as Matroska or WebM. */
        val broken: Boolean = false,
    ) {
        /** Sound alone: it read cleanly, has a track, and every track is audio. */
        val soundOnly: Boolean get() = !broken && tracks > 0 && soundTracks == tracks

        /** Nothing in it is left to overwrite. */
        val clean: Boolean get() = !broken && placeTags == 0 && namedPlaces == 0 && attachments == 0 && filledVoids == 0
    }

    private data class Erase(val start: Long, val length: Long)

    private class WalkResult(val erases: List<Erase>, val found: Found)

    /** Inspects [file] without changing it. */
    fun inspect(file: File): Found = walk(file).found

    /**
     * Copies [from] to [to] with every tag, title and name that names a
     * place, every attached file, every `Void` that still holds bytes, and
     * the `CRC-32` of each part so changed, overwritten in place as a `Void`
     * of identical byte length. If [from] does not read cleanly, [to] is not
     * written and [Found.broken] is true.
     */
    fun clean(from: File, to: File): Found {
        val plan = walk(from)
        if (plan.found.broken) return plan.found
        val erases = plan.erases.sortedBy { it.start }
        RandomAccessFile(from, "r").use { input ->
            to.outputStream().buffered().use { output ->
                val buffer = ByteArray(BUFFER)
                var cursor = 0L
                val total = input.length()
                for (erase in erases) {
                    if (erase.start < cursor) continue
                    copyRange(input, output, cursor, erase.start - cursor, buffer)
                    writeVoid(output, erase.length, buffer)
                    cursor = erase.start + erase.length
                }
                if (cursor < total) {
                    copyRange(input, output, cursor, total - cursor, buffer)
                }
            }
        }
        return plan.found
    }

    private fun copyRange(input: RandomAccessFile, output: OutputStream, start: Long, count: Long, buffer: ByteArray) {
        if (count <= 0L) return
        input.seek(start)
        var left = count
        while (left > 0L) {
            val step = minOf(buffer.size.toLong(), left).toInt()
            val read = input.read(buffer, 0, step)
            if (read < 0) error("the file ended early")
            output.write(buffer, 0, read)
            left -= read
        }
    }

    /**
     * Writes a `Void` element of [length] bytes in all: its id, the size of
     * what follows, and zeros. The size is written in one byte where that is
     * enough, and in eight, the longest EBML has, where it is not; [scratch]
     * is filled with the zeros.
     */
    private fun writeVoid(output: OutputStream, length: Long, scratch: ByteArray) {
        require(length >= 2L) { "an element is at least 2 bytes: $length" }
        output.write(VOID.toInt())
        var left: Long
        if (length - 2L <= MOST_SHORT_VOID) {
            left = length - 2L
            output.write(0x80 or left.toInt())
        } else {
            left = length - 9L
            output.write(0x01)
            for (i in 6 downTo 0) output.write((left ushr (8 * i)).toInt() and 0xFF)
        }
        scratch.fill(0)
        while (left > 0L) {
            val step = minOf(scratch.size.toLong(), left).toInt()
            output.write(scratch, 0, step)
            left -= step
        }
    }

    private fun walk(file: File): WalkResult = try {
        RandomAccessFile(file, "r").use { input ->
            val total = input.length()
            if (total < 5L) WalkResult(emptyList(), Found(broken = true)) else Walker(Window(input, total)).walkAll()
        }
    } catch (_: Exception) {
        WalkResult(emptyList(), Found(broken = true))
    }

    /** Reads a file's bytes at any place through one buffer, so a walk that goes forward asks the system once for many elements. */
    private class Window(private val input: RandomAccessFile, val total: Long) {
        private val buffer = ByteArray(BUFFER)
        private var from = 0L
        private var held = 0

        /** The byte at [at], as 0 to 255. */
        fun byte(at: Long): Int {
            if (at < from || at >= from + held) fill(at)
            return buffer[(at - from).toInt()].toInt() and 0xFF
        }

        private fun fill(at: Long) {
            if (at < 0L || at >= total) error("outside the file")
            input.seek(at)
            var read = 0
            while (read < buffer.size) {
                val n = input.read(buffer, read, buffer.size - read)
                if (n < 0) break
                read += n
            }
            if (read <= 0) error("the file ended early")
            from = at
            held = read
        }
    }

    /** One element: its id, where it starts, where what it holds starts, and how many bytes that is (fewer than none when it does not say). */
    private class Header(val id: Long, val start: Long, val dataStart: Long, val size: Long) {
        val unknown: Boolean get() = size < 0L
        val end: Long get() = dataStart + size
        val length: Long get() = end - start
    }

    private class Walker(private val input: Window) {
        private val erases = ArrayList<Erase>()
        private var tracks = 0
        private var soundTracks = 0
        private var placeTags = 0
        private var namedPlaces = 0
        private var attachments = 0
        private var filledVoids = 0
        private var broken = false
        private var elements = 0

        /** Something in the cluster of no known size that was read last was overwritten. */
        private var clusterBlanked = false

        fun walkAll(): WalkResult {
            val total = input.total
            val head = header(0L, total)
            if (head == null || head.id != EBML || head.unknown || !namesMatroska(head)) {
                broken = true
            } else {
                // What the header holds besides: a blank part in it that still holds bytes is overwritten like any other.
                walkPart(head, 0)
                val segment = segmentAfter(head.end, total)
                if (segment == null) {
                    broken = true
                } else {
                    // One segment, to the end of the file: nothing stands after it.
                    val end = if (segment.unknown) total else segment.end
                    if (end != total) broken = true else walkSegment(segment, end)
                }
            }
            return WalkResult(
                erases = if (broken) emptyList() else erases,
                found = Found(
                    tracks = tracks,
                    soundTracks = soundTracks,
                    placeTags = placeTags,
                    namedPlaces = namedPlaces,
                    attachments = attachments,
                    filledVoids = filledVoids,
                    broken = broken,
                ),
            )
        }

        /**
         * The element that starts at [at] in a parent that ends at [limit];
         * null when its id or its size does not read, when it runs past the
         * parent, or when the file has more elements than are read.
         */
        private fun header(at: Long, limit: Long): Header? {
            if (++elements > MOST_ELEMENTS || at >= limit) return null
            val first = input.byte(at)
            // An id is one to four bytes, as many as its first byte has zero bits before its first one, and one more.
            val idLength = Integer.numberOfLeadingZeros(first) - 23
            if (idLength > 4 || at + idLength >= limit) return null
            var id = first.toLong()
            for (i in 1 until idLength) id = (id shl 8) or input.byte(at + i).toLong()
            // An id with every bit after its length set is no element's. Nor is one with none of them set, but for the one such id
            // Matroska itself has: that of a chapter's display.
            if (id == (1L shl (7 * idLength + 1)) - 1L) return null
            if (id == (1L shl (7 * idLength)) && id != CHAPTER_DISPLAY) return null
            val sizeAt = at + idLength
            val sizeFirst = input.byte(sizeAt)
            val sizeLength = Integer.numberOfLeadingZeros(sizeFirst) - 23
            if (sizeLength > 8 || sizeAt + sizeLength > limit) return null
            var size = (sizeFirst and (0xFF ushr sizeLength)).toLong()
            for (i in 1 until sizeLength) size = (size shl 8) or input.byte(sizeAt + i).toLong()
            val dataStart = sizeAt + sizeLength
            // Every bit set says the size is not known.
            if (size == (1L shl (7 * sizeLength)) - 1L) return Header(id, at, dataStart, -1L)
            if (size > limit - dataStart) return null
            return Header(id, at, dataStart, size)
        }

        /** Calls [each] for every child of [master], which has a known size; a child that does not read, or one of no known size, breaks the file. */
        private fun children(master: Header, each: (Header) -> Unit) {
            var cursor = master.dataStart
            val end = master.end
            while (cursor < end && !broken) {
                val child = header(cursor, end)
                if (child == null || child.unknown) {
                    broken = true
                    return
                }
                each(child)
                cursor = child.end
            }
        }

        /** The words an element holds, up to their first zero byte; null when it holds more than [most] bytes. */
        private fun text(element: Header, most: Int): String? = if (element.size > most.toLong()) null else wordsHead(element, most)

        /** The first [most] bytes of the words an element holds, up to their first zero byte. */
        private fun wordsHead(element: Header, most: Int): String {
            val count = minOf(element.size, most.toLong()).toInt()
            val bytes = ByteArray(count) { input.byte(element.dataStart + it).toByte() }
            return String(bytes, Charsets.UTF_8).substringBefore('\u0000')
        }

        /** The whole number an element holds; one longer than eight bytes breaks the file. */
        private fun number(element: Header): Long {
            if (element.size > 8L) {
                broken = true
                return -1L
            }
            var value = 0L
            for (i in 0 until element.size.toInt()) value = (value shl 8) or input.byte(element.dataStart + i).toLong()
            return value
        }

        /** Whether the EBML header says the file is Matroska or WebM, with ids and sizes no longer than are read here. */
        private fun namesMatroska(head: Header): Boolean {
            var docType: String? = null
            var readable = true
            children(head) { child ->
                when (child.id) {
                    DOC_TYPE -> docType = text(child, 32)
                    MAX_ID_LENGTH -> if (number(child) !in 0L..4L) readable = false
                    MAX_SIZE_LENGTH -> if (number(child) !in 0L..8L) readable = false
                }
            }
            return !broken && readable && (docType == "matroska" || docType == "webm")
        }

        /** The segment that follows the EBML header, with nothing between them but room left blank; null when something else stands there. */
        private fun segmentAfter(from: Long, total: Long): Header? {
            var cursor = from
            while (cursor < total) {
                val next = header(cursor, total) ?: return null
                if (next.id == SEGMENT) return next
                if (next.id != VOID || next.unknown) return null
                blankIfFilled(next)
                cursor = next.end
            }
            return null
        }

        private fun walkSegment(segment: Header, end: Long) {
            var cursor = segment.dataStart
            var crc: Header? = null
            var blanked = false
            while (cursor < end && !broken) {
                val child = header(cursor, end)
                if (child == null) {
                    broken = true
                    return
                }
                if (child.id == CLUSTER) {
                    // The sound itself. One that says its size is passed over whole: nothing in it is read. One of no known size
                    // ends where the next part of the segment begins, and is read part by part to find where that is.
                    if (child.unknown) {
                        cursor = clusterEnd(child.dataStart, end)
                        if (clusterBlanked) {
                            blanked = true
                            clusterBlanked = false
                        }
                    } else {
                        cursor = child.end
                    }
                    continue
                }
                if (child.unknown) {
                    broken = true
                    return
                }
                when (child.id) {
                    SEEK_HEAD, INFO, TRACKS, CUES, CHAPTERS -> if (walkPart(child, 0)) blanked = true
                    TAGS -> if (walkTags(child)) blanked = true
                    ATTACHMENTS -> if (walkAttachments(child)) blanked = true
                    VOID -> if (blankIfFilled(child)) blanked = true
                    CRC_32 -> crc = child
                    // Nothing else stands in a segment.
                    else -> broken = true
                }
                cursor = child.end
            }
            checksum(crc, blanked)
        }

        /**
         * Where a cluster of no known size ends: at the first element that is
         * no part of a cluster but of the segment or of the file, else at the
         * end of the segment. Its parts are read only for where they end, but
         * for one kind: a blank part met on the way may be the segment's as
         * well as the cluster's, there is no telling, so one that still holds
         * bytes is overwritten as anywhere else, with the cluster's own
         * `CRC-32` where it has one, and [clusterBlanked] says so.
         */
        private fun clusterEnd(from: Long, end: Long): Long {
            var cursor = from
            var crc: Header? = null
            var blanked = false
            while (cursor < end) {
                val child = header(cursor, end)
                if (child == null) {
                    broken = true
                    return end
                }
                if (child.id in ENDS_CLUSTER) break
                if (child.unknown) {
                    broken = true
                    return end
                }
                when (child.id) {
                    VOID -> if (blankIfFilled(child)) blanked = true
                    CRC_32 -> crc = child
                }
                cursor = child.end
            }
            if (checksum(crc, blanked)) clusterBlanked = true
            return cursor
        }

        /**
         * Reads a part that holds others, and every part inside it that
         * holds others in turn ([HOLDERS]), down to the parts that hold a
         * value: each id and size is read and held to its parent. A `Void`
         * that still holds bytes is overwritten; so is a title, a track's
         * name or a chapter's name that names a place ([WORDS]); and a track
         * is counted, as sound or not. True when something in the part was
         * overwritten, and then its `CRC-32` is too.
         */
        private fun walkPart(master: Header, depth: Int): Boolean {
            if (depth > MAX_DEPTH) {
                broken = true
                return false
            }
            var crc: Header? = null
            var blanked = false
            children(master) { child ->
                when {
                    child.id == VOID -> if (blankIfFilled(child)) blanked = true
                    child.id == CRC_32 -> crc = child
                    child.id == TRACK_ENTRY -> if (trackEntry(child, depth + 1)) blanked = true
                    child.id in WORDS -> if (wordsNamePlace(child)) blanked = true
                    child.id in HOLDERS -> if (walkPart(child, depth + 1)) blanked = true
                }
            }
            return checksum(crc, blanked)
        }

        /**
         * Counts one track, and counts it as sound when it says so once or
         * more and never says anything else. Then what the track holds is
         * read as any part that holds others is ([walkPart]): true when
         * something in it was overwritten.
         */
        private fun trackEntry(entry: Header, depth: Int): Boolean {
            var typed = false
            var coded = false
            var sound = true
            children(entry) { child ->
                when (child.id) {
                    TRACK_TYPE -> {
                        typed = true
                        if (number(child) != AUDIO) sound = false
                    }
                    CODEC_ID -> {
                        coded = true
                        if (text(child, 64)?.startsWith("A_") != true) sound = false
                    }
                }
            }
            tracks++
            if (typed && coded && sound) soundTracks++
            return if (broken) false else walkPart(entry, depth)
        }

        /**
         * Whether the words of a title, a track's name or a chapter's name
         * name a place ([namesPlace]); words longer than are read are taken
         * to. The element is overwritten when they do.
         */
        private fun wordsNamePlace(element: Header): Boolean {
            val words = text(element, MOST_NAME)
            if (words != null && !namesPlace(words)) return false
            namedPlaces++
            blank(element)
            return true
        }

        /** Reads the tags; true when something in them was overwritten. */
        private fun walkTags(master: Header): Boolean {
            var crc: Header? = null
            var blanked = false
            children(master) { child ->
                when (child.id) {
                    TAG -> if (walkTag(child)) blanked = true
                    VOID -> if (blankIfFilled(child)) blanked = true
                    CRC_32 -> crc = child
                }
            }
            return checksum(crc, blanked)
        }

        /** Reads what one `Tag` holds for its target; true when something in it was overwritten. */
        private fun walkTag(master: Header): Boolean {
            var crc: Header? = null
            var blanked = false
            children(master) { child ->
                when (child.id) {
                    SIMPLE_TAG -> if (simpleTag(child, 0)) blanked = true
                    VOID -> if (blankIfFilled(child)) blanked = true
                    CRC_32 -> crc = child
                    // What the tag is for: read as any part that holds others.
                    else -> if (child.id in HOLDERS && walkPart(child, 1)) blanked = true
                }
            }
            return checksum(crc, blanked)
        }

        /**
         * Reads one `SimpleTag`. One whose name names a place, whose words
         * hold coordinates, or that holds a picture, is overwritten whole,
         * with the tags inside it; any other stays, and the tags inside it
         * are read the same way. True when it, or something in it, was
         * overwritten.
         */
        private fun simpleTag(master: Header, depth: Int): Boolean {
            if (depth > MAX_DEPTH) {
                broken = true
                return false
            }
            var place = false
            children(master) { child ->
                when (child.id) {
                    TAG_NAME -> {
                        val name = text(child, MOST_NAME)
                        if (name == null || VideoData.isLocationKeyName(name)) place = true
                    }
                    TAG_STRING -> if (holdsCoordinates(wordsHead(child, MOST_NAME))) place = true
                    TAG_BINARY -> if (holdsPicture(child)) place = true
                }
            }
            if (broken) return false
            if (place) {
                placeTags++
                blank(master)
                return true
            }
            var crc: Header? = null
            var blanked = false
            children(master) { child ->
                when (child.id) {
                    SIMPLE_TAG -> if (simpleTag(child, depth + 1)) blanked = true
                    VOID -> if (blankIfFilled(child)) blanked = true
                    CRC_32 -> crc = child
                }
            }
            return checksum(crc, blanked)
        }

        /** Whether what an element holds begins as a picture or as Exif data does. */
        private fun holdsPicture(element: Header): Boolean {
            val count = minOf(element.size, PictureData.HEAD.toLong()).toInt()
            val head = ByteArray(count) { input.byte(element.dataStart + it).toByte() }
            return PictureData.kindOf(head) != null || (head.size >= EXIF_HEADER.size && EXIF_HEADER.indices.all { head[it] == EXIF_HEADER[it] })
        }

        /**
         * Reads what is attached. Every attached file is overwritten: a cover
         * picture can hold the place it was taken, a file of another kind is
         * not read for one, and sound needs nothing that is attached. True
         * when something was overwritten.
         */
        private fun walkAttachments(master: Header): Boolean {
            var crc: Header? = null
            var blanked = false
            children(master) { child ->
                when (child.id) {
                    ATTACHED_FILE -> {
                        attachments++
                        blank(child)
                        blanked = true
                    }
                    VOID -> if (blankIfFilled(child)) blanked = true
                    CRC_32 -> crc = child
                }
            }
            return checksum(crc, blanked)
        }

        /** A part that had something in it overwritten has its `CRC-32` overwritten too: it no longer holds. Answers [blanked]. */
        private fun checksum(crc: Header?, blanked: Boolean): Boolean {
            if (blanked && crc != null) blank(crc)
            return blanked
        }

        private fun blank(element: Header) {
            erases += Erase(element.start, element.length)
        }

        /** A `Void` that still holds bytes of what stood there is overwritten with one that holds none; true when it was. */
        private fun blankIfFilled(element: Header): Boolean {
            for (at in element.dataStart until element.end) {
                if (input.byte(at) != 0) {
                    filledVoids++
                    blank(element)
                    return true
                }
            }
            return false
        }
    }
}
