package com.navree.client.platform

import com.navree.client.design.FileRules.Video
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.ByteArrayOutputStream
import java.io.File
import java.nio.file.Files

class VideoDataTest {
    private val folder = Files.createTempDirectory("navree-video-test").toFile()

    @After
    fun clear() {
        folder.deleteRecursively()
    }

    private fun box(type: String, vararg parts: ByteArray): ByteArray {
        val body = ByteArrayOutputStream().apply { parts.forEach(::write) }.toByteArray()
        val size = 8 + body.size
        val header = byteArrayOf(
            (size ushr 24).toByte(),
            (size ushr 16).toByte(),
            (size ushr 8).toByte(),
            size.toByte(),
        ) + type.toByteArray(Charsets.ISO_8859_1)
        return header + body
    }

    private fun indexBox(index: Int, vararg parts: ByteArray): ByteArray {
        val body = ByteArrayOutputStream().apply { parts.forEach(::write) }.toByteArray()
        val size = 8 + body.size
        val header = byteArrayOf(
            (size ushr 24).toByte(),
            (size ushr 16).toByte(),
            (size ushr 8).toByte(),
            size.toByte(),
            (index ushr 24).toByte(),
            (index ushr 16).toByte(),
            (index ushr 8).toByte(),
            index.toByte(),
        )
        return header + body
    }

    private fun be32(value: Int): ByteArray = byteArrayOf(
        (value ushr 24).toByte(),
        (value ushr 16).toByte(),
        (value ushr 8).toByte(),
        value.toByte(),
    )

    private fun hdlr(handler: String, componentType: String = "\u0000\u0000\u0000\u0000"): ByteArray =
        box("hdlr", ByteArray(4), componentType.toByteArray(Charsets.ISO_8859_1), handler.toByteArray(Charsets.ISO_8859_1), ByteArray(12))

    @Test
    fun an_iso_or_quicktime_video_is_told_from_other_videos_and_pictures() {
        val mp4 = box("ftyp", "isom\u0000\u0000\u0002\u0000isommp42".toByteArray(Charsets.ISO_8859_1)) + box("mdat", "samples".toByteArray())
        assertEquals(Video.Iso, VideoData.kindOf(mp4, mp4.size.toLong(), "video/mp4"))
        val mov = box("moov", box("mvhd", ByteArray(16))) + box("mdat", "frames".toByteArray())
        assertEquals(Video.Iso, VideoData.kindOf(mov, mov.size.toLong(), "video/quicktime"))

        val webm = byteArrayOf(0x1A, 0x45, 0xDF.toByte(), 0xA3.toByte(), 1, 2, 3, 4)
        assertEquals(Video.Other, VideoData.kindOf(webm, webm.size.toLong(), "video/webm"))
        val avi = "RIFF\u0000\u0000\u0000\u0000AVI LIST".toByteArray(Charsets.ISO_8859_1)
        assertEquals(Video.Other, VideoData.kindOf(avi, avi.size.toLong(), "video/x-msvideo"))
        assertEquals("an unknown container named as video is still refused", Video.Other, VideoData.kindOf("unknown".toByteArray(), 7, "video/x-matroska"))

        val heic = box("ftyp", "heic\u0000\u0000\u0000\u0000mif1heic".toByteArray(Charsets.ISO_8859_1))
        assertNull("HEIC stays a picture", VideoData.kindOf(heic, heic.size.toLong(), "image/heic"))
        assertNull("a PDF is not a video", VideoData.kindOf("%PDF-1.7\n".toByteArray(), 9, "application/pdf"))
    }

