package com.navree.client.platform

import android.content.ContentProvider
import android.content.ContentResolver
import android.content.ContentValues
import android.content.Context
import android.database.Cursor
import android.database.MatrixCursor
import android.media.MediaCodec
import android.media.MediaExtractor
import android.media.MediaFormat
import android.media.MediaMetadataRetriever
import android.media.MediaMuxer
import android.net.Uri
import android.os.Build
import android.os.ParcelFileDescriptor
import android.os.SystemClock
import android.provider.OpenableColumns
import android.webkit.MimeTypeMap
import androidx.exifinterface.media.ExifInterface
import com.navree.client.core.FileRef
import com.navree.client.design.FileRules
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.withTimeoutOrNull
import okhttp3.MediaType.Companion.toMediaTypeOrNull
import okhttp3.RequestBody
import okhttp3.RequestBody.Companion.asRequestBody
import java.io.File
import java.io.InputStream
import java.io.OutputStream
import java.nio.ByteBuffer
import java.nio.file.Files
import java.nio.file.StandardCopyOption
import java.util.UUID
import java.util.concurrent.ConcurrentHashMap
import kotlin.concurrent.thread

/**
 * The files a person chose for a form's file fields, kept by the client until
 * the form sends them. The core holds only each file's token, name, type and
 * size; a request that names a token reads its bytes from here.
 *
 * A file is copied, as it is chosen and off the main thread, into the app's
 * cache folder (never into a backup or a transfer to another device), under a
 * token the client makes: the grant the system gives for a chosen document or
 * picture does not outlast the app, and the copy is what is sent, so the
 * document can change or go meanwhile. A provider that stops answering while
 * its document is read is given up after a time, and the file refused. The copy is read as a stream when it is
 * sent, never all into memory. A file that is gone by then is sent as no file,
 * never as an empty one.
 *
 * A copy goes when its form's server took the form ([sent]), when its field
 * leaves a screen the core no longer holds (the file field drops it), when it
 * is removed or replaced, a day after it was made (looked at with each new
 * file), and at the end of a session; a copy a past process left (the core's
 * own state did not outlive it) is removed when a session starts.
 *
 * Unless the field keeps it, the place a picture was taken is taken out of the
 * copy, or the picture is refused, by what the copy's own bytes say it is
 * ([FileRules.place], [PictureData]): never by the type the provider gave or
 * the file's name. So is the place a video was made ([FileRules.videoPlace],
 * [VideoData]); a Matroska or WebM file that holds sound alone is no video,
 * and is taken without what names a place in it ([MatroskaData]).
 */
class PickedFiles internal constructor(private val folder: File) {
    /** The most bytes one file may have, and all the files of one field together: `NavreeConfig.mostFileBytes`. */
    @Volatile
    internal var mostBytes: Long = FileRules.MOST_BYTES

    /** The longest a document's provider is waited for while it gives nothing, in milliseconds. */
    @Volatile
    internal var mostWaitMs: Long = NO_ANSWER_MS

    /** What a file was chosen as: its media type, by token. A token that is here was made by this process. */
    private val types = ConcurrentHashMap<String, String>()

    /** Metadata of each kept file by token, for [PickedFilesProvider]. */
    private val refs = ConcurrentHashMap<String, FileRef>()

    /** Which session the store is in: one more at each [clear]. Read and changed under [lock]. */
    private var generation = 0L
    private val lock = Any()

    /** What came of one chosen document. */
    sealed interface Result {
        data class Kept(val file: FileRef) : Result

        data class Refused(val why: FileRules.Refusal) : Result
    }

    /** One document being read: when its provider last gave something, and what became of it. */
    private class Reading(@Volatile var name: String) {
        @Volatile
        private var heard = SystemClock.elapsedRealtime()

        /** The provider has given everything: what is left is the client's own work on the copy, which no clock is held against. */
        @Volatile
        var local = false

        /** The field has given this document up: whatever its reader still does is thrown away. */
        @Volatile
        var givenUp = false

        /** Where the bytes are being copied to, once that has begun. */
        @Volatile
        var copy: File? = null

        /** The file, once it is kept. Read and written under the store's lock. */
        var kept: FileRef? = null

        /** The provider answered, or gave bytes. */
        fun heardNow() {
            heard = SystemClock.elapsedRealtime()
        }

