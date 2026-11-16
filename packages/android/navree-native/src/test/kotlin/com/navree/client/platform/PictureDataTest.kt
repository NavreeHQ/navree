package com.navree.client.platform

import com.navree.client.design.FileRules.Picture
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.ByteArrayInputStream
import java.io.ByteArrayOutputStream
import java.io.File
import java.util.zip.CRC32
import java.util.zip.Deflater

/**
 * Pictures written byte by byte here, each kind with a place in every part a
 * reader could find it in beside the first Exif block: what kind each is, and
 * what is left of it once cleaned.
 */
class PictureDataTest {
    private fun ascii(text: String) = text.toByteArray(Charsets.ISO_8859_1)

    private fun bytes(vararg values: Int) = ByteArray(values.size) { values[it].toByte() }

    private fun concat(vararg parts: ByteArray): ByteArray = ByteArrayOutputStream().also { out -> parts.forEach { out.write(it) } }.toByteArray()

    private val rdf = "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">"
    private val rdfEnd = "</rdf:RDF></x:xmpmeta>"
    private val xmpWithPlace = "$rdf<rdf:Description xmlns:exif=\"http://ns.adobe.com/exif/1.0/\" exif:GPSLatitude=\"30,54.06N\" exif:GPSLongitude=\"75,51.44E\"/>$rdfEnd"
    private val xmpPlain = "$rdf<rdf:Description xmlns:dc=\"http://purl.org/dc/elements/1.1/\"><dc:creator>Harpreet Kaur</dc:creator></rdf:Description>$rdfEnd"

    private fun file(bytes: ByteArray): File = File.createTempFile("picture", ".bin").apply {
        deleteOnExit()
        writeBytes(bytes)
    }

    private fun cleaned(bytes: ByteArray, kind: Picture): Pair<PictureData.Found, ByteArray> {
        val to = File.createTempFile("cleaned", ".bin").apply { deleteOnExit() }
        val found = PictureData.clean(file(bytes), to, kind)
        return found to to.readBytes()
    }

    // ------------------------------------------------------------------ what a file is

    @Test
    fun a_picture_is_told_by_its_first_bytes() {
        assertEquals(Picture.Jpeg, PictureData.kindOf(bytes(0xFF, 0xD8, 0xFF, 0xE0)))
        assertEquals(Picture.Png, PictureData.kindOf(bytes(0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0, 0)))
        assertEquals(Picture.Webp, PictureData.kindOf(ascii("RIFF\u0000\u0000\u0000\u0000WEBPVP8 ")))
        assertEquals(Picture.Gif, PictureData.kindOf(ascii("GIF89a....")))
        assertEquals(Picture.Raw, PictureData.kindOf(ascii("II*\u0000\u0008\u0000\u0000\u0000")))
        assertEquals("a DNG is a TIFF", Picture.Raw, PictureData.kindOf(ascii("MM\u0000*\u0000\u0000\u0000\u0008")))
        assertEquals(Picture.Raw, PictureData.kindOf(ascii("FUJIFILMCCD-RAW 0201")))
        fun iso(major: String, vararg compatible: String) = concat(bytes(0, 0, 0, 16 + 4 * compatible.size), ascii("ftyp$major\u0000\u0000\u0000\u0000"), *compatible.map { ascii(it) }.toTypedArray())
        assertEquals(Picture.Heic, PictureData.kindOf(iso("heic", "mif1", "heic")))
        assertEquals("HEIF by its compatible brands", Picture.Heic, PictureData.kindOf(iso("mif1", "mif1", "heix")))
        assertEquals(Picture.Avif, PictureData.kindOf(iso("avif", "mif1", "avif")))
        assertEquals("AVIF by its compatible brands", Picture.Avif, PictureData.kindOf(iso("mif1", "mif1", "avif", "miaf")))
        assertEquals("a Canon CR3", Picture.Raw, PictureData.kindOf(iso("crx ", "crx ", "isom")))
        assertNull("an MP4 video is not a picture", PictureData.kindOf(iso("isom", "isom", "mp42")))
        assertNull("a PDF", PictureData.kindOf(ascii("%PDF-1.7")))
        assertNull("nothing", PictureData.kindOf(ByteArray(0)))
    }

