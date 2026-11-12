package com.navree.client.platform

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import java.io.File
import java.security.KeyStore
import java.security.MessageDigest
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/**
 * What the core keeps between launches: files in the app's no-backup folder
 * (never in a cloud backup or a device transfer), each sealed with AES-GCM
 * under a key the Android Keystore holds and never lets out. Keys are opaque
 * to the client; each is a file named by its SHA-256.
 *
 * The core empties it when the person changes or signs out (`cache_clear`).
 */
class SealedCache(context: Context) {
    private val folder = File(context.noBackupFilesDir, "navree-cache")

    fun read(key: String): String? = try {
        val file = file(key)
        if (!file.exists()) {
            null
        } else {
            val bytes = file.readBytes()
            val cipher = Cipher.getInstance(TRANSFORMATION)
            cipher.init(Cipher.DECRYPT_MODE, key(), GCMParameterSpec(128, bytes, 0, IV_BYTES))
            cipher.doFinal(bytes, IV_BYTES, bytes.size - IV_BYTES).decodeToString()
        }
    } catch (_: Exception) {
        // A file that does not open (the key was replaced, the file was cut short) is as good as none.
        file(key).delete()
        null
    }

    fun write(key: String, value: String?) {
        val file = file(key)
        if (value == null) {
            file.delete()
            return
        }
        try {
            folder.mkdirs()
            val cipher = Cipher.getInstance(TRANSFORMATION)
            cipher.init(Cipher.ENCRYPT_MODE, key())
            val sealed = cipher.iv + cipher.doFinal(value.encodeToByteArray())
            val next = File(folder, file.name + ".new")
            next.writeBytes(sealed)
            next.renameTo(file)
        } catch (_: Exception) {
            file.delete()
        }
    }

    fun clear() {
        folder.listFiles()?.forEach { it.delete() }
    }

    private fun file(key: String): File {
        val digest = MessageDigest.getInstance("SHA-256").digest(key.encodeToByteArray())
        return File(folder, digest.joinToString("") { "%02x".format(it) })
    }

    private fun key(): SecretKey {
        val store = KeyStore.getInstance(KEYSTORE).apply { load(null) }
        (store.getKey(ALIAS, null) as? SecretKey)?.let { return it }
        val generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, KEYSTORE)
        generator.init(
            KeyGenParameterSpec.Builder(ALIAS, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setKeySize(256)
                .build(),
        )
        return generator.generateKey()
    }

    private companion object {
        const val KEYSTORE = "AndroidKeyStore"
        const val ALIAS = "navree.cache"
        const val TRANSFORMATION = "AES/GCM/NoPadding"
        const val IV_BYTES = 12
    }
}
