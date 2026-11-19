package com.navree.client.platform

import com.navree.client.design.FileRules.Video
import java.io.File
import java.io.RandomAccessFile

/**
 * What a video is, read from its own bytes (and from a `video/` media type when
 * its container is another), and the parts of an ISO media or QuickTime file
 * (MP4, MOV, M4V, 3GP) that can say where it was made. Plain Kotlin over files
 * and bytes, so every rule is tested without a device.
 *
 * Location boxes (`©xyz`, 3GPP `loci`, `gps `, `GPS `, `GPMF`), QuickTime
 * metadata items keyed to a place (`com.apple.quicktime.location.ISO6709` and
 * its companions in `keys` and `ilst`), iTunes freeform place items (`----`),
 * and XMP or XML boxes (`XMP_`, `xml `, `bxml`, `uuid`) whose packet names a
 * place ([PictureData.namesPlace]) are overwritten in place as `free` boxes of
 * the very same byte length, leaving every sample and chunk offset (`stco`,
 * `co64`, `tfhd`, `sidx`) intact. Tracks that are neither video nor audio
 * (`mdia` -> `hdlr` other than `vide` or `soun`, such as timed GPS or camera
 * metadata tracks) are counted in [Found.nonMediaTracks] so [PickedFiles] can
 * remux video and audio tracks alone before cleaning.
 */
internal object VideoData {
    const val HEAD = 512

    private val CLASSIC_QT_BOXES = setOf("moov", "mdat", "wide", "free", "skip", "pnot")
    private val CONTAINER_BOXES = setOf("moov", "trak", "edts", "mdia", "minf", "dinf", "stbl", "udta", "moof", "traf", "mfra")
    private val DIRECT_LOCATION_BOXES = setOf("\u00A9xyz", "loci", "gps ", "GPS ", "GPMF")
    private val XML_BOXES = setOf("XMP_", "xml ", "bxml", "uuid")

    /** Most bytes of an XML or UUID box read to check whether it names a place; larger boxes are treated as naming one. */
    private const val MOST_XML = 16L * 1024 * 1024
    private const val MAX_DEPTH = 32
    private const val BUFFER = 64 * 1024

    /**
     * The kind of video these first bytes (from a file of [fileLength] bytes)
     * and [mime] name, or null when they name no video. Call only after
     * [PictureData.kindOf] answered null so HEIC, AVIF and CR3 pictures stay
     * pictures.
     */
    fun kindOf(head: ByteArray, fileLength: Long = head.size.toLong(), mime: String? = null): Video? {
        fun at(offset: Int, vararg bytes: Int) = head.size >= offset + bytes.size && bytes.indices.all { head[offset + it].toInt() and 0xFF == bytes[it] }
        fun text(offset: Int, value: String) = at(offset, *value.map { it.code }.toIntArray())
        val isVideoMime = mime?.trim()?.lowercase()?.startsWith("video/") == true
        return when {
            isIsoVideo(head, fileLength) -> Video.Iso
            // Matroska / WebM (EBML).
            at(0, 0x1A, 0x45, 0xDF, 0xA3) -> Video.Other
            // AVI.
            text(0, "RIFF") && (text(8, "AVI ") || text(8, "AVIX")) -> Video.Other
            // Flash Video.
            at(0, 0x46, 0x4C, 0x56, 0x01) -> Video.Other
            // ASF / WMV.
            at(0, 0x30, 0x26, 0xB2, 0x75, 0x8E, 0x66, 0xCF, 0x11) -> Video.Other
            // Ogg container: treat as video when named as video or Ogg video (`theora` / `fishead` / `video/`).
            text(0, "OggS") && (isVideoMime || containsAscii(head, "theora") || containsAscii(head, " daala")) -> Video.Other
            // MPEG program stream (`00 00 01 BA`) or video elementary stream (`00 00 01 B3`).
            at(0, 0x00, 0x00, 0x01, 0xBA) -> Video.Other
            // MPEG transport stream (three 188-byte sync bytes `0x47`).
            head.size >= 377 && head[0] == 0x47.toByte() && head[188] == 0x47.toByte() && head[376] == 0x47.toByte() -> Video.Other
            isVideoMime -> Video.Other
            else -> null
        }
    }

