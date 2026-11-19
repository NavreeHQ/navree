package com.navree.client.design

/**
 * What a file field takes and how the system is asked for it. The page says
 * the kinds of file by the web's own list (media types and extensions, such as
 * `image/png,.pdf`, or a whole family like every picture) and whether it
 * takes one or several; this decides which of the system's pickers opens,
 * checks each file that comes back against the list, and names, in the
 * client's own terms, why a file was not taken.
 */
object FileRules {
    /**
     * The default for the most bytes one file may have, and the most all the
     * files of one field may have together: 100 MiB. The protocol gives a file
     * field no largest size; an app sets its own with
     * `NavreeConfig.mostFileBytes`. It guards the device's storage, since each
     * file chosen is copied into the app's cache to be sent from there.
     */
    const val MOST_BYTES: Long = 100L * 1024 * 1024

    /** Which of the system's pickers opens. */
    enum class Source {
        /** The photo picker: the field takes only pictures and video. */
        Photos,

        /** The system's file picker, over the device and its cloud drives. */
        Files,
    }

    /** What the photo picker is asked to show. */
    sealed interface Media {
        data object ImageAndVideo : Media

        data object ImageOnly : Media

        data object VideoOnly : Media

        /** Exactly one kind, such as `image/png`. */
        data class Single(val mime: String) : Media
    }

    /** How the picker for a field is opened. */
    data class Plan(
        val source: Source,
        val multiple: Boolean,
        /** For [Source.Photos]. */
        val media: Media,
        /** For [Source.Files]: the kinds the file picker is limited to, every kind when it is not. */
        val documentTypes: List<String>,
    )

    /** Why a file was not taken. */
    sealed interface Refusal {
        val name: String

        /** The file has more bytes than the most. */
        data class TooLarge(override val name: String, val most: Long) : Refusal

        /** The file alone fits, but with the files the field already holds it would have more bytes than the most. */
        data class TooLargeTogether(override val name: String, val most: Long) : Refusal

        /** The file is not one of the kinds the field takes; [accepted] is the field's own list. */
        data class WrongType(override val name: String, val accepted: String) : Refusal

        /** The file could not be read. */
        data class Unreadable(override val name: String) : Refusal

        /** Where the file is kept gave nothing of it for longer than the client waits. */
        data class NoAnswer(override val name: String) : Refusal

        /** The place a picture or video was made is in it, and the field does not keep it, and it cannot be taken out of this kind of file. */
        data class WithLocation(override val name: String) : Refusal

        /** A picture of a kind this device cannot read for the place it was taken, for a field that does not keep it. */
        data class PlaceUnchecked(override val name: String) : Refusal

        /** A video of a kind this device cannot rewrite without the place it was made, for a field that does not keep it. */
        data class VideoUnchecked(override val name: String) : Refusal
    }

    /** A kind of picture, read from the file's own first bytes, never from its type or name. */
    enum class Picture {
        Jpeg,
        Png,
        Webp,
        Gif,

        /** HEIC and HEIF: an ISO media file whose brands name HEVC pictures (`heic`, `heix`, `mif1` and the like). */
        Heic,

        /** AVIF: an ISO media file whose brands name AV1 pictures (`avif`, `avis`). */
        Avif,

        /** TIFF and the raw formats of cameras (DNG, CR2, NEF, ORF, RW2, CR3, RAF and others). */
        Raw,
    }

    /** What the client does about the place a picture was taken, for a field that does not keep it. */
    enum class Place {
        /** Not a picture: nothing to do. */
        None,

        /** Taken out of the copy (JPEG, PNG, WebP: the kinds `ExifInterface` writes), then read again to see it is gone. */
        TakeOut,

        /** Read, and the picture refused when it holds one: a kind this device reads but `ExifInterface` does not write. */
        Check,

        /** Refused: a kind this device cannot read for it. */
        Refuse,
    }

    /**
     * What is done about the place in a picture of [kind] on a system of API
     * level [sdk]. `ExifInterface` (androidx, the reference on
     * developer.android.com) reads "JPEG, PNG, WebP, HEIC (on API 28+), DNG,
     * CR2, NEF, NRW, ARW, RW2, ORF, PEF, SRW, RAF, AVIF (on API 31+)" and writes
     * "JPEG, PNG, WebP". The raw formats and TIFF are refused, though some are
     * read, since they cannot be rewritten and TIFF is not on the list. A GIF
     * has no Exif data: only an XMP packet in it can name a place, and that is
     * read.
     */
    fun place(kind: Picture?, sdk: Int): Place = when (kind) {
        null -> Place.None
        Picture.Jpeg, Picture.Png, Picture.Webp -> Place.TakeOut
        Picture.Gif -> Place.Check
        Picture.Heic -> if (sdk >= 28) Place.Check else Place.Refuse
        Picture.Avif -> if (sdk >= 31) Place.Check else Place.Refuse
        Picture.Raw -> Place.Refuse
    }

    /** A kind of video, read from the file's own first bytes (or a `video/` media type when its container is another). */
    enum class Video {
        /** An ISO media or QuickTime container (MP4, MOV, M4V, 3GP), whose location boxes can be removed. */
        Iso,

        /** Any other video container (WebM, Matroska, AVI, Ogg, transport streams), which the client does not rewrite. */
        Other,
    }

    /** What is done about the place a video was made, for a field that does not keep it. */
    fun videoPlace(kind: Video?): Place = when (kind) {
        null -> Place.None
        Video.Iso -> Place.TakeOut
        Video.Other -> Place.Refuse
    }