    @Test
    fun an_xmp_packet_names_a_place_by_a_gps_property_or_a_place_name() {
        assertTrue(PictureData.namesPlace(ascii(xmpWithPlace)))
        assertTrue("the place a picture points to", PictureData.namesPlace(ascii("<rdf:Description exif:GPSDestLatitude=\"30,54N\"/>")))
        assertTrue(PictureData.namesPlace(ascii("<photoshop:City>Ludhiana</photoshop:City>")))
        assertTrue(PictureData.namesPlace(ascii("<Iptc4xmpExt:LocationShown>")))
        assertTrue(PictureData.namesPlace(ascii("<photoshop:State>Punjab</photoshop:State>")))
        assertFalse(PictureData.namesPlace(ascii(xmpPlain)))
    }

    // ------------------------------------------------------------------ JPEG

    private fun segment(marker: Int, payload: ByteArray) = concat(bytes(0xFF, marker, (payload.size + 2) shr 8, (payload.size + 2) and 0xFF), payload)

    private val soi = bytes(0xFF, 0xD8)
    private val eoi = bytes(0xFF, 0xD9)
    private val exif = segment(0xE1, concat(ascii("Exif\u0000\u0000"), ascii("MM\u0000*\u0000\u0000\u0000\u0008the first")))
    private val dqt = segment(0xDB, ByteArray(65))
    private val sos = segment(0xDA, bytes(1, 1, 0, 0, 63, 0))

    // Entropy-coded data with a stuffed 0xFF, a restart marker and fill bytes.
    private val scan = bytes(0x12, 0xFF, 0x00, 0x34, 0xFF, 0xD0, 0x56, 0xFF, 0x00, 0x78)

    private fun xmpSegment(xmp: String) = segment(0xE1, concat(ascii("http://ns.adobe.com/xap/1.0/\u0000"), ascii(xmp)))

    @Test
    fun a_jpeg_keeps_its_first_exif_block_and_its_picture_and_nothing_else_that_can_name_a_place() {
        val picture = concat(
            soi,
            exif,
            xmpSegment(xmpWithPlace),
            segment(0xE1, concat(ascii("Exif\u0000\u0000"), ascii("MM\u0000*a second, which readers of the first do not see"))),
            segment(0xED, concat(ascii("Photoshop 3.0\u0000"), ascii("8BIM\u0004\u0004Moga"))),
            segment(0xE2, concat(ascii("MPF\u0000"), ascii("MM\u0000*more pictures"))),
            dqt,
            sos,
            scan,
            eoi,
            // A motion photo's video, or a second picture, after the end.
            ascii("\u0000\u0000\u0000\u0018ftypmp42 with its own place"),
        )
        val before = PictureData.inspect(file(picture), Picture.Jpeg)
        assertEquals(PictureData.Found(exifBlocks = 2, xmpWithPlace = 1, otherMetadata = 2, trailing = 31), before)
        assertFalse(before.clean)
        val (found, after) = cleaned(picture, Picture.Jpeg)
        assertEquals(before, found)
        assertArrayEquals("the picture, its first Exif block and its tables, and nothing else", concat(soi, exif, dqt, sos, scan, eoi), after)
        assertTrue(PictureData.inspect(file(after), Picture.Jpeg).clean)
    }

    @Test
    fun a_jpeg_keeps_an_xmp_packet_that_names_no_place() {
        val picture = concat(soi, exif, xmpSegment(xmpPlain), dqt, sos, scan, eoi)
        val (found, after) = cleaned(picture, Picture.Jpeg)
        assertTrue(found.clean)
        assertArrayEquals(picture, after)
    }

