package com.navree.client.platform

import android.content.ContentResolver
import android.content.Context
import android.net.Uri
import android.os.Build
import android.provider.OpenableColumns
import android.webkit.MimeTypeMap
import androidx.exifinterface.media.ExifInterface
import com.navree.client.core.FileRef
import com.navree.client.design.FileRules
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.ensureActive
import kotlinx.coroutines.withContext
import okhttp3.MediaType.Companion.toMediaTypeOrNull
import okhttp3.RequestBody
import okhttp3.RequestBody.Companion.asRequestBody
import java.io.File
import java.io.InputStream
import java.io.OutputStream
import java.util.UUID
import java.util.concurrent.ConcurrentHashMap

/**
 * The files a person chose for a form's file fields, kept by the client until
 * the form sends them. The core holds only each file's token, name, type and
 * size; a request that names a token reads its bytes from here.
 *
 * A file is copied, as it is chosen and off the main thread, into the app's
 * cache folder (never into a backup or a transfer to another device), under a
 * token the client makes: the grant the system gives for a chosen document or
 * picture does not outlast the app, and the copy is what is sent, so the
 * document can change or go meanwhile. The copy is read as a stream when it is
 * sent, never all into memory. A file that is gone by then is sent as no file,
 * never as an empty one.
 *
 * The folder is emptied at the end of a session, and a copy a past process
 * left (the core's own state did not outlive it) is removed when a session starts.
 *
 * Unless the field keeps it, the place a picture was taken is taken out of the
 * copy, or the picture is refused, by what the copy's own bytes say it is
 * ([FileRules.place], [PictureData]): never by the type the provider gave or
 * the file's name.
 */
class PickedFiles internal constructor(private val folder: File) {
    /** The most bytes one file may have, and all the files of one field together: `NavreeConfig.mostFileBytes`. */
    @Volatile
    internal var mostBytes: Long = FileRules.MOST_BYTES

    /** What a file was chosen as: its media type, by token. A token that is here was made by this process. */
    private val types = ConcurrentHashMap<String, String>()

    /** What came of one chosen document. */
    sealed interface Result {
        data class Kept(val file: FileRef) : Result

        data class Refused(val why: FileRules.Refusal) : Result
    }

    /**
     * Copies the document at [uri] and keeps it, or says why it was not taken:
     * larger than the most, alone or with the [held] bytes the field's other
     * files have (told from the provider's size at once, and again while it is
     * copied, since a provider may not know its size), not a kind
     * the field takes ([accept]), or not readable. Unless [keepLocation], the
     * place a picture was taken is taken out of the copy first. Runs on a
     * background thread and stops when the caller's job is cancelled.
     */
    suspend fun keep(resolver: ContentResolver, uri: Uri, accept: String?, keepLocation: Boolean, held: Long = 0): Result = withContext(Dispatchers.IO) {
        val (shownName, knownSize) = describe(resolver, uri)
        val name = FileRules.displayName(shownName ?: uri.lastPathSegment)
        val mime = mimeOf(resolver, uri, name)
        FileRules.refusal(accept, name, mime, knownSize, mostBytes, held)?.let { return@withContext Result.Refused(it) }
        folder.mkdirs()
        sweep()
        val token = UUID.randomUUID().toString().replace("-", "")
        val copy = File(folder, token)
        try {
            val input = resolver.openInputStream(uri) ?: return@withContext Result.Refused(FileRules.Refusal.Unreadable(name))
            val copied = input.use { source -> copy.outputStream().use { sink -> copyUpTo(source, sink, mostBytes - held) } }
            if (!copied) {
                copy.delete()
                return@withContext Result.Refused(if (held > 0) FileRules.Refusal.TooLargeTogether(name, mostBytes) else FileRules.Refusal.TooLarge(name, mostBytes))
            }
            if (!keepLocation) {
                // What the file is, by its own first bytes: never by the type the provider gave or the name.
                val kind = PictureData.kindOf(copy)
                val refused = when (FileRules.place(kind, Build.VERSION.SDK_INT)) {
                    FileRules.Place.None -> null
                    FileRules.Place.TakeOut -> if (takeOutPlace(copy, kind!!)) null else FileRules.Refusal.WithLocation(name)
                    FileRules.Place.Check -> if (holdsPlace(copy, kind!!)) FileRules.Refusal.WithLocation(name) else null
                    FileRules.Place.Refuse -> FileRules.Refusal.PlaceUnchecked(name)
                }
                if (refused != null) {
                    copy.delete()
                    return@withContext Result.Refused(refused)
                }
            }
        } catch (e: CancellationException) {
            copy.delete()
            throw e
        } catch (_: Exception) {
            // A document that went, a grant that ended, a provider that failed: it could not be read.
            copy.delete()
            return@withContext Result.Refused(FileRules.Refusal.Unreadable(name))
        }
        types[token] = mime
        Result.Kept(FileRef(token = token, name = name, mediaType = mime, size = copy.length()))
    }

