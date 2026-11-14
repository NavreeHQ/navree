package com.navree.client.platform

import android.graphics.Bitmap
import android.graphics.BitmapFactory

/**
 * The most bytes a picture may have: 10 MiB, more than a photograph a page
 * shows needs. A larger one is not loaded and its place keeps the placeholder.
 */
const val MOST_PICTURE_BYTES: Long = 10L * 1024 * 1024

/**
 * The longest side, in pixels, a picture is decoded at: 2048, sharp on any
 * phone or tablet screen at the sizes a page draws pictures, and at most
 * 16 MiB of pixels in memory.
 */
const val MOST_PICTURE_SIDE: Int = 2048

/**
 * The power of two a picture of this size is sampled down by so that neither
 * side is longer than [most], or 0 when the size could not be read.
 */
fun sampleSize(width: Int, height: Int, most: Int = MOST_PICTURE_SIDE): Int {
    if (width <= 0 || height <= 0 || most <= 0) return 0
    var size = 1
    while (width / size > most || height / size > most) size *= 2
    return size
}

/**
 * A picture decoded from its bytes, sampled down to [MOST_PICTURE_SIDE], or
 * null when it does not decode for any reason, running out of memory
 * included: the caller then keeps the placeholder.
 */
fun decodePicture(bytes: ByteArray): Bitmap? = try {
    val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
    BitmapFactory.decodeByteArray(bytes, 0, bytes.size, bounds)
    val sample = sampleSize(bounds.outWidth, bounds.outHeight)
    if (sample == 0) null else BitmapFactory.decodeByteArray(bytes, 0, bytes.size, BitmapFactory.Options().apply { inSampleSize = sample })
} catch (_: OutOfMemoryError) {
    null
} catch (_: Exception) {
    null
}
