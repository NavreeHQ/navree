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
 * Location boxes (`©xyz`, `xyz `, 3GPP `loci`/`LOCI`, `gps `, `GPS `, `GPMF`,
 * Canon `CMT4`/`CNTH` and Canon's metadata `uuid`, Nikon `NCDT`), QuickTime
 * metadata items keyed to a place (`com.apple.quicktime.location.ISO6709` and
 * its companions in `keys` and `ilst`), iTunes freeform place items (`----`),
 * embedded image or Exif items inside `ilst`, ISO binary XML (`bxml`), and XMP
 * or XML boxes (`XMP_`, `xml `, `uuid`) whose elements or naming attributes
 * name a place ([PictureData.namesPlace], Sony `NonRealTimeMeta` `ExifGPS`) are
 * overwritten in place as `free` boxes of the very same byte length, and any
 * existing `free` or `skip` tombstones with non-zero bytes are zeroed in place,
 * leaving every sample and chunk offset (`stco`, `co64`, `tfhd`, `sidx`) intact. ISO
 * `meta` item tables (`iinf`, `iloc`, `idat`, `iref`, `pitm`) and malformed
 * metadata tables mark [Found.broken] so the file is refused rather than sent.
 * Tracks that are neither video nor audio (`mdia` -> `hdlr` other than `vide`
 * or `soun`) are counted in [Found.nonMediaTracks] so [PickedFiles] remuxes
 * video and audio tracks alone before cleaning.
 */
internal object VideoData {
    const val HEAD = 512

    private val CLASSIC_QT_BOXES = setOf("moov", "mdat", "wide", "free", "skip", "pnot")
    private val LEADING_ISO_BOXES = setOf("mdat", "free", "skip", "wide", "pnot", "pdin", "udta", "meta", "uuid", "XMP_")
    private val CONTAINER_BOXES = setOf("moov", "trak", "edts", "mdia", "minf", "dinf", "stbl", "udta", "moof", "traf", "mfra")
    private val DIRECT_LOCATION_BOXES = setOf("\u00A9xyz", "xyz ", "loci", "LOCI", "gps ", "GPS ", "GPMF", "CMT4", "CNTH", "NCDT")
    private val META_ITEM_TABLE_BOXES = setOf("iinf", "iloc", "idat", "iref", "pitm")
    private val NAMING_XML_ATTRS = setOf("name", "key", "property", "id", "tag", "field", "group", "item", "type", "about", "scheme")

    /** Canon's QuickTime/MP4 camera metadata `uuid` (`85c0b687-820f-11e0-8111-f4ce462b6a48`), which holds `CNTH`, `CMT1`..`CMT4` (Exif GPS) and `THMB`. */
    private val CANON_UUID = byteArrayOf(
        0x85.toByte(), 0xC0.toByte(), 0xB6.toByte(), 0x87.toByte(),
        0x82.toByte(), 0x0F, 0x11, 0xE0.toByte(),
        0x81.toByte(), 0x11, 0xF4.toByte(), 0xCE.toByte(),
        0x46, 0x2B, 0x6A, 0x48,
    )
    private val EXIF_HEADER = "Exif\u0000\u0000".toByteArray(Charsets.ISO_8859_1)

    /** Most bytes of an XML or UUID box read to check whether it names a place; larger boxes are treated as naming one. */
    private const val MOST_XML = 16L * 1024 * 1024
    private const val MAX_TOP_BOXES = 64
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
            // Ogg container: treat as video when named as video or Ogg video (`theora` / ` daala`).
            text(0, "OggS") && (isVideoMime || containsAscii(head, "theora") || containsAscii(head, " daala")) -> Video.Other
            // MPEG program stream (`00 00 01 BA`).
            at(0, 0x00, 0x00, 0x01, 0xBA) -> Video.Other
            // MPEG transport stream (188-byte or 192-byte BDAV sync bytes `0x47`).
            (head.size >= 377 && head[0] == 0x47.toByte() && head[188] == 0x47.toByte() && head[376] == 0x47.toByte()) ||
                (head.size >= 389 && head[4] == 0x47.toByte() && head[196] == 0x47.toByte() && head[388] == 0x47.toByte()) -> Video.Other
            isVideoMime -> Video.Other
            else -> null
        }
    }

    /**
     * The kind of video [file] is, by its own bytes and [mime]. When the first
     * [HEAD] bytes begin with a classic QuickTime or padding box (`mdat`,
     * `free`, `skip`, `wide`) longer than [HEAD], walks top-level box headers
     * across the file so a headerless `.mov` or padded `.mp4` is still seen as
     * [Video.Iso] even under a generic media type.
     */
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
        val slice = head.copyOf(read)
        val total = file.length()
        if (isIsoVideoFile(file, slice, total)) return Video.Iso
        return kindOf(slice, total, mime)
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

    private fun isIsoVideoFile(file: File, head: ByteArray, total: Long): Boolean {
        if (head.size < 8 || total < 8L) return false
        if (isIsoVideo(head, total)) return true
        val firstType = String(head, 4, 4, Charsets.ISO_8859_1)
        if (firstType !in LEADING_ISO_BOXES) return false
        return try {
            RandomAccessFile(file, "r").use { input ->
                var cursor = 0L
                var count = 0
                while (cursor < total && count < MAX_TOP_BOXES) {
                    val box = readBoxHeader(input, cursor, total) ?: return false
                    when {
                        box.type == "ftyp" || box.type == "styp" -> return box.length in 8L..4096L
                        box.type == "moov" -> {
                            return box.bodyLength >= 8L && (box.end == total || readBoxHeader(input, box.end, total) != null)
                        }
                        box.type !in LEADING_ISO_BOXES -> return false
                    }
                    cursor = box.end
                    count++
                }
                false
            }
        } catch (_: Exception) {
            false
        }
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
        /** XMP, XML, binary XML or UUID boxes whose packet names a place. */
        val xmpWithPlace: Int = 0,
        /** Tracks whose `mdia` handler is neither `vide` nor `soun` (timed metadata, subtitles, telemetry). */
        val nonMediaTracks: Int = 0,
        /** Existing `free` or `skip` boxes whose payload still holds non-zero bytes (such as tombstoned metadata). */
        val nonZeroFreeBoxes: Int = 0,
        /** The container's boxes did not read cleanly, or held unrewritable item tables. */
        val broken: Boolean = false,
    ) {
        val clean: Boolean get() = !broken && locationBoxes == 0 && xmpWithPlace == 0 && nonMediaTracks == 0 && nonZeroFreeBoxes == 0
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
        var nonZeroFreeBoxes = 0
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
                    nonZeroFreeBoxes = nonZeroFreeBoxes,
                    broken = broken,
                ),
            )
        }

        fun inspectFreeOrSkipBox(box: BoxHeader) {
            if (boxBodyHasNonZeroByte(input, box)) {
                nonZeroFreeBoxes++
                erases += Erase(box.start, box.length)
            }
        }

        fun inspectXmlOrUuidBox(box: BoxHeader) {
            when (box.type) {
                "bxml" -> {
                    // ISO Binary MPEG XML is tokenized binary; overwrite unconditionally.
                    xmpWithPlace++
                    erases += Erase(box.start, box.length)
                }
                "xml " -> {
                    if (box.bodyLength < 4L) {
                        broken = true
                        return
                    }
                    if (payloadNamesPlace(input, box.bodyStart + 4L, box.bodyLength - 4L)) {
                        xmpWithPlace++
                        erases += Erase(box.start, box.length)
                    }
                }
                "XMP_" -> {
                    if (payloadNamesPlace(input, box.bodyStart, box.bodyLength)) {
                        xmpWithPlace++
                        erases += Erase(box.start, box.length)
                    }
                }
                "uuid" -> {
                    if (box.bodyLength < 16L) {
                        broken = true
                        return
                    }
                    input.seek(box.bodyStart)
                    val uuid = ByteArray(16)
                    input.readFully(uuid)
                    if (uuid.contentEquals(CANON_UUID)) {
                        locationBoxes++
                        erases += Erase(box.start, box.length)
                    } else if (payloadNamesPlace(input, box.bodyStart + 16L, box.bodyLength - 16L)) {
                        xmpWithPlace++
                        erases += Erase(box.start, box.length)
                    }
                }
            }
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
                if (child.type in META_ITEM_TABLE_BOXES) {
                    // Item-addressed ISO `meta` tables (`iinf`/`iloc`/`idat`) can hold binary `Exif` GPS extents in `mdat`; fail closed.
                    broken = true
                    return
                }
                if (child.type == "keys") {
                    val parsed = parseQuickTimeLocationKeyIndices(input, child)
                    if (parsed == null) {
                        broken = true
                        return
                    }
                    locationIndices += parsed
                }
            }
            for (child in children) {
                if (broken) return
                when {
                    child.type == "free" || child.type == "skip" -> inspectFreeOrSkipBox(child)
                    child.type in DIRECT_LOCATION_BOXES -> {
                        locationBoxes++
                        erases += Erase(child.start, child.length)
                    }
                    child.type == "bxml" || child.type == "xml " || child.type == "XMP_" || child.type == "uuid" -> inspectXmlOrUuidBox(child)
                    child.type == "ilst" -> {
                        var itemCursor = child.bodyStart
                        while (itemCursor < child.end && !broken) {
                            val item = readBoxHeader(input, itemCursor, child.end) ?: run {
                                broken = true
                                return
                            }
                            if (item.type == "free" || item.type == "skip") {
                                inspectFreeOrSkipBox(item)
                            } else {
                                val keyIndex = be32(item.typeBytes, 0).toInt()
                                val isLocationItem = item.type in DIRECT_LOCATION_BOXES ||
                                    keyIndex in locationIndices ||
                                    (item.type == "----" && freeformItemNamesPlace(item)) ||
                                    ilstItemHoldsImageOrExif(item)
                                if (broken) return
                                if (isLocationItem) {
                                    locationBoxes++
                                    erases += Erase(item.start, item.length)
                                } else {
                                    inspectKeptIlstItem(item)
                                }
                            }
                            itemCursor = item.end
                        }
                    }
                    child.type == "udta" -> parseContainer(child.type, child.bodyStart, child.end, depth + 1)
                    child.type == "meta" -> parseMeta(child, depth + 1)
                }
            }
        }

        fun inspectKeptIlstItem(item: BoxHeader) {
            var cursor = item.bodyStart
            while (cursor < item.end && !broken) {
                val sub = readBoxHeader(input, cursor, item.end) ?: return
                if (sub.type == "free" || sub.type == "skip") inspectFreeOrSkipBox(sub)
                cursor = sub.end
            }
        }

        fun freeformItemNamesPlace(item: BoxHeader): Boolean {
            var cursor = item.bodyStart
            var matched = false
            while (cursor < item.end) {
                val sub = readBoxHeader(input, cursor, item.end) ?: run {
                    broken = true
                    return true
                }
                if (sub.type == "mean" || sub.type == "name") {
                    if (sub.bodyLength < 4L) {
                        broken = true
                        return true
                    }
                    val textLength = sub.bodyLength - 4L
                    if (textLength > 4096L) {
                        matched = true
                    } else if (textLength > 0L) {
                        input.seek(sub.bodyStart + 4L)
                        val bytes = ByteArray(textLength.toInt())
                        input.readFully(bytes)
                        if (isLocationKeyName(String(bytes, Charsets.UTF_8))) matched = true
                    }
                } else if (sub.type == "data" && dataSubBoxHoldsImageOrPlace(sub)) {
                    matched = true
                }
                cursor = sub.end
            }
            if (cursor != item.end) {
                broken = true
                return true
            }
            return matched
        }

        fun ilstItemHoldsImageOrExif(item: BoxHeader): Boolean {
            if (item.type == "covr") return true
            var cursor = item.bodyStart
            while (cursor < item.end) {
                val sub = readBoxHeader(input, cursor, item.end) ?: return false
                if (sub.type == "data" && dataSubBoxHoldsImageOrPlace(sub)) return true
                cursor = sub.end
            }
            return false
        }

        fun dataSubBoxHoldsImageOrPlace(sub: BoxHeader): Boolean {
            // QuickTime / iTunes `data` box: 4-byte type indicator + 4-byte locale + value bytes.
            if (sub.bodyLength <= 8L) return false
            input.seek(sub.bodyStart)
            val typeIndicator = input.readInt()
            // Type 13 = JPEG, 14 = PNG, 27 = BMP.
            if (typeIndicator == 13 || typeIndicator == 14 || typeIndicator == 27) return true
            val valueStart = sub.bodyStart + 8L
            val valueLen = sub.bodyLength - 8L
            val headLen = minOf(valueLen, PictureData.HEAD.toLong()).toInt()
            input.seek(valueStart)
            val head = ByteArray(headLen)
            input.readFully(head)
            return PictureData.kindOf(head) != null || looksLikeBinaryExif(head)
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
                    box.type == "free" || box.type == "skip" -> inspectFreeOrSkipBox(box)
                    box.type in DIRECT_LOCATION_BOXES -> {
                        locationBoxes++
                        erases += Erase(box.start, box.length)
                    }
                    box.type == "bxml" || box.type == "xml " || box.type == "XMP_" || box.type == "uuid" -> inspectXmlOrUuidBox(box)
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

    /**
     * Parses a QuickTime `keys` box and returns the 1-based indices whose key
     * names a place, or null when the `keys` box is malformed or truncated so
     * the caller fails closed.
     */
    private fun parseQuickTimeLocationKeyIndices(input: RandomAccessFile, keysBox: BoxHeader): Set<Int>? {
        if (keysBox.bodyLength < 8L) return null
        input.seek(keysBox.bodyStart)
        val versionAndFlags = input.readInt()
        if (((versionAndFlags ushr 24) and 0xFF) != 0) return null
        val count = input.readInt()
        if (count < 0 || count > 100_000) return null
        val matched = HashSet<Int>()
        var cursor = keysBox.bodyStart + 8L
        for (index in 1..count) {
            if (cursor + 8L > keysBox.end) return null
            input.seek(cursor)
            val keySize = input.readInt().toLong() and 0xFFFFFFFFL
            if (keySize < 8L || keySize > 1_048_576L || cursor + keySize > keysBox.end) return null
            input.skipBytes(4) // namespace (`mdta`)
            val nameBytes = ByteArray((keySize - 8L).toInt())
            input.readFully(nameBytes)
            val keyName = String(nameBytes, Charsets.UTF_8)
            if (isLocationKeyName(keyName)) matched += index
            cursor += keySize
        }
        if (cursor != keysBox.end) return null
        return matched
    }

    internal fun isLocationKeyName(raw: String): Boolean {
        val cleaned = raw.trim().lowercase()
        if (cleaned.isEmpty()) return false
        if (PictureData.isPlace(cleaned) || cleaned.endsWith("gps")) return true
        val tokens = cleaned.split('.', '/', ':', '_', '-', ' ').filter { it.isNotEmpty() }
        return tokens.any { token ->
            PictureData.isPlace(token) ||
                token.endsWith("gps") ||
                token == "iso6709" ||
                token == "coordinates" ||
                token == "coord" ||
                token == "coords" ||
                token == "geolocation"
        }
    }

    private fun payloadNamesPlace(input: RandomAccessFile, start: Long, length: Long): Boolean {
        if (length <= 0L) return false
        if (length > MOST_XML) return true
        input.seek(start)
        val bytes = ByteArray(length.toInt())
        input.readFully(bytes)
        return looksLikeBinaryExif(bytes) || PictureData.namesPlace(bytes) || xmlAttributesNamePlace(bytes)
    }

    private fun looksLikeBinaryExif(bytes: ByteArray): Boolean {
        if (bytes.size >= EXIF_HEADER.size && EXIF_HEADER.indices.all { bytes[it] == EXIF_HEADER[it] }) return true
        if (bytes.size >= 4) {
            val b0 = bytes[0].toInt() and 0xFF
            val b1 = bytes[1].toInt() and 0xFF
            val b2 = bytes[2].toInt() and 0xFF
            val b3 = bytes[3].toInt() and 0xFF
            if ((b0 == 0x49 && b1 == 0x49 && b2 == 0x2A && b3 == 0x00) || (b0 == 0x4D && b1 == 0x4D && b2 == 0x00 && b3 == 0x2A)) return true
        }
        return false
    }

    /**
     * Scans XML attribute values on naming attributes (`name="ExifGPS"`,
     * `name="Latitude"`, `key="com.apple.quicktime.location.ISO6709"`) so
     * schemas like Sony `NonRealTimeMeta` that put field names in attribute
     * values rather than element names are caught alongside XMP.
     */
    internal fun xmlAttributesNamePlace(raw: ByteArray): Boolean {
        val text = String(raw.filter { it != 0.toByte() }.toByteArray(), Charsets.ISO_8859_1)
        var at = 0
        while (at < text.length) {
            val ch = text[at]
            if (!(ch in 'a'..'z' || ch in 'A'..'Z' || ch == '_')) {
                at++
                continue
            }
            val attrStart = at
            while (at < text.length && (text[at] in 'a'..'z' || text[at] in 'A'..'Z' || text[at] in '0'..'9' || text[at] == '_' || text[at] == '-' || text[at] == '.' || text[at] == ':')) {
                at++
            }
            val attrName = text.substring(attrStart, at).substringAfterLast(':').lowercase()
            while (at < text.length && text[at].isWhitespace()) at++
            if (at >= text.length || text[at] != '=') continue
            at++
            while (at < text.length && text[at].isWhitespace()) at++
            if (at >= text.length || (text[at] != '"' && text[at] != '\'')) continue
            val quote = text[at]
            at++
            val valStart = at
            while (at < text.length && text[at] != quote) at++
            val attrValue = text.substring(valStart, at)
            if (at < text.length) at++
            if (attrName in NAMING_XML_ATTRS && isLocationKeyName(attrValue)) return true
        }
        return false
    }

    private fun boxBodyHasNonZeroByte(input: RandomAccessFile, box: BoxHeader): Boolean {
        var left = box.bodyLength
        if (left <= 0L) return false
        input.seek(box.bodyStart)
        val buf = ByteArray(minOf(BUFFER.toLong(), left).toInt())
        while (left > 0L) {
            val step = minOf(buf.size.toLong(), left).toInt()
            val read = input.read(buf, 0, step)
            if (read <= 0) return true
            for (i in 0 until read) {
                if (buf[i] != 0.toByte()) return true
            }
            left -= read
        }
        return false
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