        /** How long the provider has given nothing, in milliseconds; nothing once the rest is the client's own work. */
        fun silentFor(): Long = if (local) 0 else SystemClock.elapsedRealtime() - heard
    }

    /** The field gave the document up while its provider was being read. */
    private class GivenUp : Exception()

    /**
     * Copies the document at [uri] and keeps it, or says why it was not taken:
     * larger than the most, alone or with the [held] bytes the field's other
     * files have (told from the provider's size at once, and again while it is
     * copied, since a provider may not know its size), not a kind
     * the field takes ([accept]), not readable, or kept somewhere that does not
     * answer. Unless [keepLocation], the place a picture was taken is taken out
     * of the copy first.
     *
     * The provider is asked on a thread of its own, and is waited for only as
     * long as it keeps answering: one that gives nothing for [mostWaitMs] (no
     * answer to a question, no byte of the document) is given up, the file is
     * refused as [FileRules.Refusal.NoAnswer], and its copy is deleted. A read
     * that never returns can be neither interrupted nor closed from another
     * thread with any promise from the platform, so that thread is left to its
     * provider; the field is not. It stops the same way when the caller's job
     * is cancelled.
     */
    suspend fun keep(resolver: ContentResolver, uri: Uri, accept: String?, keepLocation: Boolean, held: Long = 0): Result {
        val session = synchronized(lock) { generation }
        val reading = Reading(FileRules.displayName(uri.lastPathSegment))
        val result = CompletableDeferred<Result>()
        val reader = thread(name = "navree-picked-read", isDaemon = true) {
            result.complete(
                try {
                    read(resolver, uri, accept, keepLocation, held, session, reading)
                } catch (_: Throwable) {
                    Result.Refused(FileRules.Refusal.Unreadable(reading.name))
                },
            )
        }
        try {
            while (true) {
                withTimeoutOrNull(WATCH_MS) { result.await() }?.let { return it }
                if (reading.silentFor() >= mostWaitMs) break
            }
        } catch (e: CancellationException) {
            giveUp(reading)?.let { drop(it.token) }
            reader.interrupt()
            throw e
        }
        // It may have been kept in the same moment: then it is kept.
        giveUp(reading)?.let { return Result.Kept(it) }
        reader.interrupt()
        return Result.Refused(FileRules.Refusal.NoAnswer(reading.name))
    }

    /** Gives a document up: its reader keeps nothing from now, and what it had copied goes. Answers the file if it was kept already. */
    private fun giveUp(reading: Reading): FileRef? {
        val kept = synchronized(lock) {
            reading.givenUp = true
            reading.kept
        }
        if (kept == null) reading.copy?.delete()
        return kept
    }

