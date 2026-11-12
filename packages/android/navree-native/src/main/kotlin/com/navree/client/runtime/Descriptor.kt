package com.navree.client.runtime

import com.navree.client.protocol.NATIVE_CONTENT_TYPE
import com.navree.client.protocol.PROTOCOL_MAJOR
import com.navree.client.protocol.VOCABULARY

/**
 * What the client tells the server about itself on every request, so a page
 * can be lowered for this system, form factor and vocabulary.
 */
data class ClientDescriptor(
    val os: String,
    /** `phone` or `tablet`. */
    val form: String,
    val app: String,
    val dark: Boolean,
    val highContrast: Boolean,
    val textScale: Float,
    val locale: String,
    val timeZone: String,
    val caps: List<String> = emptyList(),
) {
    /** The value of `X-Navree-Client`: `key=value` pairs, separated by semicolons. */
    fun header(): String = listOf(
        "platform" to "android",
        "os" to token(os),
        "form" to token(form),
        "app" to token(app),
        "vocab" to VOCABULARY.toString(),
        "scheme" to if (dark) "dark" else "light",
        "contrast" to if (highContrast) "more" else "normal",
        "text" to scale(textScale),
        "locale" to token(locale),
        "tz" to token(timeZone),
        "caps" to caps.joinToString("+") { token(it) },
    ).joinToString("; ") { (key, value) -> "$key=$value" }

    /** Every header a native request carries besides its cookies. */
    fun headers(): Map<String, String> = mapOf(
        "X-Navree-Native" to PROTOCOL_MAJOR.toString(),
        "X-Navree-Client" to header(),
        "Accept" to NATIVE_CONTENT_TYPE,
    )

    private fun scale(value: Float): String {
        val rounded = Math.round(value * 100f) / 100f
        return if (rounded == Math.rint(rounded.toDouble()).toFloat()) "${rounded.toInt()}.0" else rounded.toString()
    }

    /** A value may hold only what cannot be mistaken for the header's own punctuation. */
    private fun token(text: String): String = text.filter { it.isLetterOrDigit() || it in "._-/+" }
}

/** The header that asks for a page without the HTML shell, for the web view inside native chrome. */
const val EMBED_HEADER = "X-Navree-Embed"
