package com.navree.client.platform

import com.navree.client.design.FileRules.Picture
import java.io.ByteArrayOutputStream
import java.io.File
import java.io.InputStream
import java.io.RandomAccessFile
import java.util.zip.Inflater

/**
 * What a picture is, read from its own bytes, and the parts of it outside its
 * Exif data that can say where it was taken. Nothing here trusts the type a
 * provider gave or the name a file has. Plain Kotlin over files and bytes, so
 * every rule is tested without a device.
 *
 * For the three kinds the client rewrites (JPEG, PNG, WebP), [clean] copies a
 * picture leaving out: an XMP packet that names a place (in a JPEG with its
 * extended parts), every Exif block after the first (the one Exif reader and
 * writer of the platform see only the first), the containers other programs
 * keep Exif, XMP or IPTC data in beside the standard ones (a JPEG's Photoshop
 * block, a PNG's "Raw profile type" text), a JPEG's index of further pictures
 * (MPF), and everything after the end of the picture (where a motion photo
 * keeps its video and a JPEG further pictures). [inspect] says what of that a
 * file still holds. The place in the Exif data itself is the platform's
 * `ExifInterface`'s to take out ([PickedFiles]).
 */
internal object PictureData {
    /** How many first bytes [kindOf] needs to see a kind; it reads fewer when the file is shorter. */
    const val HEAD = 64

    /** The kind of picture these first bytes start, or null when they start none of them. */
    fun kindOf(head: ByteArray): Picture? {
        fun at(offset: Int, vararg bytes: Int) = head.size >= offset + bytes.size && bytes.indices.all { head[offset + it].toInt() and 0xFF == bytes[it] }
        fun text(offset: Int, value: String) = at(offset, *value.map { it.code }.toIntArray())
        return when {
            at(0, 0xFF, 0xD8, 0xFF) -> Picture.Jpeg
            at(0, 0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A) -> Picture.Png
            text(0, "RIFF") && text(8, "WEBP") -> Picture.Webp
            text(0, "GIF87a") || text(0, "GIF89a") -> Picture.Gif
            text(0, "II*\u0000") || text(0, "MM\u0000*") || text(0, "II+\u0000") || text(0, "MM\u0000+") ||
                text(0, "IIRO") || text(0, "IIRS") || text(0, "MMOR") || text(0, "IIU\u0000") || text(0, "FUJIFILMCCD-RAW") -> Picture.Raw
            text(4, "ftyp") -> isoKind(head)
            else -> null
        }
    }

    private val AVIF_BRANDS = setOf("avif", "avis")
    private val HEIC_BRANDS = setOf("heic", "heix", "hevc", "hevx", "heim", "heis", "hevm", "hevs", "mif1", "msf1", "mif2", "heif")
    private val RAW_BRANDS = setOf("crx ")

    /** An ISO media file's brands (its major brand and the compatible ones its first box names). */
    private fun isoKind(head: ByteArray): Picture? {
        val size = ((head[0].toInt() and 0xFF) shl 24) or ((head[1].toInt() and 0xFF) shl 16) or ((head[2].toInt() and 0xFF) shl 8) or (head[3].toInt() and 0xFF)
        val end = minOf(head.size, if (size in 16..4096) size else head.size)
        val brands = buildList {
            if (head.size >= 12) add(String(head, 8, 4, Charsets.ISO_8859_1))
            var at = 16
            while (at + 4 <= end) {
                add(String(head, at, 4, Charsets.ISO_8859_1))
                at += 4
            }
        }
        return when {
            brands.any { it in AVIF_BRANDS } -> Picture.Avif
            brands.any { it in RAW_BRANDS } -> Picture.Raw
            brands.any { it in HEIC_BRANDS } -> Picture.Heic
            else -> null
        }
    }

    /** The kind of picture a file is, by its first bytes. */
    fun kindOf(file: File): Picture? {
        val head = ByteArray(HEAD)
        val read = file.inputStream().use { input -> readUpTo(input, head) }
        return kindOf(head.copyOf(read))
    }

    private val PLACE = Regex("GPS[A-Z]|Location|City|Country|Province|State\\b|Sublocation")

    /**
     * Whether an XMP packet names a place: a GPS property of the Exif schema
     * (`exif:GPSLatitude`, `GPSDestLongitude` and the rest), or a place by its
     * name (IPTC's `LocationCreated` and `LocationShown`, Photoshop's `City`,
     * `State` and `Country`).
     */
    fun namesPlace(xmp: ByteArray): Boolean = PLACE.containsMatchIn(String(xmp, Charsets.ISO_8859_1))