    /** The kept copy under a token, or null for a token that is not one of ours or a copy that is gone. */
    fun file(token: String): File? = if (FileRules.isToken(token)) File(folder, token).takeIf { it.isFile } else null

    /** Whether the bytes under a token can still be read. A cheap check, but it touches the disk: not on the main thread. */
    fun readable(token: String): Boolean = file(token)?.canRead() == true

    /**
     * The body a request sends for a token, streamed from the copy, or null
     * when the copy is gone (the part is then left out of the request).
     */
    fun body(token: String): RequestBody? = file(token)?.takeIf { it.canRead() }?.asRequestBody((types[token] ?: "application/octet-stream").toMediaTypeOrNull())

    /** Lets the copy under a token go. */
    fun drop(token: String) {
        file(token)?.delete()
        types.remove(token)
    }

    /** Lets every copy go: the session ended. */
    fun clear() {
        types.clear()
        folder.deleteRecursively()
    }

    /** Removes the copies no token of this process names: what a process that ended left behind. */
    fun purgeStale() {
        folder.listFiles()?.forEach { if (!types.containsKey(it.name)) it.delete() }
    }

    /** Removes the copies a form has had a day to send. */
    private fun sweep() {
        val cut = System.currentTimeMillis() - DAY_MS
        folder.listFiles()?.forEach {
            if (it.lastModified() < cut) {
                types.remove(it.name)
                it.delete()
            }
        }
    }