    @Test
    fun location_boxes_and_quicktime_location_items_and_xmp_with_a_place_are_overwritten_as_free_without_moving_sample_offsets() {
        val ftyp = box("ftyp", "mp42\u0000\u0000\u0000\u0000isommp42".toByteArray(Charsets.ISO_8859_1))
        val xyz = box("\u00A9xyz", "\u0000\u0012\u0015\u00C7+30.9010+075.8573/".toByteArray(Charsets.ISO_8859_1))
        val loci = box("loci", "\u0000\u0000\u0000\u0000\u0015\u00C7Amritsar\u0000".toByteArray(Charsets.ISO_8859_1))
        // QuickTime `udta` ending with a 4-byte zero terminator.
        val udta = box("udta", xyz, loci, ByteArray(4))

        val keyLocation = "com.apple.quicktime.location.ISO6709".toByteArray(Charsets.UTF_8)
        val keyMake = "com.apple.quicktime.make".toByteArray(Charsets.UTF_8)
        val keys = box(
            "keys",
            ByteArray(4),
            be32(2),
            be32(8 + keyLocation.size), "mdta".toByteArray(Charsets.ISO_8859_1), keyLocation,
            be32(8 + keyMake.size), "mdta".toByteArray(Charsets.ISO_8859_1), keyMake,
        )
        val ilstQt = box(
            "ilst",
            indexBox(1, box("data", be32(1), ByteArray(4), "+31.6340+074.8723/".toByteArray(Charsets.UTF_8))),
            indexBox(2, box("data", be32(1), ByteArray(4), "Charkha Camera".toByteArray(Charsets.UTF_8))),
        )
        val qtMeta = box("meta", hdlr("mdta", "mhlr"), keys, ilstQt)

        // ISO `FullBox` `meta` holding an iTunes `----` freeform place item.
        val freeform = box(
            "----",
            box("mean", ByteArray(4), "com.apple.iTunes".toByteArray(Charsets.UTF_8)),
            box("name", ByteArray(4), "location".toByteArray(Charsets.UTF_8)),
            box("data", be32(1), ByteArray(4), "Ludhiana".toByteArray(Charsets.UTF_8)),
        )
        val isoMeta = box("meta", ByteArray(4), hdlr("mdir"), box("ilst", freeform))

        val videTrak = box("trak", box("mdia", hdlr("vide"), box("minf", box("dinf", hdlr("alis", "dhlr")))))
        val moov = box("moov", videTrak, udta, qtMeta, isoMeta)

        val xmpPlace = """<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#" xmlns:exif="http://ns.adobe.com/exif/1.0/"><rdf:Description exif:GPSLatitude="30,54.06N"/></rdf:RDF></x:xmpmeta>""".toByteArray(Charsets.UTF_8)
        val xmpTitle = """<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#" xmlns:dc="http://purl.org/dc/elements/1.1/"><rdf:Description dc:title="Charkha Loom"/></rdf:RDF></x:xmpmeta>""".toByteArray(Charsets.UTF_8)
        val uuidPlace = box("uuid", ByteArray(16) { 0xBE.toByte() }, xmpPlace)
        val uuidTitle = box("uuid", ByteArray(16) { 0x11 }, xmpTitle)
        val mdatPayload = "SAMPLE-BYTES-AT-FIXED-OFFSET".toByteArray(Charsets.ISO_8859_1)
        val mdat = box("mdat", mdatPayload)

        val originalBytes = ftyp + moov + uuidPlace + uuidTitle + mdat
        val mdatOffset = ftyp.size + moov.size + uuidPlace.size + uuidTitle.size
        val source = File(folder, "source.mp4").apply { writeBytes(originalBytes) }
        val cleaned = File(folder, "cleaned.mp4")

        val before = VideoData.inspect(source)
        assertFalse(before.broken)
        assertEquals(4, before.locationBoxes)
        assertEquals(1, before.xmpWithPlace)
        assertEquals(0, before.nonMediaTracks)
        assertFalse(before.clean)

        val removed = VideoData.clean(source, cleaned)
        assertEquals(before, removed)
        val cleanedBytes = cleaned.readBytes()
        assertEquals("in-place free boxes preserve exact file size", originalBytes.size, cleanedBytes.size)
        assertEquals(
            "mdat sample offset is unchanged",
            String(mdat, Charsets.ISO_8859_1),
            String(cleanedBytes, mdatOffset, mdat.size, Charsets.ISO_8859_1),
        )
        val asLatin = String(cleanedBytes, Charsets.ISO_8859_1)
        assertFalse(asLatin.contains("30.9010"))
        assertFalse(asLatin.contains("31.6340"))
        assertFalse(asLatin.contains("Amritsar"))
        assertFalse(asLatin.contains("Ludhiana"))
        assertFalse(asLatin.contains("30,54.06N"))
        assertTrue("non-location QuickTime metadata stays", asLatin.contains("Charkha Camera"))
        assertTrue("non-location XMP title stays", asLatin.contains("Charkha Loom"))

        val after = VideoData.inspect(cleaned)
        assertTrue("cleaned file passes re-inspection: $after", after.clean)
        assertFalse(cleaned.inputStream().use { PictureData.packetNamesPlace(it) })
    }

    @Test
    fun a_track_whose_handler_is_neither_video_nor_audio_is_counted_and_a_truncated_box_is_broken() {
        val ftyp = box("ftyp", "isom\u0000\u0000\u0002\u0000isommp42".toByteArray(Charsets.ISO_8859_1))
        val telemetry = ftyp + box(
            "moov",
            box("trak", box("mdia", hdlr("vide"))),
            box("trak", box("mdia", hdlr("soun"))),
            box("trak", box("mdia", hdlr("gpmd"))),
        )
        val telemetryFile = File(folder, "telemetry.mp4").apply { writeBytes(telemetry) }
        val found = VideoData.inspect(telemetryFile)
        assertFalse(found.broken)
        assertEquals(1, found.nonMediaTracks)
        assertFalse(found.clean)

        val truncated = File(folder, "truncated.mp4").apply { writeBytes(ftyp + byteArrayOf(0, 0, 1, 0, 'm'.code.toByte(), 'o'.code.toByte(), 'o'.code.toByte(), 'v'.code.toByte())) }
        assertTrue(VideoData.inspect(truncated).broken)
    }
}