    @Test
    fun a_jpegs_extended_xmp_that_names_a_place_goes_even_when_the_name_is_split_between_its_parts() {
        val guid = ascii("0123456789ABCDEF0123456789ABCDEF")
        val whole = ascii("<rdf:Description exif:GPSLongitude=\"75,51.44E\"/>")
        fun part(offset: Int, data: ByteArray) = segment(0xE1, concat(ascii("http://ns.adobe.com/xmp/extension/\u0000"), guid, bytes(0, 0, 0, whole.size), bytes(0, 0, 0, offset), data))
        // "GPSLongitude" split as "G" and "PSLongitude": neither part names a place alone.
        val cut = String(whole, Charsets.ISO_8859_1).indexOf("PSLongitude")
        assertFalse(PictureData.namesPlace(whole.copyOfRange(0, cut)) || PictureData.namesPlace(whole.copyOfRange(cut, whole.size)))
        val picture = concat(soi, exif, xmpSegment(xmpPlain), part(0, whole.copyOfRange(0, cut)), part(cut, whole.copyOfRange(cut, whole.size)), dqt, sos, scan, eoi)
        val (found, after) = cleaned(picture, Picture.Jpeg)
        assertEquals(1, found.xmpWithPlace)
        assertArrayEquals("the main packet names no place and stays; its extended parts go", concat(soi, exif, xmpSegment(xmpPlain), dqt, sos, scan, eoi), after)
    }

    @Test
    fun a_jpeg_that_does_not_end_is_not_cleaned() {
        val (found, _) = cleaned(concat(soi, exif, dqt, sos, scan), Picture.Jpeg)
        assertTrue(found.broken)
        assertFalse(found.clean)
    }

    // ------------------------------------------------------------------ PNG

    private fun chunk(type: String, data: ByteArray): ByteArray {
        val crc = CRC32().apply { update(ascii(type)); update(data) }.value
        return concat(bytes(data.size ushr 24, (data.size shr 16) and 0xFF, (data.size shr 8) and 0xFF, data.size and 0xFF), ascii(type), data,
            bytes((crc shr 24).toInt() and 0xFF, (crc shr 16).toInt() and 0xFF, (crc shr 8).toInt() and 0xFF, crc.toInt() and 0xFF))
    }

    private fun deflate(data: ByteArray): ByteArray {
        val deflater = Deflater().apply {
            setInput(data)
            finish()
        }
        val out = ByteArrayOutputStream()
        val buffer = ByteArray(1024)
        while (!deflater.finished()) out.write(buffer, 0, deflater.deflate(buffer))
        return out.toByteArray()
    }

    private val signature = bytes(0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A)
    private val ihdr = chunk("IHDR", bytes(0, 0, 0, 1, 0, 0, 0, 1, 8, 2, 0, 0, 0))
    private val pngExif = chunk("eXIf", ascii("MM\u0000*\u0000\u0000\u0000\u0008the first"))
    private val idat = chunk("IDAT", deflate(bytes(0, 255, 0, 0)))
    private val iend = chunk("IEND", ByteArray(0))
    private val comment = chunk("tEXt", ascii("Comment\u0000Harpreet's shop, Moga"))

    @Test
    fun a_png_keeps_its_first_exif_chunk_and_its_picture_and_nothing_else_that_can_name_a_place() {
        val picture = concat(
            signature,
            ihdr,
            pngExif,
            chunk("eXIf", ascii("MM\u0000*a second")),
            // XMP, compressed, as iTXt may hold it: keyword, 0, compressed, method, language, 0, translated, 0, text.
            chunk("iTXt", concat(ascii("XML:com.adobe.xmp\u0000"), bytes(1, 0), ascii("\u0000\u0000"), deflate(ascii(xmpWithPlace)))),
            chunk("tEXt", ascii("Raw profile type exif\u0000\nexif\n 20\n4d4d002a")),
            comment,
            idat,
            iend,
            ascii("after the end"),
        )
        val before = PictureData.inspect(file(picture), Picture.Png)
        assertEquals(PictureData.Found(exifBlocks = 2, xmpWithPlace = 1, otherMetadata = 1, trailing = 13), before)
        val (_, after) = cleaned(picture, Picture.Png)
        assertArrayEquals(concat(signature, ihdr, pngExif, comment, idat, iend), after)
        assertTrue(PictureData.inspect(file(after), Picture.Png).clean)
    }