    /** The kinds a field's `accept` names: trimmed, in lower case, none empty. */
    fun kinds(accept: String?): List<String> =
        accept.orEmpty().split(',').map { it.trim().lowercase() }.filter { it.isNotEmpty() }

    /**
     * Where the files come from, by the protocol's own rule: a field that
     * takes only pictures and video opens the photo picker, any other opens
     * the file picker. Both ask for no permission.
     */
    fun source(accept: String?): Source {
        val kinds = kinds(accept)
        return if (kinds.isNotEmpty() && kinds.all { it.startsWith("image/") || it.startsWith("video/") }) Source.Photos else Source.Files
    }

    /** What the photo picker shows: one kind when the field names exactly one, else pictures, video or both. */
    fun media(accept: String?): Media {
        val kinds = kinds(accept)
        val one = kinds.distinct().singleOrNull()?.takeIf { !it.endsWith("/*") }
        val images = kinds.any { it.startsWith("image/") }
        val video = kinds.any { it.startsWith("video/") }
        return when {
            one != null -> Media.Single(one)
            images && !video -> Media.ImageOnly
            video && !images -> Media.VideoOnly
            else -> Media.ImageAndVideo
        }
    }

    /**
     * The kinds the file picker is limited to: a kind named by its media type
     * as it is, one named by its extension (`.pdf`) as the system's type for
     * it ([typeOfExtension], given the extension without its dot). A list with
     * an extension the system has no type for shows every file instead of
     * hiding some; what comes back is checked again by [refusal].
     */
    fun documentTypes(accept: String?, typeOfExtension: (String) -> String?): List<String> {
        val kinds = kinds(accept)
        if (kinds.isEmpty()) return ANY
        val types = ArrayList<String>()
        for (kind in kinds) {
            val type = if (kind.startsWith(".")) typeOfExtension(kind.drop(1)) else kind
            if (type == null || type.isBlank()) return ANY
            types += type
        }
        return types.distinct()
    }

    private val ANY = listOf("*/*")

    /** How a field is asked for its files. */
    fun plan(accept: String?, multiple: Boolean, typeOfExtension: (String) -> String?): Plan =
        Plan(source(accept), multiple, media(accept), documentTypes(accept, typeOfExtension))

    /**
     * Whether a file of this [name] and media type [mime] is one the list
     * takes: an extension matches the end of the name, a whole family (every
     * `image` type) the start of the type, any other kind the whole type. A field that names no kind takes every file.
     */
    fun accepts(accept: String?, name: String, mime: String?): Boolean {
        val kinds = kinds(accept)
        if (kinds.isEmpty()) return true
        val lowerName = name.lowercase()
        val type = mime?.lowercase()?.substringBefore(';')?.trim()
        return kinds.any { kind ->
            when {
                kind == "*/*" || kind == "*" -> true
                kind.startsWith(".") -> lowerName.endsWith(kind)
                kind.endsWith("/*") -> type != null && type.startsWith(kind.dropLast(1))
                else -> type == kind
            }
        }
    }

    /**
     * Why a file with this name, media type and size (when it is known) is not
     * taken, or null when it is. [held] is how many bytes the other files of the
     * field have already: with several files the total obeys [most] as well.
     */
    fun refusal(accept: String?, name: String, mime: String?, size: Long?, most: Long = MOST_BYTES, held: Long = 0): Refusal? = when {
        size != null && size > most -> Refusal.TooLarge(name, most)
        size != null && held + size > most -> Refusal.TooLargeTogether(name, most)
        !accepts(accept, name, mime) -> Refusal.WrongType(name, kinds(accept).joinToString(", "))
        else -> null
    }

    /** The most characters of a file's name that are kept. */
    const val MOST_NAME = 255

    /**
     * The name a file is sent under and shown by: the last part of what the
     * provider gave (never a path), without control characters or the format
     * characters that turn the order of text around or hide in it (such as
     * U+202E), `file` when nothing is left. It is read character by
     * character and not by halves of one, so a format character outside the
     * first plane (a tag character, which draws nothing) goes too, and so
     * does half of a pair that stands alone. A name longer than [MOST_NAME]
     * characters is cut to that, and keeps its ending.
     */
    fun displayName(raw: String?): String {
        val last = raw.orEmpty().substringAfterLast('/').substringAfterLast('\\')
        val clean = StringBuilder()
        var at = 0
        while (at < last.length) {
            val point = last.codePointAt(at)
            at += Character.charCount(point)
            val type = Character.getType(point)
            if (Character.isISOControl(point) || type == Character.FORMAT.toInt() || type == Character.SURROGATE.toInt()) continue
            clean.appendCodePoint(point)
        }
        return shortened(clean.toString().trim()).ifEmpty { "file" }
    }

    /** A name of at most [MOST_NAME] characters: a longer one loses its middle part, never its ending (what stands after the last dot, where that is short). */
    private fun shortened(name: String): String {
        if (name.codePointCount(0, name.length) <= MOST_NAME) return name
        val dot = name.lastIndexOf('.')
        val ending = if (dot > 0 && name.length - dot <= 16) name.substring(dot) else ""
        val kept = MOST_NAME - ending.codePointCount(0, ending.length)
        return name.substring(0, name.offsetByCodePoints(0, kept)).trim() + ending
    }

    /** A token the client made for a kept file: 32 lower-case hex digits, so it can never name a path. */
    fun isToken(token: String): Boolean = token.length == 32 && token.all { it in '0'..'9' || it in 'a'..'f' }
}