    /** What [inspect] found in a picture, or what [clean] left out of one. */
    data class Found(
        /** Exif blocks: a JPEG's `Exif` segments, a PNG's `eXIf` chunks, a WebP's `EXIF` chunks. */
        val exifBlocks: Int = 0,
        /** XMP packets (a JPEG's extended XMP counted as part of its main packet) that name a place. */
        val xmpWithPlace: Int = 0,
        /** Blocks other programs keep metadata in: a JPEG's Photoshop and MPF segments, a PNG's "Raw profile type" text. */
        val otherMetadata: Int = 0,
        /** Bytes after the end of the picture. */
        val trailing: Long = 0,
        /** The structure did not read as its kind's. */
        val broken: Boolean = false,
    ) {
        /** Nothing but the first Exif block can hold a place any more. */
        val clean: Boolean get() = !broken && exifBlocks <= 1 && xmpWithPlace == 0 && otherMetadata == 0 && trailing == 0L
    }

    /** A part of a file, from [start] for [length] bytes, kept or left out. */
    private class Part(val start: Long, val length: Long, val keep: Boolean, val replace: ByteArray? = null)

    /** How a file is copied: its parts in order, and what was left out. */
    private class Plan(val parts: List<Part>, val found: Found)

    /** What in this picture can still say where it was taken, beside its first Exif block. */
    fun inspect(file: File, kind: Picture): Found = plan(file, kind).found

    /**
     * Copies [from] to [to] without what [inspect] counts, and answers what was
     * left out. A file that does not read as its [kind] is not copied: the
     * answer says it is [Found.broken].
     */
    fun clean(from: File, to: File, kind: Picture): Found {
        val plan = plan(from, kind)
        if (plan.found.broken) return plan.found
        RandomAccessFile(from, "r").use { input ->
            to.outputStream().buffered().use { output ->
                val buffer = ByteArray(BUFFER)
                for (part in plan.parts) {
                    if (!part.keep) continue
                    if (part.replace != null) {
                        output.write(part.replace)
                        continue
                    }
                    input.seek(part.start)
                    var left = part.length
                    while (left > 0) {
                        val read = input.read(buffer, 0, minOf(buffer.size.toLong(), left).toInt())
                        if (read < 0) error("the file ended early")
                        output.write(buffer, 0, read)
                        left -= read
                    }
                }
            }
        }
        return plan.found
    }

    private fun plan(file: File, kind: Picture): Plan = try {
        RandomAccessFile(file, "r").use { input ->
            when (kind) {
                Picture.Jpeg -> jpeg(input)
                Picture.Png -> png(input)
                Picture.Webp -> webp(input)
                else -> Plan(emptyList(), Found(broken = true))
            }
        }
    } catch (_: Exception) {
        Plan(emptyList(), Found(broken = true))
    }

    // ------------------------------------------------------------------ JPEG

    private val EXIF = "Exif\u0000\u0000".toByteArray(Charsets.ISO_8859_1)
    private val XMP = "http://ns.adobe.com/xap/1.0/\u0000".toByteArray(Charsets.ISO_8859_1)
    private val XMP_EXTENSION = "http://ns.adobe.com/xmp/extension/\u0000".toByteArray(Charsets.ISO_8859_1)
    private val PHOTOSHOP = "Photoshop 3.0\u0000".toByteArray(Charsets.ISO_8859_1)
    private val MPF = "MPF\u0000".toByteArray(Charsets.ISO_8859_1)

    /** The most bytes of XMP read to decide whether it names a place; a packet longer than this is taken to. */
    private const val MOST_XMP = 16L * 1024 * 1024

    private fun RandomAccessFile.u8(): Int = read().also { if (it < 0) error("the file ended early") }

    private fun RandomAccessFile.u16(): Int = (u8() shl 8) or u8()

    private fun RandomAccessFile.bytes(count: Int): ByteArray = ByteArray(count).also { readFully(it) }

    private fun ByteArray.startsWith(prefix: ByteArray): Boolean = size >= prefix.size && prefix.indices.all { this[it] == prefix[it] }