    private fun describe(resolver: ContentResolver, uri: Uri): Pair<String?, Long?> = try {
        resolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME, OpenableColumns.SIZE), null, null, null)?.use { cursor ->
            if (!cursor.moveToFirst()) {
                null to null
            } else {
                val name = cursor.getColumnIndex(OpenableColumns.DISPLAY_NAME).takeIf { it >= 0 && !cursor.isNull(it) }?.let(cursor::getString)
                // The size is null when the provider does not know it, as with a file on a cloud drive.
                val size = cursor.getColumnIndex(OpenableColumns.SIZE).takeIf { it >= 0 && !cursor.isNull(it) }?.let(cursor::getLong)
                name to size
            }
        } ?: (null to null)
    } catch (_: Exception) {
        null to null
    }

    /** The media type the provider names, else the one the name's extension stands for, else none known. */
    private fun mimeOf(resolver: ContentResolver, uri: Uri, name: String): String {
        val given = try {
            resolver.getType(uri)
        } catch (_: Exception) {
            null
        }
        if (given != null && given.isNotBlank() && given != "application/octet-stream") return given
        val extension = name.substringAfterLast('.', "").lowercase()
        return extension.takeIf { it.isNotEmpty() }?.let { MimeTypeMap.getSingleton().getMimeTypeFromExtension(it) } ?: given?.takeIf { it.isNotBlank() } ?: "application/octet-stream"
    }

    /** Copies at most [most] bytes; false when the source has more. Stops when the job is cancelled. */
    private suspend fun copyUpTo(source: InputStream, sink: OutputStream, most: Long): Boolean {
        val buffer = ByteArray(BUFFER)
        var total = 0L
        while (true) {
            currentCoroutineContext().ensureActive()
            val read = source.read(buffer)
            if (read < 0) return true
            total += read
            if (total > most) return false
            sink.write(buffer, 0, read)
        }
    }

    /**
     * Takes the place a picture was taken out of the copy of a JPEG, PNG or
     * WebP, and reads what is left to see that it is gone. First the parts
     * outside the Exif data that can name a place ([PictureData.clean]: an XMP
     * packet that names one, Exif blocks after the first, other programs'
     * metadata, whatever follows the end of the picture); then every GPS tag
     * of the Exif data, and an XMP packet kept in the Exif data that names a
     * place, with `ExifInterface`. True when the copy, read again, holds none.
     */
    private fun takeOutPlace(copy: File, kind: FileRules.Picture): Boolean {
        val cleaned = File(copy.path + ".clean")
        return try {
            if (PictureData.clean(copy, cleaned, kind).broken || !cleaned.renameTo(copy)) return false
            val exif = ExifInterface(copy)
            var changed = false
            for (tag in GPS_TAGS) {
                if (exif.getAttribute(tag) != null) {
                    exif.setAttribute(tag, null)
                    changed = true
                }
            }
            if (exif.getAttributeBytes(ExifInterface.TAG_XMP)?.let(PictureData::namesPlace) == true) {
                exif.setAttribute(ExifInterface.TAG_XMP, null)
                changed = true
            }
            if (changed) exif.saveAttributes()
            // What will be sent, read again from the start.
            val again = ExifInterface(copy)
            GPS_TAGS.none { again.getAttribute(it) != null } &&
                again.latLong == null &&
                again.getAttributeBytes(ExifInterface.TAG_XMP)?.let(PictureData::namesPlace) != true &&
                PictureData.inspect(copy, kind).clean
        } catch (_: Exception) {
            false
        } finally {
            cleaned.delete()
        }
    }

    /**
     * Whether a picture of a kind the client does not rewrite (HEIC, AVIF,
     * GIF) holds a place: a GPS tag or an XMP packet `ExifInterface` reads in
     * it (not for a GIF, which has no Exif data), or an XMP packet in its
     * bytes that names one. A picture that cannot be read is taken to hold one.
     */
    private fun holdsPlace(copy: File, kind: FileRules.Picture): Boolean = try {
        val exif = if (kind == FileRules.Picture.Gif) null else ExifInterface(copy)
        val inExif = exif != null && (
            GPS_TAGS.any { exif.getAttribute(it) != null } ||
                exif.latLong != null ||
                exif.getAttributeBytes(ExifInterface.TAG_XMP)?.let(PictureData::namesPlace) == true
            )
        inExif || copy.inputStream().buffered().use { PictureData.packetNamesPlace(it) }
    } catch (_: Exception) {
        true
    }

    companion object {
        private const val DAY_MS = 24L * 60 * 60 * 1000
        private const val BUFFER = 64 * 1024

        @Volatile
        private var store: PickedFiles? = null

        /** The one store of the app's process. */
        fun of(context: Context): PickedFiles = store ?: synchronized(this) {
            store ?: PickedFiles(File(context.applicationContext.cacheDir, "navree-picked")).also { store = it }
        }

        /**
         * Every GPS tag of a picture's Exif data, as the `ExifInterface`
         * reference lists them: where it was taken, the place it points to
         * (the `GPSDest` tags), and the time, direction, speed and means of the
         * fix.
         */
        internal val GPS_TAGS: List<String> = listOf(
            ExifInterface.TAG_GPS_VERSION_ID,
            ExifInterface.TAG_GPS_LATITUDE, ExifInterface.TAG_GPS_LATITUDE_REF,
            ExifInterface.TAG_GPS_LONGITUDE, ExifInterface.TAG_GPS_LONGITUDE_REF,
            ExifInterface.TAG_GPS_ALTITUDE, ExifInterface.TAG_GPS_ALTITUDE_REF,
            ExifInterface.TAG_GPS_TIMESTAMP, ExifInterface.TAG_GPS_DATESTAMP,
            ExifInterface.TAG_GPS_SATELLITES, ExifInterface.TAG_GPS_STATUS,
            ExifInterface.TAG_GPS_MEASURE_MODE, ExifInterface.TAG_GPS_DOP,
            ExifInterface.TAG_GPS_SPEED, ExifInterface.TAG_GPS_SPEED_REF,
            ExifInterface.TAG_GPS_TRACK, ExifInterface.TAG_GPS_TRACK_REF,
            ExifInterface.TAG_GPS_IMG_DIRECTION, ExifInterface.TAG_GPS_IMG_DIRECTION_REF,
            ExifInterface.TAG_GPS_MAP_DATUM,
            ExifInterface.TAG_GPS_DEST_LATITUDE, ExifInterface.TAG_GPS_DEST_LATITUDE_REF,
            ExifInterface.TAG_GPS_DEST_LONGITUDE, ExifInterface.TAG_GPS_DEST_LONGITUDE_REF,
            ExifInterface.TAG_GPS_DEST_BEARING, ExifInterface.TAG_GPS_DEST_BEARING_REF,
            ExifInterface.TAG_GPS_DEST_DISTANCE, ExifInterface.TAG_GPS_DEST_DISTANCE_REF,
            ExifInterface.TAG_GPS_PROCESSING_METHOD, ExifInterface.TAG_GPS_AREA_INFORMATION,
            ExifInterface.TAG_GPS_DIFFERENTIAL, ExifInterface.TAG_GPS_H_POSITIONING_ERROR,
        )
    }
}