    /** The reading itself, on the reader's thread: every call into the provider can block for as long as the provider likes. */
    private fun read(resolver: ContentResolver, uri: Uri, accept: String?, keepLocation: Boolean, held: Long, session: Long, reading: Reading): Result {
        val (shownName, knownSize) = describe(resolver, uri)
        val name = FileRules.displayName(shownName ?: uri.lastPathSegment)
        reading.name = name
        reading.heardNow()
        val mime = mimeOf(resolver, uri, name)
        reading.heardNow()
        FileRules.refusal(accept, name, mime, knownSize, mostBytes, held)?.let { return Result.Refused(it) }
        folder.mkdirs()
        sweep()
        val token = UUID.randomUUID().toString().replace("-", "")
        val copy = File(folder, token)
        reading.copy = copy
        try {
            val input = resolver.openInputStream(uri) ?: return Result.Refused(FileRules.Refusal.Unreadable(name))
            reading.heardNow()
            val copied = input.use { source -> copy.outputStream().use { sink -> copyUpTo(source, sink, mostBytes - held, reading) } }
            if (!copied) {
                copy.delete()
                return Result.Refused(if (held > 0) FileRules.Refusal.TooLargeTogether(name, mostBytes) else FileRules.Refusal.TooLarge(name, mostBytes))
            }
            // From here on the provider is not asked again.
            reading.local = true
            if (!keepLocation) {
                // What the file is, by its own first bytes: never by the type the provider gave or the name, save to refuse a video whose container is another.
                val picture = PictureData.kindOf(copy)
                val refused = if (picture != null) {
                    when (FileRules.place(picture, Build.VERSION.SDK_INT)) {
                        FileRules.Place.None -> null
                        FileRules.Place.TakeOut -> if (takeOutPlace(copy, picture)) null else FileRules.Refusal.WithLocation(name)
                        FileRules.Place.Check -> if (holdsPlace(copy, picture)) FileRules.Refusal.WithLocation(name) else null
                        FileRules.Place.Refuse -> FileRules.Refusal.PlaceUnchecked(name)
                    }
                } else {
                    // A Matroska or WebM file that holds sound alone is no video, whatever type or name it came under: it is taken, without what names a place in it.
                    val video = VideoData.kindOf(copy, mime)
                    when (FileRules.videoPlace(video)) {
                        FileRules.Place.None -> null
                        FileRules.Place.TakeOut -> {
                            val taken = if (video == FileRules.Video.Sound) takeOutSoundPlace(copy) else takeOutVideoPlace(copy)
                            if (taken) null else FileRules.Refusal.WithLocation(name)
                        }
                        FileRules.Place.Check, FileRules.Place.Refuse -> FileRules.Refusal.VideoUnchecked(name)
                    }
                }
                if (refused != null) {
                    copy.delete()
                    return Result.Refused(refused)
                }
            }
        } catch (_: Exception) {
            // A document that went, a grant that ended, a provider that failed, a field that gave it up: it could not be read.
            copy.delete()
            return Result.Refused(FileRules.Refusal.Unreadable(name))
        }
        // A session that ended while the file was read takes its copy with it, and so does a field that gave the file up: no token of either is kept.
        val file = FileRef(token = token, name = name, mediaType = mime, size = copy.length())
        val kept = synchronized(lock) {
            if (generation != session || reading.givenUp) false else {
                types[token] = mime
                refs[token] = file
                reading.kept = file
                true
            }
        }
        if (!kept) {
            copy.delete()
            return Result.Refused(FileRules.Refusal.Unreadable(name))
        }
        return Result.Kept(file)
    }

    /** The kept copy under a token, or null for a token that is not one of ours or a copy that is gone. */
    fun file(token: String): File? = if (FileRules.isToken(token)) File(folder, token).takeIf { it.isFile } else null

    /** Metadata of the kept copy under [token], or null when none is held. */
    internal fun ref(token: String): FileRef? = if (FileRules.isToken(token)) refs[token] else null

    /** A read-only `content://` address inside this app for a kept file handed to a web view's file chooser. */
    internal fun uri(context: Context, file: FileRef): Uri = PickedFilesProvider.uri(context, file)

    /** Whether the bytes under a token can still be read. A cheap check, but it touches the disk: not on the main thread. */
    fun readable(token: String): Boolean = file(token)?.canRead() == true

    /**
     * The body a request sends for a token, streamed from the copy, or null
     * when the copy is gone (the part is then left out of the request).
     */
    fun body(token: String): RequestBody? = file(token)?.takeIf { it.canRead() }?.asRequestBody((types[token] ?: "application/octet-stream").toMediaTypeOrNull())

    /** Lets the copy under a token go. */
    fun drop(token: String) {
        if (FileRules.isToken(token)) File(folder, token).delete()
        types.remove(token)
        refs.remove(token)
    }

    /** Lets the copies under these tokens go: a form sent them and the server took it. */
    fun sent(tokens: List<String>) {
        for (token in tokens) drop(token)
    }