    /**
     * A JPEG's segments up to its end (EOI), with the data of each scan read
     * through to the marker after it. Segments are kept or left out whole.
     */
    private fun jpeg(input: RandomAccessFile): Plan {
        if (input.u8() != 0xFF || input.u8() != 0xD8) return Plan(emptyList(), Found(broken = true))
        val parts = ArrayList<Part>()
        parts += Part(0, 2, keep = true)
        var exif = 0
        var other = 0
        var xmpPlace = 0
        var mainXmpDropped = false
        // The extended XMP segments, decided together once all are seen.
        val extensions = ArrayList<Int>()
        val extension = ByteArrayOutputStream()
        var extensionTooLong = false
        var end = -1L
        loop@ while (true) {
            val start = input.filePointer
            if (input.u8() != 0xFF) return Plan(emptyList(), Found(broken = true))
            var marker = input.u8()
            // Fill bytes before a marker.
            while (marker == 0xFF) marker = input.u8()
            when {
                marker == 0xD9 -> {
                    parts += Part(start, input.filePointer - start, keep = true)
                    end = input.filePointer
                    break@loop
                }
                marker == 0x01 || marker in 0xD0..0xD7 -> parts += Part(start, input.filePointer - start, keep = true)
                else -> {
                    val length = input.u16()
                    if (length < 2) return Plan(emptyList(), Found(broken = true))
                    val bodyAt = input.filePointer
                    val head = input.bytes(minOf(length - 2, 64))
                    var keep = true
                    when {
                        marker == 0xE1 && head.startsWith(EXIF) -> {
                            exif++
                            keep = exif == 1
                        }
                        marker == 0xE1 && head.startsWith(XMP) -> {
                            input.seek(bodyAt)
                            if (namesPlace(input.bytes(length - 2))) {
                                xmpPlace++
                                mainXmpDropped = true
                                keep = false
                            }
                        }
                        marker == 0xE1 && head.startsWith(XMP_EXTENSION) -> {
                            // The part's own data follows a 32-byte id, its full length and its offset.
                            input.seek(bodyAt)
                            val body = input.bytes(length - 2)
                            val from = XMP_EXTENSION.size + 40
                            if (body.size < from) return Plan(emptyList(), Found(broken = true))
                            if (extension.size() + body.size <= MOST_XMP) extension.write(body, from, body.size - from) else extensionTooLong = true
                            extensions += parts.size
                        }
                        marker == 0xED && head.startsWith(PHOTOSHOP) -> {
                            other++
                            keep = false
                        }
                        marker == 0xE2 && head.startsWith(MPF) -> {
                            other++
                            keep = false
                        }
                    }
                    input.seek(bodyAt + length - 2)
                    parts += Part(start, 2L + length, keep)
                    if (marker == 0xDA) {
                        // A scan's data runs to the next marker that is not a restart marker or a stuffed zero.
                        val dataAt = input.filePointer
                        val next = scanEnd(input)
                        parts += Part(dataAt, next - dataAt, keep = true)
                        input.seek(next)
                    }
                }
            }
        }
        if (extensions.isNotEmpty() && (mainXmpDropped || extensionTooLong || namesPlace(extension.toByteArray()))) {
            if (!mainXmpDropped) xmpPlace++
            for (index in extensions) parts[index] = Part(parts[index].start, parts[index].length, keep = false)
        }
        val trailing = input.length() - end
        return Plan(parts, Found(exifBlocks = exif, xmpWithPlace = xmpPlace, otherMetadata = other, trailing = trailing))
    }

    /** Where a scan's entropy-coded data ends: the offset of the next marker that is not a restart marker. */
    private fun scanEnd(input: RandomAccessFile): Long {
        val buffer = ByteArray(BUFFER)
        var base = input.filePointer
        var previousWasFF = false
        while (true) {
            val read = input.read(buffer)
            if (read < 0) error("the scan does not end")
            for (i in 0 until read) {
                val byte = buffer[i].toInt() and 0xFF
                if (previousWasFF) {
                    if (byte != 0x00 && byte !in 0xD0..0xD7 && byte != 0xFF) return base + i - 1
                    previousWasFF = byte == 0xFF
                } else {
                    previousWasFF = byte == 0xFF
                }
            }
            base += read
        }
    }

    // ------------------------------------------------------------------ PNG

    private val PNG_SIGNATURE = byteArrayOf(0x89.toByte(), 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A)
    private const val XMP_KEYWORD = "XML:com.adobe.xmp"

