package com.navree.client.core

import com.navree.protocol.Navree
import kotlinx.serialization.KSerializer
import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.descriptors.SerialDescriptor
import kotlinx.serialization.encoding.Decoder
import kotlinx.serialization.encoding.Encoder
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonDecoder
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonEncoder
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonArray
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.put

/*
 * The runtime core's view state, as the client reads it: everything it draws
 * from. Read like the protocol's models: a field these do not know is passed
 * over, and a value outside a closed list is read as its field's default.
 */

/** How the core's JSON is read. */
internal val CoreJson: Json = Json {
    ignoreUnknownKeys = true
    coerceInputValues = true
    explicitNulls = false
}

/** How far the app has got in starting. */
@Serializable
enum class Stage {
    @SerialName("starting") Starting,
    @SerialName("ready") Ready,
    @SerialName("failed") Failed,
    @SerialName("upgrade") Upgrade,
}

/** Why something could not be loaded. The words for each are the client's. */
@Serializable
enum class Failure {
    @SerialName("offline") Offline,
    @SerialName("timeout") Timeout,
    @SerialName("server") Server,
    @SerialName("protocol") Protocol,
    @SerialName("config") Config,
}

/** A message of the core's own, worded by the client. */
@Serializable
enum class Notice {
    @SerialName("offline") Offline,
    @SerialName("timeout") Timeout,
    @SerialName("failed") Failed,
    @SerialName("unsaved") Unsaved,
    @SerialName("loading") Loading,
    @SerialName("copied") Copied,
    @SerialName("fields_need_attention") FieldsNeedAttention,
    @SerialName("loaded") Loaded,
    @SerialName("second_step") SecondStep,
}

/** The window's width class, as the client reported it. */
@Serializable
enum class Width {
    @SerialName("compact") Compact,
    @SerialName("medium") Medium,
    @SerialName("expanded") Expanded,
}

@Serializable
data class WindowPart(val width: Width = Width.Compact)

/** The app's own part of the chrome. */
@Serializable
data class AppPart(
    val name: String = "",
    val context: String? = null,
    val home: String = "/",
    val theme: Navree.Theme? = null,
    val user: Navree.User? = null,
)

/** A top-level destination and the screens stacked in it, the first at the bottom. */
@Serializable
data class Destination(
    val label: String = "",
    val icon: Navree.Icon? = null,
    val href: String = "",
    val badge: String? = null,
    val group: String? = null,
    val items: List<Navree.NavItem> = emptyList(),
    val stack: List<Long> = emptyList(),
)

/** Screens presented over everything else, with a stack of their own. */
@Serializable
data class Modal(
    val present: Navree.Present = Navree.Present.SHEET,
    val locked: Boolean = false,
    val stack: List<Long> = emptyList(),
)

/** Where the person is: everything about navigation, and no screen. */
@Serializable
data class NavPart(
    val stage: Stage = Stage.Starting,
    val failure: Failure? = null,
    val upgrade: Navree.Upgrade? = null,
    val destinations: List<Destination> = emptyList(),
    val selected: Int = 0,
    val modals: List<Modal> = emptyList(),
    val front: Long? = null,
    @SerialName("can_go_back") val canGoBack: Boolean = false,
    val window: WindowPart = WindowPart(),
) {
    /** Every screen some stack holds. */
    fun held(): Set<Long> = buildSet {
        for (destination in destinations) addAll(destination.stack)
        for (modal in modals) addAll(modal.stack)
    }
}

/** What a screen shows. */
@Serializable(with = ContentSerializer::class)
sealed interface Content {
    /** Nothing yet. */
    data object Blank : Content

    /** A tree of parts, ready to draw. */
    data class Native(val screen: Navree.Screen) : Content

    /** The page in the web view; load `url` again whenever `serial` moves on. */
    data class Web(val url: String, val title: String?, val why: List<String>, val serial: Long) : Content
}

object ContentSerializer : KSerializer<Content> {
    override val descriptor: SerialDescriptor = JsonElement.serializer().descriptor

