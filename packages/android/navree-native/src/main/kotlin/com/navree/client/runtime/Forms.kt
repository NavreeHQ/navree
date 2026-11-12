package com.navree.client.runtime

import com.navree.client.protocol.ChoiceInput
import com.navree.client.protocol.ChoicesInput
import com.navree.client.protocol.ColourInput
import com.navree.client.protocol.DateInput
import com.navree.client.protocol.FieldNode
import com.navree.client.protocol.FileInput
import com.navree.client.protocol.FormNode
import com.navree.client.protocol.NameValue
import com.navree.client.protocol.SliderInput
import com.navree.client.protocol.TextInput
import com.navree.client.protocol.ToggleInput
import com.navree.client.protocol.UnknownInput
import java.io.ByteArrayOutputStream
import java.net.URLEncoder

/** What the user has put in a field, when it differs from what the tree holds. */
sealed interface FieldValue {
    data class Text(val text: String) : FieldValue
    data class Flag(val on: Boolean) : FieldValue
    data class Many(val values: List<String>) : FieldValue
    data class Number(val number: Double) : FieldValue
    data class Files(val files: List<PickedFile>) : FieldValue
}

/** A file the user picked, read into memory when it was picked. */
class PickedFile(val name: String, val type: String, val bytes: ByteArray) {
    override fun equals(other: Any?): Boolean =
        other is PickedFile && other.name == name && other.type == type && other.bytes.contentEquals(bytes)

    override fun hashCode(): Int = 31 * name.hashCode() + bytes.contentHashCode()
}

/** One thing a form sends. */
sealed interface Part {
    val name: String

    data class Text(override val name: String, val value: String) : Part

    data class File(override val name: String, val file: PickedFile) : Part
}

/** The body of a post, with the type that says how to read it. */
class Payload(val contentType: String, val bytes: ByteArray) {
    override fun equals(other: Any?): Boolean =
        other is Payload && other.contentType == contentType && other.bytes.contentEquals(bytes)

    override fun hashCode(): Int = 31 * contentType.hashCode() + bytes.contentHashCode()

    override fun toString(): String = "Payload($contentType, ${bytes.size} bytes)"
}

/** Turns a form and what the user typed into what a browser would post. */
object Forms {
    const val URLENCODED = "application/x-www-form-urlencoded"
    const val MULTIPART = "multipart/form-data"

    /**
     * What the form sends, in document order: its hidden values, then each
     * field as an HTML form would send it, then the pair of the button that
     * sent it. A switch that is off and a disabled field send nothing.
     * `hidden` holds values a command set on hidden fields, by the hidden
     * field's id.
     */
    fun collect(
        form: FormNode,
        values: Map<String, FieldValue> = emptyMap(),
        hidden: Map<String, String> = emptyMap(),
        with: NameValue? = null,
    ): List<Part> {
        val parts = mutableListOf<Part>()
        for (field in form.hidden) {
            if (field.name.isEmpty()) continue
            parts += Part.Text(field.name, field.id?.let(hidden::get) ?: field.value ?: "")
        }
        for (field in form.fields()) {
            if (!field.disabled && field.name.isNotEmpty()) parts += field(field, values[field.name])
        }
        if (with != null && with.name.isNotEmpty()) parts += Part.Text(with.name, with.value ?: "")
        return parts
    }

