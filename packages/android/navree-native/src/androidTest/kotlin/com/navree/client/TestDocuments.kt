package com.navree.client

import android.content.ContentProvider
import android.content.ContentValues
import android.content.Context
import android.database.Cursor
import android.database.MatrixCursor
import android.net.Uri
import android.os.ParcelFileDescriptor
import android.provider.OpenableColumns
import java.io.File
import java.io.IOException
import java.util.concurrent.ConcurrentHashMap
import kotlin.concurrent.thread

/**
 * A documents provider a device test hands the file field's picker result
 * from: each document has a name that is not its address, a media type, and
 * either a size or none (a provider that does not know it, as a cloud drive
 * does not), and its bytes are a file in the app's cache that the test writes.
 * It is read as the system's pickers' documents are: by `content://` address,
 * through the resolver, with no permission. A document may also say a size
 * that is not its own, be a stream that never ends, or be opened and then
 * never give a byte: a provider may do any of these, and the client must hold.
 */
class TestDocuments : ContentProvider() {
    private class Document(val file: File?, val name: String, val type: String?, val size: Long?, val endless: ByteArray? = null, val silent: Boolean = false)

    override fun onCreate(): Boolean = true

    private fun document(uri: Uri): Document? = documents[uri.lastPathSegment]

    override fun query(uri: Uri, projection: Array<String>?, selection: String?, selectionArgs: Array<String>?, sortOrder: String?): Cursor? {
        val document = document(uri) ?: return null
        val cursor = MatrixCursor(arrayOf(OpenableColumns.DISPLAY_NAME, OpenableColumns.SIZE))
        cursor.addRow(arrayOf<Any?>(document.name, document.size))
        return cursor
    }

    override fun getType(uri: Uri): String? = document(uri)?.type

    override fun openFile(uri: Uri, mode: String): ParcelFileDescriptor? {
        val document = document(uri) ?: throw java.io.FileNotFoundException(uri.toString())
        if (document.silent) {
            // A pipe nothing is ever written into, whose writing end stays open: a read of it never returns.
            val (read, write) = ParcelFileDescriptor.createPipe()
            held += write
            return read
        }
        val endless = document.endless ?: return ParcelFileDescriptor.open(document.file!!, ParcelFileDescriptor.MODE_READ_ONLY)
        // A pipe the provider writes the same bytes into for as long as the reader reads.
        val (read, write) = ParcelFileDescriptor.createPipe()
        thread(name = "test-documents-endless", isDaemon = true) {
            try {
                ParcelFileDescriptor.AutoCloseOutputStream(write).use { out -> while (true) out.write(endless) }
            } catch (_: IOException) {
                // The reader closed its end.
            }
        }
        return read
    }

    override fun insert(uri: Uri, values: ContentValues?): Uri? = null

    override fun delete(uri: Uri, selection: String?, selectionArgs: Array<String>?): Int = 0

    override fun update(uri: Uri, values: ContentValues?, selection: String?, selectionArgs: Array<String>?): Int = 0

    companion object {
        private const val AUTHORITY = "com.navree.client.test.documents"
        private val documents = ConcurrentHashMap<String, Document>()

        /** The writing ends of the pipes of the documents that never give a byte, held open until the test is over. */
        private val held = java.util.concurrent.CopyOnWriteArrayList<ParcelFileDescriptor>()

        private fun address(id: String): Uri = Uri.parse("content://$AUTHORITY/document/$id")

        /**
         * Writes [bytes] as a document and gives the address a picker would hand
         * back. Its size is its own, none when not [knowsSize], or [claimedSize]
         * when that is given.
         */
        fun add(context: Context, name: String, type: String?, bytes: ByteArray, knowsSize: Boolean = true, claimedSize: Long? = null): Uri {
            val id = java.util.UUID.randomUUID().toString()
            val folder = File(context.cacheDir, "test-documents").apply { mkdirs() }
            val file = File(folder, id).apply { writeBytes(bytes) }
            documents[id] = Document(file, name, type, claimedSize ?: if (knowsSize) bytes.size.toLong() else null)
            return address(id)
        }

        /** A document whose bytes never end: [chunk] again and again. Its size is [claimedSize], or none. */
        fun addEndless(name: String, type: String?, chunk: ByteArray, claimedSize: Long? = null): Uri {
            val id = java.util.UUID.randomUUID().toString()
            documents[id] = Document(null, name, type, claimedSize, endless = chunk)
            return address(id)
        }

        /** A document its provider opens and then never gives a byte of. Its size is not known. */
        fun addSilent(name: String, type: String?): Uri {
            val id = java.util.UUID.randomUUID().toString()
            documents[id] = Document(null, name, type, null, silent = true)
            return address(id)
        }

        /** Lets every document go, and ends the pipes that never gave a byte, so the threads still reading them end too. */
        fun clear() {
            documents.values.forEach { it.file?.delete() }
            documents.clear()
            held.forEach { runCatching { it.close() } }
            held.clear()
        }
    }
}
