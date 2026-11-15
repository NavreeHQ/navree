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
     * The most bytes one file may have: 100 MiB. The protocol gives a file
     * field no largest size, so this is the client's own limit: a file is
     * copied into the app's cache to be sent from there, and a phone's
     * photographs and short videos fit well inside it.
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

        /** The file is not one of the kinds the field takes; [accepted] is the field's own list. */
        data class WrongType(override val name: String, val accepted: String) : Refusal

        /** The file could not be read. */
        data class Unreadable(override val name: String) : Refusal

        /** The place a picture was taken is in it, and the field does not keep it, and it cannot be taken out of this kind of file. */
        data class WithLocation(override val name: String) : Refusal
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

    /** Why a file with this name, media type and size (when it is known) is not taken, or null when it is. */
    fun refusal(accept: String?, name: String, mime: String?, size: Long?, most: Long = MOST_BYTES): Refusal? = when {
        size != null && size > most -> Refusal.TooLarge(name, most)
        !accepts(accept, name, mime) -> Refusal.WrongType(name, kinds(accept).joinToString(", "))
        else -> null
    }

    /**
     * The name a file is sent under: the last part of what the provider gave
     * (never a path), without control characters, `file` when nothing is left.
     */
    fun displayName(raw: String?): String {
        val last = raw.orEmpty().substringAfterLast('/').substringAfterLast('\\')
        val clean = last.filter { !it.isISOControl() }.trim()
        return clean.ifEmpty { "file" }
    }

    /** The kinds of file whose own data can hold the place a picture was taken, which the client takes out unless the field keeps it. */
    fun mayHoldLocation(mime: String?): Boolean = when (mime?.lowercase()) {
        "image/jpeg", "image/png", "image/webp", "image/heic", "image/heif", "image/avif" -> true
        else -> false
    }

    /** The kinds the picture's own data can be rewritten for, to take the place out: the others are refused when they hold one. */
    fun canRewriteLocation(mime: String?): Boolean = when (mime?.lowercase()) {
        "image/jpeg", "image/png", "image/webp" -> true
        else -> false
    }

    /** A token the client made for a kept file: 32 lower-case hex digits, so it can never name a path. */
    fun isToken(token: String): Boolean = token.length == 32 && token.all { it in '0'..'9' || it in 'a'..'f' }
}