    private fun field(field: FieldNode, edit: FieldValue?): List<Part> {
        val name = field.name
        return when (val input = field.input) {
            is TextInput -> listOf(Part.Text(name, (edit as? FieldValue.Text)?.text ?: input.value ?: ""))
            is ToggleInput -> {
                val on = (edit as? FieldValue.Flag)?.on ?: input.on
                if (on) listOf(Part.Text(name, input.value ?: "on")) else emptyList()
            }
            is ChoiceInput -> {
                val value = (edit as? FieldValue.Text)?.text ?: input.value
                if (value != null) listOf(Part.Text(name, value)) else emptyList()
            }
            is ChoicesInput -> ((edit as? FieldValue.Many)?.values ?: input.values).map { Part.Text(name, it) }
            is DateInput -> listOf(Part.Text(name, (edit as? FieldValue.Text)?.text ?: input.value ?: ""))
            is SliderInput -> {
                val shown = (edit as? FieldValue.Number)?.number?.let(::number)
                listOf(Part.Text(name, shown ?: input.value ?: input.min ?: "0"))
            }
            is ColourInput -> listOf(Part.Text(name, (edit as? FieldValue.Text)?.text ?: input.value ?: ""))
            is FileInput -> (edit as? FieldValue.Files)?.files.orEmpty().map { Part.File(name, it) }
            is UnknownInput -> emptyList()
        }
    }

    /** A number as a form writes it: `2`, not `2.0`. */
    fun number(value: Double): String =
        if (value == Math.rint(value) && !value.isInfinite() && Math.abs(value) < 1e15) {
            value.toLong().toString()
        } else {
            value.toString()
        }

    /** Whether the parts have to be sent as `multipart/form-data`: one of them is a file. */
    fun needsMultipart(parts: List<Part>): Boolean = parts.any { it is Part.File }

    /**
     * `name=value&…` as `application/x-www-form-urlencoded`: UTF-8, a space as
     * `+`, line breaks as CR LF. A file sends its name, as a browser does.
     */
    fun urlencoded(parts: List<Part>): String = parts.joinToString("&") { part ->
        val value = when (part) {
            is Part.Text -> part.value
            is Part.File -> part.file.name
        }
        encode(part.name) + "=" + encode(value)
    }

    private fun encode(text: String): String = URLEncoder.encode(lineBreaks(text), "UTF-8")

    private fun lineBreaks(text: String): String = text.replace("\r\n", "\n").replace('\r', '\n').replace("\n", "\r\n")

    /** The parts as `multipart/form-data` with this boundary. */
    fun multipart(parts: List<Part>, boundary: String): ByteArray {
        val out = ByteArrayOutputStream()
        fun write(text: String) = out.write(text.toByteArray(Charsets.UTF_8))
        for (part in parts) {
            write("--$boundary\r\n")
            when (part) {
                is Part.Text -> {
                    write("Content-Disposition: form-data; name=\"${quoted(part.name)}\"\r\n\r\n")
                    write(lineBreaks(part.value))
                }
                is Part.File -> {
                    write(
                        "Content-Disposition: form-data; name=\"${quoted(part.name)}\"; " +
                            "filename=\"${quoted(part.file.name)}\"\r\n",
                    )
                    write("Content-Type: ${part.file.type.ifEmpty { "application/octet-stream" }}\r\n\r\n")
                    out.write(part.file.bytes)
                }
            }
            write("\r\n")
        }
        write("--$boundary--\r\n")
        return out.toByteArray()
    }

    /** A name inside a quoted header value: a quote and line breaks are percent-encoded. */
    private fun quoted(text: String): String = text.replace("\"", "%22").replace("\r", "%0D").replace("\n", "%0A")

    /** The body of a post for these parts. */
    fun payload(parts: List<Part>, boundary: String): Payload =
        if (needsMultipart(parts)) {
            Payload("$MULTIPART; boundary=$boundary", multipart(parts, boundary))
        } else {
            Payload(URLENCODED, urlencoded(parts).toByteArray(Charsets.UTF_8))
        }

    /** A request action's fields as a post body. */
    fun payload(fields: List<NameValue>): Payload =
        Payload(URLENCODED, urlencoded(fields.map { Part.Text(it.name, it.value ?: "") }).toByteArray(Charsets.UTF_8))

    /** The address a `get` form goes to: its action with the fields as the query. */
    fun query(action: String, parts: List<Part>): String {
        val path = action.substringBefore('#').substringBefore('?')
        val query = urlencoded(parts)
        return if (query.isEmpty()) path else "$path?$query"
    }
}