    /** The kind of video [file] is, by its own first bytes and [mime]. */
    fun kindOf(file: File, mime: String? = null): Video? {
        if (PictureData.kindOf(file) != null) return null
        val head = ByteArray(HEAD)
        val read = file.inputStream().use { input ->
            var total = 0
            while (total < head.size) {
                val n = input.read(head, total, head.size - total)
                if (n < 0) break
                total += n
            }
            total
        }
        return kindOf(head.copyOf(read), file.length(), mime)
    }

    private fun isIsoVideo(head: ByteArray, fileLength: Long): Boolean {
        if (head.size < 8 || PictureData.kindOf(head) != null) return false
        val firstType = String(head, 4, 4, Charsets.ISO_8859_1)
        val firstSize = be32(head, 0)
        if (firstType == "ftyp" || firstType == "styp") {
            return firstSize == 1L || (firstSize in 8L..4096L && firstSize <= fileLength)
        }
        if (firstType in CLASSIC_QT_BOXES) {
            val actualSize = when (firstSize) {
                0L -> fileLength
                1L -> if (head.size >= 16) be64(head, 8) else return false
                else -> firstSize
            }
            if (actualSize < 8L || actualSize > fileLength) return false
            if (actualSize == fileLength) return firstType == "moov"
            if (actualSize + 8L <= head.size) {
                val secondType = String(head, actualSize.toInt() + 4, 4, Charsets.ISO_8859_1)
                return secondType in CLASSIC_QT_BOXES || secondType == "ftyp"
            }
        }
        return false
    }

    private fun containsAscii(bytes: ByteArray, needle: String): Boolean {
        val target = needle.toByteArray(Charsets.ISO_8859_1)
        if (bytes.size < target.size) return false
        for (i in 0..bytes.size - target.size) {
            if (target.indices.all { bytes[i + it] == target[it] }) return true
        }
        return false
    }

    /** What [inspect] found in an ISO or QuickTime video, or what [clean] removed from one. */
    data class Found(
        /** Location boxes and QuickTime / iTunes metadata items naming a place. */
        val locationBoxes: Int = 0,
        /** XMP, XML or UUID boxes whose packet names a place. */
        val xmpWithPlace: Int = 0,
        /** Tracks whose `mdia` handler is neither `vide` nor `soun` (timed metadata, subtitles, telemetry). */
        val nonMediaTracks: Int = 0,
        /** The container's boxes did not read cleanly to the end of the file. */
        val broken: Boolean = false,
    ) {
        val clean: Boolean get() = !broken && locationBoxes == 0 && xmpWithPlace == 0 && nonMediaTracks == 0
    }

    private data class Erase(val start: Long, val length: Long)

    private class WalkResult(val erases: List<Erase>, val found: Found)

    /** Inspects [file] without changing it. */
    fun inspect(file: File): Found = walk(file).found

    /**
     * Copies [from] to [to] with every location box and place-naming metadata
     * item overwritten in place as an ISO `free` box of identical byte length.
     * If [from] does not read cleanly as an ISO or QuickTime container, [to] is
     * not written and [Found.broken] is true.
     */
    fun clean(from: File, to: File): Found {
        val plan = walk(from)
        if (plan.found.broken) return plan.found
        val erases = plan.erases.sortedBy { it.start }
        RandomAccessFile(from, "r").use { input ->
            to.outputStream().buffered().use { output ->
                val buffer = ByteArray(BUFFER)
                val zeros = ByteArray(BUFFER)
                var cursor = 0L
                val total = input.length()
                for (erase in erases) {
                    if (erase.start < cursor) continue
                    copyRange(input, output, cursor, erase.start - cursor, buffer)
                    writeFreeBox(output, erase.length, zeros)
                    cursor = erase.start + erase.length
                }
                if (cursor < total) {
                    copyRange(input, output, cursor, total - cursor, buffer)
                }
            }
        }
        return plan.found
    }