    override fun deserialize(decoder: Decoder): Content {
        val input = decoder as JsonDecoder
        val fields = input.decodeJsonElement() as? JsonObject ?: return Content.Blank
        return when ((fields["t"] as? JsonPrimitive)?.contentOrNull) {
            "native" -> fields["screen"]?.let { Content.Native(input.json.decodeFromJsonElement(Navree.Screen.serializer(), it)) } ?: Content.Blank
            "web" -> Content.Web(
                url = (fields["url"] as? JsonPrimitive)?.contentOrNull ?: "",
                title = (fields["title"] as? JsonPrimitive)?.contentOrNull,
                why = (fields["why"] as? kotlinx.serialization.json.JsonArray)?.mapNotNull { (it as? JsonPrimitive)?.contentOrNull }.orEmpty(),
                serial = (fields["serial"] as? JsonPrimitive)?.contentOrNull?.toLongOrNull() ?: 0,
            )
            else -> Content.Blank
        }
    }

    override fun serialize(encoder: Encoder, value: Content) {
        val output = encoder as JsonEncoder
        output.encodeJsonElement(
            when (value) {
                Content.Blank -> buildJsonObject { put("t", "blank") }
                is Content.Native -> buildJsonObject {
                    put("t", "native")
                    put("screen", output.json.encodeToJsonElement(Navree.Screen.serializer(), value.screen))
                }
                is Content.Web -> buildJsonObject {
                    put("t", "web")
                    put("url", value.url)
                    value.title?.let { put("title", it) }
                    put("why", buildJsonArray { value.why.forEach { add(JsonPrimitive(it)) } })
                    put("serial", value.serial)
                }
            },
        )
    }
}

/** A file the person picked, kept by the client under its own `token`. */
@Serializable
data class FileRef(
    val token: String,
    val name: String,
    @SerialName("type") val mediaType: String = "",
    val size: Long = 0,
)

/** What a field holds. */
@Serializable(with = FieldValueSerializer::class)
sealed interface FieldValue {
    data class Text(val text: String) : FieldValue
    data class Flag(val on: Boolean) : FieldValue
    data class Many(val values: List<String>) : FieldValue
    data class Files(val files: List<FileRef>) : FieldValue
    /** A range of two ends; an end not picked is empty. */
    data class Range(val from: String, val to: String) : FieldValue
    data object None : FieldValue

    fun json(): JsonElement = CoreJson.encodeToJsonElement(FieldValueSerializer, this)
}

object FieldValueSerializer : KSerializer<FieldValue> {
    override val descriptor: SerialDescriptor = JsonElement.serializer().descriptor

    override fun deserialize(decoder: Decoder): FieldValue {
        val input = decoder as JsonDecoder
        val fields = input.decodeJsonElement() as? JsonObject ?: return FieldValue.None
        return when ((fields["t"] as? JsonPrimitive)?.contentOrNull) {
            "text" -> FieldValue.Text((fields["text"] as? JsonPrimitive)?.contentOrNull ?: "")
            "flag" -> FieldValue.Flag((fields["on"] as? JsonPrimitive)?.contentOrNull == "true")
            "many" -> FieldValue.Many((fields["values"] as? kotlinx.serialization.json.JsonArray)?.mapNotNull { (it as? JsonPrimitive)?.contentOrNull }.orEmpty())
            "files" -> FieldValue.Files(
                (fields["files"] as? kotlinx.serialization.json.JsonArray)?.mapNotNull {
                    runCatching { input.json.decodeFromJsonElement(FileRef.serializer(), it) }.getOrNull()
                }.orEmpty(),
            )
            "range" -> FieldValue.Range(
                (fields["from"] as? JsonPrimitive)?.contentOrNull ?: "",
                (fields["to"] as? JsonPrimitive)?.contentOrNull ?: "",
            )
            else -> FieldValue.None
        }
    }

