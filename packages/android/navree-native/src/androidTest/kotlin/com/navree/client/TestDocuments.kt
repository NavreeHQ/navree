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
import java.util.concurrent.ConcurrentHashMap

/**
 * A documents provider a device test hands the file field's picker result
 * from: each document has a name that is not its address, a media type, and
 * either a size or none (a provider that does not know it, as a cloud drive
 * does not), and its bytes are a file in the app's cache that the test writes.
 * It is read as the system's pickers' documents are: by `content://` address,
 * through the resolver, with no permission.
 */
class TestDocuments : ContentProvider() {
    private class Document(val file: File, val name: String, val type: String?, val knowsSize: Boolean)

    override fun onCreate(): Boolean = true

    private fun document(uri: Uri): Document? = documents[uri.lastPathSegment]

    override fun query(uri: Uri, projection: Array<String>?, selection: String?, selectionArgs: Array<String>?, sortOrder: String?): Cursor? {
        val document = document(uri) ?: return null
        val cursor = MatrixCursor(arrayOf(OpenableColumns.DISPLAY_NAME, OpenableColumns.SIZE))
        cursor.addRow(arrayOf<Any?>(document.name, if (document.knowsSize) document.file.length() else null))
        return cursor
    }

    override fun getType(uri: Uri): String? = document(uri)?.type

    override fun openFile(uri: Uri, mode: String): ParcelFileDescriptor? {
        val document = document(uri) ?: throw java.io.FileNotFoundException(uri.toString())
        return ParcelFileDescriptor.open(document.file, ParcelFileDescriptor.MODE_READ_ONLY)
    }

    override fun insert(uri: Uri, values: ContentValues?): Uri? = null

    override fun delete(uri: Uri, selection: String?, selectionArgs: Array<String>?): Int = 0

    override fun update(uri: Uri, values: ContentValues?, selection: String?, selectionArgs: Array<String>?): Int = 0

    companion object {
        private const val AUTHORITY = "com.navree.client.test.documents"
        private val documents = ConcurrentHashMap<String, Document>()

        /** Writes [bytes] as a document and gives the address a picker would hand back. */
        fun add(context: Context, name: String, type: String?, bytes: ByteArray, knowsSize: Boolean = true): Uri {
            val id = java.util.UUID.randomUUID().toString()
            val folder = File(context.cacheDir, "test-documents").apply { mkdirs() }
            val file = File(folder, id).apply { writeBytes(bytes) }
            documents[id] = Document(file, name, type, knowsSize)
            return Uri.parse("content://$AUTHORITY/document/$id")
        }

        /** Lets every document go. */
        fun clear() {
            documents.values.forEach { it.file.delete() }
            documents.clear()
        }
    }
}