    private fun copyRange(input: RandomAccessFile, output: java.io.OutputStream, start: Long, count: Long, buffer: ByteArray) {
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

    private fun writeFreeBox(output: java.io.OutputStream, length: Long, zeros: ByteArray) {
        require(length >= 8L) { "a box is at least 8 bytes: $length" }
        val left = if (length <= 0xFFFFFFFFL) {
            val header = byteArrayOf(
                (length ushr 24).toByte(),
                (length ushr 16).toByte(),
                (length ushr 8).toByte(),
                length.toByte(),
                'f'.code.toByte(),
                'r'.code.toByte(),
                'e'.code.toByte(),
                'e'.code.toByte(),
            )
            output.write(header)
            length - 8L
        } else {
            require(length >= 16L) { "a 64-bit box is at least 16 bytes: $length" }
            val header = ByteArray(16)
            header[3] = 1
            header[4] = 'f'.code.toByte()
            header[5] = 'r'.code.toByte()
            header[6] = 'e'.code.toByte()
            header[7] = 'e'.code.toByte()
            for (i in 0 until 8) header[8 + i] = (length ushr (56 - 8 * i)).toByte()
            output.write(header)
            length - 16L
        }
        var remaining = left
        while (remaining > 0L) {
            val step = minOf(zeros.size.toLong(), remaining).toInt()
            output.write(zeros, 0, step)
            remaining -= step
        }
    }

    private data class BoxHeader(val start: Long, val bodyStart: Long, val end: Long, val typeBytes: ByteArray) {
        val length: Long get() = end - start
        val bodyLength: Long get() = end - bodyStart
        val type: String get() = String(typeBytes, Charsets.ISO_8859_1)
    }

    private fun readBoxHeader(input: RandomAccessFile, at: Long, parentEnd: Long): BoxHeader? {
        if (at + 8L > parentEnd) return null
        input.seek(at)
        val raw = ByteArray(8)
        input.readFully(raw)
        val size32 = be32(raw, 0)
        val typeBytes = raw.copyOfRange(4, 8)
        return when (size32) {
            0L -> BoxHeader(at, at + 8L, parentEnd, typeBytes)
            1L -> {
                if (at + 16L > parentEnd) return null
                val ext = ByteArray(8)
                input.readFully(ext)
                val size64 = be64(ext, 0)
                if (size64 < 16L || at + size64 > parentEnd || at + size64 < at) return null
                BoxHeader(at, at + 16L, at + size64, typeBytes)
            }
            in 8L..0xFFFFFFFFL -> {
                if (at + size32 > parentEnd) return null
                BoxHeader(at, at + 8L, at + size32, typeBytes)
            }
            else -> null
        }
    }

    private fun walk(file: File): WalkResult = try {
        val total = file.length()
        if (total < 8L) {
            WalkResult(emptyList(), Found(broken = true))
        } else {
            RandomAccessFile(file, "r").use { input -> BoxWalker(input).walkAll(total) }
        }
    } catch (_: Exception) {
        WalkResult(emptyList(), Found(broken = true))
    }

    private class BoxWalker(private val input: RandomAccessFile) {
        val erases = ArrayList<Erase>()
        var locationBoxes = 0
        var xmpWithPlace = 0
        var nonMediaTracks = 0
        var broken = false
        var sawTopBox = false

        fun walkAll(total: Long): WalkResult {
            parseContainer(null, 0L, total, 0)
            if (!sawTopBox) broken = true
            return WalkResult(
                erases = if (broken) emptyList() else erases,
                found = Found(
                    locationBoxes = locationBoxes,
                    xmpWithPlace = xmpWithPlace,
                    nonMediaTracks = nonMediaTracks,
                    broken = broken,
                ),
            )
        }

        fun parseMeta(box: BoxHeader, depth: Int) {
            if (depth > MAX_DEPTH) {
                broken = true
                return
            }
            if (box.bodyLength == 0L) return
            val childrenStart = metaChildrenStart(input, box.bodyStart, box.end) ?: run {
                broken = true
                return
            }
            // First pass: collect children and any 1-based QuickTime `keys` table.
            val children = ArrayList<BoxHeader>()
            var cursor = childrenStart
            while (cursor < box.end) {
                val child = readBoxHeader(input, cursor, box.end) ?: run {
                    broken = true
                    return
                }
                children += child
                cursor = child.end
            }
            val locationIndices = HashSet<Int>()
            for (child in children) {
                if (child.type == "keys") {
                    locationIndices += parseQuickTimeLocationKeyIndices(input, child)
                }
            }
            for (child in children) {
                when {
                    child.type in DIRECT_LOCATION_BOXES -> {
                        locationBoxes++
                        erases += Erase(child.start, child.length)
                    }
                    child.type in XML_BOXES -> {
                        if (xmlNamesPlace(input, child)) {
                            xmpWithPlace++
                            erases += Erase(child.start, child.length)
                        }
                    }
                    child.type == "ilst" -> {
                        var itemCursor = child.bodyStart
                        while (itemCursor < child.end) {
                            val item = readBoxHeader(input, itemCursor, child.end) ?: run {
                                broken = true
                                return
                            }
                            val keyIndex = be32(item.typeBytes, 0).toInt()
                            val isLocationItem = item.type in DIRECT_LOCATION_BOXES ||
                                (keyIndex in locationIndices && item.type != "free" && item.type != "skip") ||
                                (item.type == "----" && freeformItemNamesPlace(input, item))
                            if (isLocationItem) {
                                locationBoxes++
                                erases += Erase(item.start, item.length)
                            }
                            itemCursor = item.end
                        }
                    }
                    child.type == "udta" -> parseContainer(child.type, child.bodyStart, child.end, depth + 1)
                    child.type == "meta" -> parseMeta(child, depth + 1)
                }
            }
        }

        fun parseContainer(parentType: String?, from: Long, end: Long, depth: Int) {
            if (depth > MAX_DEPTH) {
                broken = true
                return
            }
            var cursor = from
            while (cursor < end && !broken) {
                // QuickTime `udta` boxes may end with a 4-byte zero terminator.
                if (parentType == "udta" && end - cursor == 4L) {
                    input.seek(cursor)
                    if (input.readInt() == 0) break
                }
                val box = readBoxHeader(input, cursor, end) ?: run {
                    broken = true
                    return
                }
                if (parentType == null) sawTopBox = true
                when {
                    box.type in DIRECT_LOCATION_BOXES -> {
                        locationBoxes++
                        erases += Erase(box.start, box.length)
                    }
                    box.type in XML_BOXES -> {
                        if (xmlNamesPlace(input, box)) {
                            xmpWithPlace++
                            erases += Erase(box.start, box.length)
                        }
                    }
                    parentType == "mdia" && box.type == "hdlr" -> {
                        val handler = readHandlerType(input, box)
                        if (handler == null) {
                            broken = true
                            return
                        } else if (handler != "vide" && handler != "soun") {
                            nonMediaTracks++
                        }
                    }
                    box.type == "meta" -> parseMeta(box, depth + 1)
                    box.type in CONTAINER_BOXES -> parseContainer(box.type, box.bodyStart, box.end, depth + 1)
                }
                cursor = box.end
            }
        }
    }

    /**
     * QuickTime `meta` starts its child boxes immediately after the box header;
     * ISO `meta` is a `FullBox` with 4 bytes of `version` and `flags` first.
     */
    private fun metaChildrenStart(input: RandomAccessFile, bodyStart: Long, end: Long): Long? {
        if (tilesCleanly(input, bodyStart, end, requireAsciiType = true)) return bodyStart
        if (bodyStart + 4L <= end && tilesCleanly(input, bodyStart + 4L, end, requireAsciiType = true)) return bodyStart + 4L
        return null
    }

    private fun tilesCleanly(input: RandomAccessFile, start: Long, end: Long, requireAsciiType: Boolean): Boolean {
        var cursor = start
        while (cursor < end) {
            val box = readBoxHeader(input, cursor, end) ?: return false
            if (requireAsciiType && !box.typeBytes.all { (it.toInt() and 0xFF) in 0x20..0x7E || (it.toInt() and 0xFF) == 0xA9 }) {
                return false
            }
            cursor = box.end
        }
        return cursor == end
    }

    /** Parses a QuickTime `keys` box and returns the 1-based indices whose key names a place. */
    private fun parseQuickTimeLocationKeyIndices(input: RandomAccessFile, keysBox: BoxHeader): Set<Int> {
        if (keysBox.bodyLength < 8L) return emptySet()
        input.seek(keysBox.bodyStart + 4L)
        val count = input.readInt()
        if (count <= 0 || count > 100_000) return emptySet()
        val matched = HashSet<Int>()
        var cursor = keysBox.bodyStart + 8L
        for (index in 1..count) {
            if (cursor + 8L > keysBox.end) break
            input.seek(cursor)
            val keySize = input.readInt().toLong() and 0xFFFFFFFFL
            if (keySize < 8L || cursor + keySize > keysBox.end) break
            input.skipBytes(4) // namespace (`mdta`)
            val nameBytes = ByteArray((keySize - 8L).toInt())
            input.readFully(nameBytes)
            val keyName = String(nameBytes, Charsets.UTF_8)
            if (isLocationKeyName(keyName)) matched += index
            cursor += keySize
        }
        return matched
    }

    /** Checks whether an iTunes `----` freeform item inside `ilst` names a place in its `name` sub-box. */
    private fun freeformItemNamesPlace(input: RandomAccessFile, item: BoxHeader): Boolean {
        var cursor = item.bodyStart
        while (cursor < item.end) {
            val sub = readBoxHeader(input, cursor, item.end) ?: return true
            if (sub.type == "name" && sub.bodyLength > 4L && sub.bodyLength <= 4096L) {
                input.seek(sub.bodyStart + 4L)
                val bytes = ByteArray((sub.bodyLength - 4L).toInt())
                input.readFully(bytes)
                if (isLocationKeyName(String(bytes, Charsets.UTF_8))) return true
            }
            cursor = sub.end
        }
        return false
    }

    internal fun isLocationKeyName(raw: String): Boolean {
        val cleaned = raw.trim().lowercase()
        if (cleaned.isEmpty()) return false
        if (PictureData.isPlace(cleaned)) return true
        val tokens = cleaned.split('.', '/', ':', '_', '-', ' ').filter { it.isNotEmpty() }
        return tokens.any { token ->
            PictureData.isPlace(token) || token == "iso6709" || token == "coordinates" || token == "coord" || token == "coords" || token == "geolocation"
        }
    }

    private fun xmlNamesPlace(input: RandomAccessFile, box: BoxHeader): Boolean {
        val offset = when (box.type) {
            // ISO `xml ` and `bxml` are FullBoxes (4-byte version/flags); `uuid` starts with a 16-byte UUID.
            "xml ", "bxml" -> 4L
            "uuid" -> 16L
            else -> 0L
        }
        val payloadLength = box.bodyLength - offset
        if (payloadLength <= 0L) return false
        if (payloadLength > MOST_XML) return true
        input.seek(box.bodyStart + offset)
        val bytes = ByteArray(payloadLength.toInt())
        input.readFully(bytes)
        return PictureData.namesPlace(bytes)
    }

    private fun readHandlerType(input: RandomAccessFile, hdlr: BoxHeader): String? {
        if (hdlr.bodyLength < 12L) return null
        input.seek(hdlr.bodyStart + 8L)
        val bytes = ByteArray(4)
        input.readFully(bytes)
        return String(bytes, Charsets.ISO_8859_1)
    }

    private fun be32(bytes: ByteArray, offset: Int): Long =
        ((bytes[offset].toLong() and 0xFF) shl 24) or
            ((bytes[offset + 1].toLong() and 0xFF) shl 16) or
            ((bytes[offset + 2].toLong() and 0xFF) shl 8) or
            (bytes[offset + 3].toLong() and 0xFF)

    private fun be64(bytes: ByteArray, offset: Int): Long {
        var result = 0L
        for (i in 0 until 8) {
            result = (result shl 8) or (bytes[offset + i].toLong() and 0xFF)
        }
        return result
    }
}