    override fun serialize(encoder: Encoder, value: FieldValue) {
        val output = encoder as JsonEncoder
        output.encodeJsonElement(
            when (value) {
                is FieldValue.Text -> buildJsonObject {
                    put("t", "text")
                    put("text", value.text)
                }
                is FieldValue.Flag -> buildJsonObject {
                    put("t", "flag")
                    put("on", value.on)
                }
                is FieldValue.Many -> buildJsonObject {
                    put("t", "many")
                    put("values", buildJsonArray { value.values.forEach { add(JsonPrimitive(it)) } })
                }
                is FieldValue.Files -> buildJsonObject {
                    put("t", "files")
                    put("files", buildJsonArray { value.files.forEach { add(output.json.encodeToJsonElement(FileRef.serializer(), it)) } })
                }
                is FieldValue.Range -> buildJsonObject {
                    put("t", "range")
                    put("from", value.from)
                    put("to", value.to)
                }
                FieldValue.None -> buildJsonObject { put("t", "none") }
            },
        )
    }
}

/** What one field holds and what is wrong with it, and the options a search found for it. */
@Serializable
data class FieldView(
    val name: String = "",
    val value: FieldValue = FieldValue.None,
    val errors: List<String> = emptyList(),
    /** While present, the option list in place of the input's own. */
    val options: List<Navree.ChoiceOption>? = null,
    /** The searched list goes on. */
    val more: Boolean = false,
    /** A search for options is on its way. */
    val searching: Boolean = false,
)

/** The state of one form. */
@Serializable
data class FormView(
    val pending: Boolean = false,
    val fields: Map<String, FieldView> = emptyMap(),
)

/** The state of a part that fetches itself. */
@Serializable
data class RegionView(val loading: Boolean = false, val failure: Failure? = null)

/** The state of a list or table that loads more by cursor, at its end and before its first row. */
@Serializable
data class ListView(
    val loading: Boolean = false,
    val failed: Boolean = false,
    @SerialName("loading_before") val loadingBefore: Boolean = false,
    @SerialName("failed_before") val failedBefore: Boolean = false,
)

/** The confirmation the core asks the person for, in the server's words. Every way out is an `answer`. */
@Serializable
data class Question(
    val id: Long,
    val title: String = "",
    val text: String? = null,
    val confirm: String = "",
    val cancel: String? = null,
)

/** A request to move the focus; answered with `focused`. */
@Serializable
data class Focus(val target: String, val serial: Long)

/** One screen. */
@Serializable
data class ScreenView(
    val id: Long,
    val url: String = "",
    val content: Content = Content.Blank,
    val loading: Boolean = false,
    val refreshing: Boolean = false,
    val cached: Boolean = false,
    val failure: Failure? = null,
    val status: Int = 0,
    val chrome: Boolean = true,
    val overlays: List<String> = emptyList(),
    val states: Map<String, List<Navree.State>> = emptyMap(),
    val forms: Map<String, FormView> = emptyMap(),
    val regions: Map<String, RegionView> = emptyMap(),
    val lists: Map<String, ListView> = emptyMap(),
    val focus: Focus? = null,
    val question: Question? = null,
) {
    val screen: Navree.Screen? get() = (content as? Content.Native)?.screen

    /** Whether a node with this id is hidden by a state the core keeps for it. */
    fun hidden(id: String?): Boolean = id != null && states[id]?.contains(Navree.State.HIDDEN) == true

    /** Whether a node with this id has the state. */
    fun has(id: String?, state: Navree.State): Boolean = id != null && states[id]?.contains(state) == true

    /** Whether an overlay of the tree is open. */
    fun open(id: String?): Boolean = id != null && id in overlays

    fun field(form: String?, field: String?): FieldView? = field?.let { forms[form ?: ""]?.fields?.get(it) }
}

/** A short message waiting to be shown. */
@Serializable
data class Message(
    val id: Long,
    val tone: Navree.Tone = Navree.Tone.NEUTRAL,
    val text: String? = null,
    val title: String? = null,
    val notice: Notice? = null,
    val action: String? = null,
)

internal fun JsonElement.stringAt(name: String): String? = (this as? JsonObject)?.get(name)?.jsonPrimitive?.contentOrNull
