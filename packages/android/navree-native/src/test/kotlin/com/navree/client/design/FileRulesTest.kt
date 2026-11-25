package com.navree.client.design

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class FileRulesTest {
    private val types = mapOf("pdf" to "application/pdf", "csv" to "text/csv")

    private fun source(accept: String?) = FileRules.source(accept)

    @Test
    fun a_field_that_takes_only_pictures_and_video_opens_the_photo_picker_and_any_other_the_files() {
        assertEquals(FileRules.Source.Photos, source("image/*"))
        assertEquals(FileRules.Source.Photos, source("image/png, image/jpeg"))
        assertEquals(FileRules.Source.Photos, source("video/*,image/*"))
        assertEquals(FileRules.Source.Files, source("image/*,.pdf"))
        assertEquals(FileRules.Source.Files, source(".png"))
        assertEquals(FileRules.Source.Files, source("application/pdf"))
        assertEquals("a field that names no kind takes any file", FileRules.Source.Files, source(null))
        assertEquals(FileRules.Source.Files, source(" , "))
    }

    @Test
    fun the_photo_picker_is_asked_for_exactly_what_the_field_takes() {
        assertEquals(FileRules.Media.ImageOnly, FileRules.media("image/*"))
        assertEquals(FileRules.Media.ImageOnly, FileRules.media("image/png,image/jpeg"))
        assertEquals(FileRules.Media.VideoOnly, FileRules.media("video/*"))
        assertEquals(FileRules.Media.ImageAndVideo, FileRules.media("image/*,video/*"))
        assertEquals(FileRules.Media.Single("image/gif"), FileRules.media("IMAGE/GIF"))
        assertEquals("the same kind twice is one kind", FileRules.Media.Single("image/gif"), FileRules.media("image/gif, image/gif"))
    }

    @Test
    fun the_file_picker_is_limited_to_the_kinds_named_and_shows_every_file_when_a_kind_has_no_type() {
        val of = { extension: String -> types[extension] }
        assertEquals(listOf("image/*", "application/pdf"), FileRules.documentTypes("image/*,.pdf", of))
        assertEquals(listOf("text/csv"), FileRules.documentTypes(".csv, text/csv", of))
        assertEquals(listOf("*/*"), FileRules.documentTypes(null, of))
        assertEquals("an extension the system has no type for hides nothing", listOf("*/*"), FileRules.documentTypes("image/*,.xyz", of))
    }

    @Test
    fun the_plan_for_the_protocols_papers_field_opens_the_file_picker_for_several_files() {
        val plan = FileRules.plan("image/*,.pdf", multiple = true) { types[it] }
        assertEquals(FileRules.Source.Files, plan.source)
        assertTrue(plan.multiple)
        assertEquals(listOf("image/*", "application/pdf"), plan.documentTypes)
    }

    @Test
    fun a_file_is_taken_when_its_extension_or_its_type_is_on_the_list() {
        val accept = "image/*, .PDF, application/vnd.ms-excel"
        assertTrue(FileRules.accepts(accept, "Receipt.pdf", "application/pdf"))
        assertTrue("an extension is matched by the name alone", FileRules.accepts(accept, "scan.PDF", null))
        assertTrue(FileRules.accepts(accept, "photo", "image/webp"))
        assertTrue(FileRules.accepts(accept, "sheet", "application/vnd.ms-excel"))
        assertTrue("a type with a parameter is read without it", FileRules.accepts(accept, "sheet", "application/vnd.ms-excel; charset=binary"))
        assertFalse(FileRules.accepts(accept, "notes.txt", "text/plain"))
        assertFalse("a type is no match for a longer one", FileRules.accepts("image/png", "x", "image/pngx"))
        assertFalse("with no type a kind named by type is no match", FileRules.accepts("image/*", "photo.jpg", null))
        assertTrue(FileRules.accepts(null, "anything.bin", null))
        assertTrue(FileRules.accepts("*/*", "anything.bin", null))
    }

    @Test
    fun a_file_that_is_too_large_or_of_another_kind_is_refused_with_what_the_field_takes() {
        assertNull(FileRules.refusal("image/*,.pdf", "bill.pdf", "application/pdf", 1_200_000))
        assertEquals(FileRules.Refusal.TooLarge("film.mp4", FileRules.MOST_BYTES), FileRules.refusal(null, "film.mp4", "video/mp4", FileRules.MOST_BYTES + 1))
        assertNull("a file exactly the most is taken", FileRules.refusal(null, "film.mp4", "video/mp4", FileRules.MOST_BYTES))
        assertNull("a size that is not known is checked as it is copied", FileRules.refusal(null, "film.mp4", "video/mp4", null))
        assertEquals(FileRules.Refusal.WrongType("notes.txt", "image/*, .pdf"), FileRules.refusal(" Image/* , .pdf", "notes.txt", "text/plain", 10))
        assertEquals("too large is said before the wrong kind", FileRules.Refusal.TooLarge("notes.txt", 5), FileRules.refusal("image/*", "notes.txt", "text/plain", 6, most = 5))
    }

    @Test
    fun the_files_of_one_field_obey_the_most_together() {
        assertNull(FileRules.refusal(null, "b.pdf", "application/pdf", 60, most = 100, held = 40))
        assertEquals(FileRules.Refusal.TooLargeTogether("b.pdf", 100), FileRules.refusal(null, "b.pdf", "application/pdf", 61, most = 100, held = 40))
        assertEquals("a file that is too large alone is said so, whatever is held", FileRules.Refusal.TooLarge("b.pdf", 100), FileRules.refusal(null, "b.pdf", "application/pdf", 101, most = 100, held = 40))
    }

    @Test
    fun a_name_is_never_a_path_and_is_never_empty() {
        assertEquals("Receipt Ludhiana.pdf", FileRules.displayName("Receipt Ludhiana.pdf"))
        assertEquals("passwd", FileRules.displayName("../../etc/passwd"))
        assertEquals("a.pdf", FileRules.displayName("C:\\papers\\a.pdf"))
        assertEquals("ab.pdf", FileRules.displayName("a\u0000b\n.pdf"))
        assertEquals("file", FileRules.displayName(null))
        assertEquals("file", FileRules.displayName("  "))
        assertEquals("file", FileRules.displayName("folder/"))
        assertEquals("keeps letters of any script", "ਰਸੀਦ.pdf", FileRules.displayName("ਰਸੀਦ.pdf"))
        assertEquals("a format character outside the first plane goes too", "ab.pdf", FileRules.displayName("a\uDB40\uDC41b.pdf"))
        assertEquals("half of a pair that stands alone is no character", "ab.pdf", FileRules.displayName("a\uD83Db.pdf"))
        assertEquals("a character outside the first plane that is none of those stays", "\uD83D\uDCC4.pdf", FileRules.displayName("\uD83D\uDCC4.pdf"))
        val cut = FileRules.displayName("\u0A30".repeat(5_000) + "\uD83D\uDCC4".repeat(5_000) + ".pdf")
        assertEquals("a name is no longer than this", FileRules.MOST_NAME, cut.codePointCount(0, cut.length))
        assertTrue("and keeps its ending: $cut", cut.endsWith(".pdf"))
        assertEquals("\u0A30".repeat(FileRules.MOST_NAME), FileRules.displayName("\u0A30".repeat(FileRules.MOST_NAME)))
        assertEquals("no character that turns the text around", "Bill Mogafdp.exe", FileRules.displayName("Bill Moga\u202Efdp.exe"))
        assertEquals("nor one that hides in it", "Rasid.pdf", FileRules.displayName("Ra\u200Bsid\u2066.pdf"))
    }

    @Test
    fun the_place_in_a_picture_is_taken_out_of_the_kinds_the_platform_writes_and_checked_or_refused_in_the_others_by_api_level() {
        assertEquals(FileRules.Place.None, FileRules.place(null, 35))
        for (kind in listOf(FileRules.Picture.Jpeg, FileRules.Picture.Png, FileRules.Picture.Webp)) {
            assertEquals(kind.name, FileRules.Place.TakeOut, FileRules.place(kind, 26))
        }
        assertEquals("a GIF has no Exif data: its XMP is read", FileRules.Place.Check, FileRules.place(FileRules.Picture.Gif, 26))
        assertEquals("HEIC is read from API 28", FileRules.Place.Refuse, FileRules.place(FileRules.Picture.Heic, 27))
        assertEquals(FileRules.Place.Check, FileRules.place(FileRules.Picture.Heic, 28))
        assertEquals("AVIF is read from API 31", FileRules.Place.Refuse, FileRules.place(FileRules.Picture.Avif, 30))
        assertEquals(FileRules.Place.Check, FileRules.place(FileRules.Picture.Avif, 31))
        assertEquals("TIFF and raw pictures cannot be rewritten", FileRules.Place.Refuse, FileRules.place(FileRules.Picture.Raw, 36))
    }

    @Test
    fun a_token_the_client_made_can_never_name_a_path() {
        assertTrue(FileRules.isToken("0123456789abcdef0123456789abcdef"))
        assertFalse(FileRules.isToken("0123456789ABCDEF0123456789ABCDEF"))
        assertFalse(FileRules.isToken("../../0123456789abcdef0123456789"))
        assertFalse(FileRules.isToken(""))
        assertFalse(FileRules.isToken("0123456789abcdef0123456789abcde"))
    }

    @Test
    fun the_place_in_an_iso_video_is_taken_out_and_other_video_containers_are_refused() {
        assertEquals(FileRules.Place.None, FileRules.videoPlace(null))
        assertEquals(FileRules.Place.TakeOut, FileRules.videoPlace(FileRules.Video.Iso))
        assertEquals(FileRules.Place.Refuse, FileRules.videoPlace(FileRules.Video.Other))
    }

    @Test
    fun a_matroska_or_webm_file_that_holds_sound_alone_is_taken_with_what_names_a_place_taken_out() {
        assertEquals(FileRules.Place.TakeOut, FileRules.videoPlace(FileRules.Video.Sound))
    }

    @Test
    fun the_web_views_accept_array_is_read_as_a_fields_accept_list() {
        assertNull(FileRules.chooserAccept(null))
        assertNull(FileRules.chooserAccept(emptyArray()))
        assertNull(FileRules.chooserAccept(arrayOf("", "  ")))
        assertEquals("image/*,.pdf", FileRules.chooserAccept(arrayOf("Image/*", " .PDF , image/* ")))
    }
}