    /** A PNG's chunks up to IEND; a chunk is kept or left out whole, its CRC with it. */
    private fun png(input: RandomAccessFile): Plan {
        if (!input.bytes(8).contentEquals(PNG_SIGNATURE)) return Plan(emptyList(), Found(broken = true))
        val parts = ArrayList<Part>()
        parts += Part(0, 8, keep = true)
        var exif = 0
        var other = 0
        var xmpPlace = 0
        var end = -1L
        while (true) {
            val start = input.filePointer
            val length = input.readInt().toLong() and 0xFFFFFFFFL
            val type = String(input.bytes(4), Charsets.ISO_8859_1)
            val dataAt = input.filePointer
            if (dataAt + length + 4 > input.length()) return Plan(emptyList(), Found(broken = true))
            var keep = true
            when (type) {
                "eXIf" -> {
                    exif++
                    keep = exif == 1
                }
                "tEXt", "zTXt", "iTXt" -> {
                    val keyword = String(input.bytes(minOf(length, 80L).toInt()), Charsets.ISO_8859_1).substringBefore('\u0000')
                    when {
                        keyword.startsWith("Raw profile type", ignoreCase = true) -> {
                            other++
                            keep = false
                        }
                        keyword == XMP_KEYWORD -> {
                            val text = if (length > MOST_XMP) null else {
                                input.seek(dataAt)
                                pngText(type, input.bytes(length.toInt()))
                            }
                            if (text == null || namesPlace(text)) {
                                xmpPlace++
                                keep = false
                            }
                        }
                    }
                }
            }
            input.seek(dataAt + length + 4)
            parts += Part(start, input.filePointer - start, keep)
            if (type == "IEND") {
                end = input.filePointer
                break
            }
        }
        return Plan(parts, Found(exifBlocks = exif, xmpWithPlace = xmpPlace, otherMetadata = other, trailing = input.length() - end))
    }

    /** The text of a PNG text chunk, inflated when it is compressed; null when it does not read. */
    private fun pngText(type: String, data: ByteArray): ByteArray? {
        val keywordEnd = data.indexOf(0)
        if (keywordEnd < 0) return null
        return when (type) {
            "tEXt" -> data.copyOfRange(keywordEnd + 1, data.size)
            "zTXt" -> inflate(data.copyOfRange(keywordEnd + 2, data.size))
            else -> {
                // keyword, 0, compressed, method, language, 0, translated keyword, 0, text
                val compressed = data.getOrNull(keywordEnd + 1)?.toInt() == 1
                val language = data.indexOf(0, keywordEnd + 3)
                val translated = if (language < 0) -1 else data.indexOf(0, language + 1)
                if (translated < 0) return null
                val text = data.copyOfRange(translated + 1, data.size)
                if (compressed) inflate(text) else text
            }
        }
    }

    private fun ByteArray.indexOf(value: Int, from: Int = 0): Int {
        for (i in from until size) if (this[i].toInt() == value) return i
        return -1
    }

    private fun inflate(data: ByteArray): ByteArray? = try {
        val inflater = Inflater()
        inflater.setInput(data)
        val out = ByteArrayOutputStream()
        val buffer = ByteArray(BUFFER)
        while (!inflater.finished()) {
            val count = inflater.inflate(buffer)
            if (count == 0 && (inflater.needsInput() || inflater.needsDictionary())) break
            out.write(buffer, 0, count)
            if (out.size() > MOST_XMP) return null
        }
        inflater.end()
        out.toByteArray()
    } catch (_: Exception) {
        null
    }

    // ------------------------------------------------------------------ WebP