    /** Lets every copy go: the session ended. A file still being read when this runs is not kept either. */
    fun clear() {
        synchronized(lock) {
            generation++
            types.clear()
            refs.clear()
        }
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
                refs.remove(it.name)
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

    /** Copies at most [most] bytes; false when the source has more. Stops when the field has given the document up. */
    private fun copyUpTo(source: InputStream, sink: OutputStream, most: Long, reading: Reading): Boolean {
        val buffer = ByteArray(BUFFER)
        var total = 0L
        while (true) {
            if (reading.givenUp) throw GivenUp()
            val read = source.read(buffer)
            reading.heardNow()
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

    /**
     * Rewrites an ISO or QuickTime video without the place it was made (remuxing
     * video and audio tracks alone first when the container also carries timed
     * metadata or other tracks), then checks the result again through the box
     * tree, raw XMP packets, and `MediaMetadataRetriever`. False when any
     * step fails or still finds a place.
     */
    private fun takeOutVideoPlace(copy: File): Boolean {
        val remuxed = File(copy.path + ".remux")
        val cleaned = File(copy.path + ".clean")
        return try {
            val initial = VideoData.inspect(copy)
            if (initial.broken) return false
            if (initial.nonMediaTracks > 0) {
                if (!remuxMediaTracksOnly(copy, remuxed)) return false
                Files.move(remuxed.toPath(), copy.toPath(), StandardCopyOption.REPLACE_EXISTING)
            }
            if (VideoData.clean(copy, cleaned).broken) return false
            Files.move(cleaned.toPath(), copy.toPath(), StandardCopyOption.REPLACE_EXISTING)
            VideoData.inspect(copy).clean &&
                copy.inputStream().buffered().use { !PictureData.packetNamesPlace(it) } &&
                noRetrieverLocation(copy)
        } catch (_: Throwable) {
            false
        } finally {
            remuxed.delete()
            cleaned.delete()
        }
    }

    /**
     * Takes what can say where it was made out of a Matroska or WebM file
     * that holds sound alone ([MatroskaData]): every tag, title and name that
     * names a place, every attached file and what a blanked part still held
     * are overwritten in place, so the sound does not move. A file that holds
     * none of them is left as it is. Then the copy is read again: it must
     * still read cleanly as sound alone, with nothing of that left in it, no
     * XMP packet in its bytes that names a place, and no place the platform
     * itself finds. False when a step fails or a place is still found.
     */
    private fun takeOutSoundPlace(copy: File): Boolean {
        val cleaned = File(copy.path + ".clean")
        return try {
            val initial = MatroskaData.inspect(copy)
            if (!initial.soundOnly) return false
            if (!initial.clean) {
                if (MatroskaData.clean(copy, cleaned).broken) return false
                Files.move(cleaned.toPath(), copy.toPath(), StandardCopyOption.REPLACE_EXISTING)
            }
            val after = MatroskaData.inspect(copy)
            after.soundOnly &&
                after.clean &&
                copy.inputStream().buffered().use { !PictureData.packetNamesPlace(it) } &&
                !retrieverNamesLocation(copy)
        } catch (_: Throwable) {
            false
        } finally {
            cleaned.delete()
        }
    }

    /**
     * Whether the platform itself names a place in a file of sound. One
     * it cannot open names none: the device may have no decoder for the
     * sound, and the file's own bytes have been read for a place already.
     */
    private fun retrieverNamesLocation(file: File): Boolean {
        val retriever = MediaMetadataRetriever()
        return try {
            retriever.setDataSource(file.path)
            retriever.extractMetadata(MediaMetadataRetriever.METADATA_KEY_LOCATION) != null
        } catch (_: Throwable) {
            false
        } finally {
            runCatching { retriever.release() }
        }
    }

    private fun remuxMediaTracksOnly(from: File, to: File): Boolean {
        val extractor = MediaExtractor()
        var muxer: MediaMuxer? = null
        var started = false
        return try {
            extractor.setDataSource(from.path)
            val created = MediaMuxer(to.path, MediaMuxer.OutputFormat.MUXER_OUTPUT_MPEG_4)
            muxer = created
            val trackMap = HashMap<Int, Int>()
            var bufferBytes = 1024 * 1024
            for (index in 0 until extractor.trackCount) {
                val format = extractor.getTrackFormat(index)
                val mime = format.getString(MediaFormat.KEY_MIME).orEmpty()
                if (mime.startsWith("video/") || mime.startsWith("audio/")) {
                    if (mime.startsWith("video/") && format.containsKey(MediaFormat.KEY_ROTATION)) {
                        created.setOrientationHint(format.getInteger(MediaFormat.KEY_ROTATION))
                    }
                    if (format.containsKey(MediaFormat.KEY_MAX_INPUT_SIZE)) {
                        bufferBytes = maxOf(bufferBytes, format.getInteger(MediaFormat.KEY_MAX_INPUT_SIZE))
                    }
                    trackMap[index] = created.addTrack(format)
                    extractor.selectTrack(index)
                }
            }
            if (trackMap.isEmpty()) return false
            created.start()
            started = true
            val buffer = ByteBuffer.allocate(bufferBytes)
            val info = MediaCodec.BufferInfo()
            while (true) {
                val track = extractor.sampleTrackIndex
                if (track < 0) break
                val dst = trackMap[track] ?: return false
                val sampleFlags = extractor.sampleFlags
                if ((sampleFlags and MediaExtractor.SAMPLE_FLAG_ENCRYPTED) != 0) return false
                buffer.clear()
                val size = extractor.readSampleData(buffer, 0)
                if (size < 0) break
                info.set(
                    0,
                    size,
                    extractor.sampleTime.coerceAtLeast(0L),
                    if ((sampleFlags and MediaExtractor.SAMPLE_FLAG_SYNC) != 0) MediaCodec.BUFFER_FLAG_KEY_FRAME else 0,
                )
                created.writeSampleData(dst, buffer, info)
                extractor.advance()
            }
            created.stop()
            started = false
            true
        } catch (_: Throwable) {
            false
        } finally {
            runCatching { extractor.release() }
            if (started) runCatching { muxer?.stop() }
            runCatching { muxer?.release() }
        }
    }

    private fun noRetrieverLocation(file: File): Boolean {
        val retriever = MediaMetadataRetriever()
        return try {
            retriever.setDataSource(file.path)
            retriever.extractMetadata(MediaMetadataRetriever.METADATA_KEY_LOCATION) == null
        } catch (_: Throwable) {
            false
        } finally {
            runCatching { retriever.release() }
        }
    }

    companion object {
        private const val DAY_MS = 24L * 60 * 60 * 1000
        private const val BUFFER = 64 * 1024

        /** How long a provider may give nothing before its document is given up: a slow network drive gives a byte in this time, a provider that hangs does not. */
        internal const val NO_ANSWER_MS = 20_000L

        /** How often the wait for a document looks at how long its provider has been silent. */
        private const val WATCH_MS = 250L

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

/**
 * Serves cleaned copies from [PickedFiles] to the app's own web view after its
 * file chooser picked them. Unexported and read-only: only tokens held by this
 * process in the current session resolve, and every query answers the cleaned
 * file's sanitized name, media type and byte length.
 */
class PickedFilesProvider : ContentProvider() {
    override fun onCreate(): Boolean = true

    private fun store(): PickedFiles? = context?.let(PickedFiles::of)

    private fun tokenOf(uri: Uri): String? = uri.pathSegments.firstOrNull()?.takeIf(FileRules::isToken)

    override fun query(
        uri: Uri,
        projection: Array<String>?,
        selection: String?,
        selectionArgs: Array<String>?,
        sortOrder: String?,
    ): Cursor? {
        val token = tokenOf(uri) ?: return null
        val picked = store() ?: return null
        val ref = picked.ref(token) ?: return null
        val file = picked.file(token) ?: return null
        val columns = projection ?: arrayOf(OpenableColumns.DISPLAY_NAME, OpenableColumns.SIZE)
        val row = Array<Any?>(columns.size) { index ->
            when (columns[index]) {
                OpenableColumns.DISPLAY_NAME -> ref.name
                OpenableColumns.SIZE -> file.length()
                else -> null
            }
        }
        return MatrixCursor(columns, 1).apply { addRow(row) }
    }

    override fun getType(uri: Uri): String? {
        val token = tokenOf(uri) ?: return null
        return store()?.ref(token)?.mediaType
    }

    override fun openFile(uri: Uri, mode: String): ParcelFileDescriptor? {
        if (mode != "r") throw SecurityException("read-only")
        val token = tokenOf(uri) ?: throw java.io.FileNotFoundException(uri.toString())
        val picked = store() ?: throw java.io.FileNotFoundException(uri.toString())
        if (picked.ref(token) == null) throw java.io.FileNotFoundException(uri.toString())
        val file = picked.file(token) ?: throw java.io.FileNotFoundException(uri.toString())
        return ParcelFileDescriptor.open(file, ParcelFileDescriptor.MODE_READ_ONLY)
    }

    override fun insert(uri: Uri, values: ContentValues?): Uri? = null

    override fun delete(uri: Uri, selection: String?, selectionArgs: Array<String>?): Int = 0

    override fun update(uri: Uri, values: ContentValues?, selection: String?, selectionArgs: Array<String>?): Int = 0

    internal companion object {
        fun uri(context: Context, file: FileRef): Uri = Uri.Builder()
            .scheme(ContentResolver.SCHEME_CONTENT)
            .authority("${context.packageName}.navree.picked")
            .appendPath(file.token)
            .appendPath(file.name)
            .build()
    }
}