    @Test
    fun a_pngs_xmp_that_names_no_place_stays_whether_it_is_compressed_or_not() {
        val picture = concat(
            signature,
            ihdr,
            chunk("iTXt", concat(ascii("XML:com.adobe.xmp\u0000"), bytes(1, 0), ascii("\u0000\u0000"), deflate(ascii(xmpPlain)))),
            chunk("iTXt", concat(ascii("XML:com.adobe.xmp\u0000"), bytes(0, 0), ascii("\u0000\u0000"), ascii(xmpPlain))),
            idat,
            iend,
        )
        val (found, after) = cleaned(picture, Picture.Png)
        assertTrue(found.clean)
        assertArrayEquals(picture, after)
    }

    // ------------------------------------------------------------------ WebP

    private fun riffChunk(type: String, data: ByteArray): ByteArray {
        val size = data.size
        return concat(ascii(type), bytes(size and 0xFF, (size shr 8) and 0xFF, (size shr 16) and 0xFF, size ushr 24), data, if (size % 2 == 1) ByteArray(1) else ByteArray(0))
    }

    private fun riff(vararg chunks: ByteArray): ByteArray {
        val body = concat(ascii("WEBP"), *chunks)
        return concat(ascii("RIFF"), bytes(body.size and 0xFF, (body.size shr 8) and 0xFF, (body.size shr 16) and 0xFF, body.size ushr 24), body)
    }

    private fun vp8x(flags: Int) = riffChunk("VP8X", bytes(flags, 0, 0, 0, 0, 0, 0, 0, 0, 0))

    @Test
    fun a_webp_keeps_its_first_exif_chunk_and_its_picture_and_says_it_has_no_xmp_once_that_is_gone() {
        val picture = riff(
            vp8x(0x08 or 0x04),
            riffChunk("VP8L", bytes(0x2F, 1, 2, 3, 4)),
            riffChunk("EXIF", ascii("MM\u0000*the first")),
            riffChunk("EXIF", ascii("MM\u0000*a second")),
            riffChunk("XMP ", ascii(xmpWithPlace)),
        )
        val withTrailing = concat(picture, ascii("after the end"))
        val before = PictureData.inspect(file(withTrailing), Picture.Webp)
        assertEquals(PictureData.Found(exifBlocks = 2, xmpWithPlace = 1, trailing = 13), before)
        val (_, after) = cleaned(withTrailing, Picture.Webp)
        val expected = riff(vp8x(0x08), riffChunk("VP8L", bytes(0x2F, 1, 2, 3, 4)), riffChunk("EXIF", ascii("MM\u0000*the first")))
        assertArrayEquals("the size in its header is the size of what is left, and the XMP flag is cleared", expected, after)
        assertTrue(PictureData.inspect(file(after), Picture.Webp).clean)
    }

    // ------------------------------------------------------------------ a kind the client does not rewrite

    @Test
    fun an_xmp_packet_that_names_a_place_is_found_in_the_bytes_of_any_file() {
        val heic = concat(bytes(0, 0, 0, 24), ascii("ftypheic\u0000\u0000\u0000\u0000mif1heic"), ByteArray(5000) { (it * 7).toByte() })
        assertTrue(PictureData.packetNamesPlace(ByteArrayInputStream(concat(heic, ascii(xmpWithPlace), ByteArray(300)))))
        assertFalse(PictureData.packetNamesPlace(ByteArrayInputStream(concat(heic, ascii(xmpPlain), ByteArray(300)))))
        assertFalse("no packet at all", PictureData.packetNamesPlace(ByteArrayInputStream(heic)))
        assertTrue("a packet the file ends inside of is read as far as it goes", PictureData.packetNamesPlace(ByteArrayInputStream(concat(heic, ascii(xmpWithPlace.substringBefore("</rdf:RDF>"))))))
        // A packet across the reads of a long file.
        val far = concat(ByteArray(64 * 1024 - 20), ascii(xmpWithPlace))
        assertTrue(PictureData.packetNamesPlace(ByteArrayInputStream(far)))
    }
}