    /** A WebP's chunks inside its RIFF size; the header's size is written again, and VP8X's XMP flag cleared when its XMP goes. */
    private fun webp(input: RandomAccessFile): Plan {
        val riff = input.bytes(12)
        if (String(riff, 0, 4, Charsets.ISO_8859_1) != "RIFF" || String(riff, 8, 4, Charsets.ISO_8859_1) != "WEBP") return Plan(emptyList(), Found(broken = true))
        val riffEnd = 8L + le32(riff, 4)
        if (riffEnd > input.length()) return Plan(emptyList(), Found(broken = true))
        class Chunk(val start: Long, val length: Long, val type: String, var keep: Boolean)
        val chunks = ArrayList<Chunk>()
        var exif = 0
        var xmpPlace = 0
        while (input.filePointer + 8 <= riffEnd) {
            val start = input.filePointer
            val header = input.bytes(8)
            val type = String(header, 0, 4, Charsets.ISO_8859_1)
            val size = le32(header, 4)
            val padded = size + (size and 1L)
            if (start + 8 + padded > riffEnd + (size and 1L)) return Plan(emptyList(), Found(broken = true))
            var keep = true
            when (type) {
                "EXIF" -> {
                    exif++
                    keep = exif == 1
                }
                "XMP " -> {
                    if (size > MOST_XMP || namesPlace(input.bytes(size.toInt()))) {
                        xmpPlace++
                        keep = false
                    }
                }
            }
            chunks += Chunk(start, minOf(8 + padded, riffEnd - start), type, keep)
            input.seek(start + 8 + padded)
        }
        val keptLength = chunks.filter { it.keep }.sumOf { it.length }
        val header = riff.copyOf().also { putLe32(it, 4, 4 + keptLength) }
        val parts = ArrayList<Part>()
        parts += Part(0, 12, keep = true, replace = header)
        val xmpGone = chunks.any { it.type == "XMP " && !it.keep }
        for (chunk in chunks) {
            if (chunk.keep && chunk.type == "VP8X" && xmpGone) {
                input.seek(chunk.start)
                val bytes = input.bytes(chunk.length.toInt())
                // The flags are the chunk's first byte: 0x04 says the file has XMP.
                bytes[8] = (bytes[8].toInt() and 0x04.inv()).toByte()
                parts += Part(chunk.start, chunk.length, keep = true, replace = bytes)
            } else {
                parts += Part(chunk.start, chunk.length, chunk.keep)
            }
        }
        return Plan(parts, Found(exifBlocks = exif, xmpWithPlace = xmpPlace, trailing = input.length() - riffEnd))
    }

    private fun le32(bytes: ByteArray, at: Int): Long =
        (bytes[at].toLong() and 0xFF) or ((bytes[at + 1].toLong() and 0xFF) shl 8) or ((bytes[at + 2].toLong() and 0xFF) shl 16) or ((bytes[at + 3].toLong() and 0xFF) shl 24)

    private fun putLe32(bytes: ByteArray, at: Int, value: Long) {
        for (i in 0 until 4) bytes[at + i] = (value shr (8 * i)).toByte()
    }

    // ------------------------------------------------------------------ an XMP packet anywhere

    private val RDF = "http://www.w3.org/1999/02/22-rdf-syntax-ns#".toByteArray(Charsets.ISO_8859_1)
    private val RDF_END = "RDF>".toByteArray(Charsets.ISO_8859_1)

    /**
     * Whether a file of a kind the client does not rewrite (HEIC, AVIF, GIF)
     * holds, as plain text, an XMP packet that names a place: each packet
     * from the RDF namespace it declares to the end of its `RDF` element, read
     * up to [MOST_XMP] bytes (a packet with no end by then is taken to name
     * one).
     */
    fun packetNamesPlace(input: InputStream): Boolean {
        val buffer = ByteArray(BUFFER)
        // The bytes of a packet being read, from its namespace on; null while none is.
        var packet: ByteArrayOutputStream? = null
        var matched = 0
        var ended = 0
        while (true) {
            val read = input.read(buffer)
            if (read < 0) break
            for (i in 0 until read) {
                val byte = buffer[i]
                val current = packet
                if (current == null) {
                    matched = if (byte == RDF[matched]) matched + 1 else if (byte == RDF[0]) 1 else 0
                    if (matched == RDF.size) {
                        packet = ByteArrayOutputStream()
                        matched = 0
                        ended = 0
                    }
                } else {
                    current.write(byte.toInt())
                    ended = if (byte == RDF_END[ended]) ended + 1 else if (byte == RDF_END[0]) 1 else 0
                    if (ended == RDF_END.size) {
                        if (namesPlace(current.toByteArray())) return true
                        packet = null
                        ended = 0
                    } else if (current.size() > MOST_XMP) {
                        return true
                    }
                }
            }
        }
        // A packet the file ended inside of is read as far as it went.
        return packet?.let { namesPlace(it.toByteArray()) } ?: false
    }

    private fun readUpTo(input: InputStream, into: ByteArray): Int {
        var total = 0
        while (total < into.size) {
            val read = input.read(into, total, into.size - total)
            if (read < 0) break
            total += read
        }
        return total
    }

    private const val BUFFER = 64 * 1024
}
