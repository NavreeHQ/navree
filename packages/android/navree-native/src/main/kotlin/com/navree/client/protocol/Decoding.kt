package com.navree.client.protocol

import kotlinx.serialization.DeserializationStrategy
import kotlinx.serialization.KSerializer
import kotlinx.serialization.SerializationException
import kotlinx.serialization.builtins.ListSerializer
import kotlinx.serialization.builtins.serializer
import kotlinx.serialization.descriptors.PrimitiveKind
import kotlinx.serialization.descriptors.PrimitiveSerialDescriptor
import kotlinx.serialization.descriptors.SerialDescriptor
import kotlinx.serialization.descriptors.buildClassSerialDescriptor
import kotlinx.serialization.encoding.Decoder
import kotlinx.serialization.encoding.Encoder
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonContentPolymorphicSerializer
import kotlinx.serialization.json.JsonDecoder
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.booleanOrNull

/**
 * How every document of the protocol is read: a field this client does not
 * know is skipped, and a value it does not know (a new tone, a new role)
 * becomes the field's default.
 */
val NavreeJson: Json = Json {
    ignoreUnknownKeys = true
    coerceInputValues = true
    explicitNulls = false
}

/** Reads an [Envelope]. Throws [SerializationException] when the text is not one. */
fun decodeEnvelope(text: String): Envelope = NavreeJson.decodeFromString(Envelope.serializer(), text)

/** Reads a [Shell]. Throws [SerializationException] when the text is not one. */
fun decodeShell(text: String): Shell = NavreeJson.decodeFromString(Shell.serializer(), text)

private fun JsonElement.kind(key: String = "t"): String? =
    ((this as? JsonObject)?.get(key) as? JsonPrimitive)?.takeIf { it.isString }?.content

private fun JsonObject.string(vararg keys: String): String? {
    for (key in keys) {
        val value = this[key]
        if (value is JsonPrimitive && value !is JsonNull) return value.content
    }
    return null
}

private fun Decoder.element(): JsonElement =
    (this as? JsonDecoder)?.decodeJsonElement()
        ?: throw SerializationException("The protocol is read from JSON only")

private fun readOnly(): Nothing = throw SerializationException("The protocol models are read, never written")

/** Picks the node class from `t`. A bare string is a body text. */
object NodeSerializer : JsonContentPolymorphicSerializer<Node>(Node::class) {
    override fun selectDeserializer(element: JsonElement): DeserializationStrategy<Node> {
        if (element !is JsonObject) return BareNode
        return when (element.kind()) {
            "stack" -> StackNode.serializer()
            "grid" -> GridNode.serializer()
            "section" -> SectionNode.serializer()
            "list" -> ListNode.serializer()
            "row" -> RowNode.serializer()
            "split" -> SplitNode.serializer()
            "tabs" -> TabsNode.serializer()
            "disclosure" -> DisclosureNode.serializer()
            "divider" -> DividerNode.serializer()
            "spacer" -> SpacerNode.serializer()
            "region" -> RegionNode.serializer()
            "text" -> TextNode.serializer()
            "rich" -> RichNode.serializer()
            "image" -> ImageNode.serializer()
            "icon" -> IconNode.serializer()
            "avatar" -> AvatarNode.serializer()
            "badge" -> BadgeNode.serializer()
            "progress" -> ProgressNode.serializer()
            "banner" -> BannerNode.serializer()
            "empty" -> EmptyNode.serializer()
            "facts" -> FactsNode.serializer()
            "table" -> TableNode.serializer()
            "chart" -> ChartNode.serializer()
            "code" -> CodeNode.serializer()
            "button" -> ButtonNode.serializer()
            "link" -> LinkNode.serializer()
            "menu" -> MenuNode.serializer()
            "form" -> FormNode.serializer()
            "field" -> FieldNode.serializer()
            "sheet" -> SheetNode.serializer()
            "dialog" -> DialogNode.serializer()
            "popover" -> PopoverNode.serializer()
            "native" -> NativeNode.serializer()
            "web" -> WebNode.serializer()
            else -> UnknownNode.serializer()
        }
    }
}

/** A node written as a bare value: a string or a number is a text, anything else is unknown. */
private object BareNode : KSerializer<Node> {
    override val descriptor: SerialDescriptor = buildClassSerialDescriptor("com.navree.client.protocol.BareNode")

    override fun deserialize(decoder: Decoder): Node {
        val element = decoder.element()
        return if (element is JsonPrimitive && element !is JsonNull) {
            TextNode(value = element.content)
        } else {
            UnknownNode()
        }
    }

    override fun serialize(encoder: Encoder, value: Node) = readOnly()
}

/** Picks the input class from `t`. */
object InputSerializer : JsonContentPolymorphicSerializer<Input>(Input::class) {
    override fun selectDeserializer(element: JsonElement): DeserializationStrategy<Input> {
        if (element !is JsonObject) return BareInput
        return when (element.kind()) {
            "text" -> TextInput.serializer()
            "toggle" -> ToggleInput.serializer()
            "choice" -> ChoiceInput.serializer()
            "choices" -> ChoicesInput.serializer()
            "date" -> DateInput.serializer()
            "slider" -> SliderInput.serializer()
            "colour", "color" -> ColourInput.serializer()
            "file" -> FileInput.serializer()
            else -> UnknownInput.serializer()
        }
    }
}

private object BareInput : KSerializer<Input> {
    override val descriptor: SerialDescriptor = buildClassSerialDescriptor("com.navree.client.protocol.BareInput")

    override fun deserialize(decoder: Decoder): Input {
        decoder.element()
        return UnknownInput()
    }

    override fun serialize(encoder: Encoder, value: Input) = readOnly()
}

/** Picks the action class from `t`. */
object ActionSerializer : JsonContentPolymorphicSerializer<Action>(Action::class) {
    override fun selectDeserializer(element: JsonElement): DeserializationStrategy<Action> {
        if (element !is JsonObject) return BareAction
        return when (element.kind()) {
            "navigate" -> Navigate.serializer()
            "submit" -> Submit.serializer()
            "request" -> Request.serializer()
            "commands" -> Commands.serializer()
            "back" -> Back.serializer()
            "refresh" -> Refresh.serializer()
            "dismiss" -> Dismiss.serializer()
            else -> UnknownAction.serializer()
        }
    }
}

private object BareAction : KSerializer<Action> {
    override val descriptor: SerialDescriptor = buildClassSerialDescriptor("com.navree.client.protocol.BareAction")

    override fun deserialize(decoder: Decoder): Action {
        decoder.element()
        return UnknownAction()
    }

    override fun serialize(encoder: Encoder, value: Action) = readOnly()
}

private fun tone(name: String?): Tone = when (name) {
    "accent" -> Tone.Accent
    "success" -> Tone.Success
    "warning" -> Tone.Warning
    "danger" -> Tone.Danger
    "info" -> Tone.Info
    else -> Tone.Neutral
}

/**
 * Reads a command: an object named by `c` (`{"c":"open","id":"details"}`), or
 * the array the browser runtime takes (`["open","details"]`).
 */
object CommandSerializer : KSerializer<Command> {
    override val descriptor: SerialDescriptor = buildClassSerialDescriptor("com.navree.client.protocol.Command")

    override fun deserialize(decoder: Decoder): Command = when (val element = decoder.element()) {
        is JsonObject -> fromObject(element)
        is JsonArray -> fromArray(element.map { (it as? JsonPrimitive)?.takeIf { part -> part !is JsonNull }?.content })
        else -> Command.Unknown("")
    }

    private fun fromObject(element: JsonObject): Command {
        val name = element.string("c", "t") ?: return Command.Unknown("")
        val id = element.string("id", "target") ?: ""
        return when (name) {
            "toggle" -> Command.Toggle(id)
            "show" -> Command.Show(id)
            "hide" -> Command.Hide(id)
            "open" -> Command.Open(id)
            "close" -> Command.Close(id)
            "copy" -> Command.Copy(element.string("text") ?: "")
            "set" -> Command.Set(id, element.string("state") ?: "", (element["on"] as? JsonPrimitive)?.booleanOrNull ?: false)
            "flip" -> Command.Flip(id, element.string("state") ?: "")
            "focus" -> Command.Focus(id)
            "submit" -> Command.SubmitForm(id)
            "text" -> Command.Text(id, element.string("text") ?: "")
            "count" -> Command.Count(id, element.string("max")?.toIntOrNull())
            "value" -> Command.Value(id, element.string("value") ?: "")
            "toast" -> Command.Toast(tone(element.string("tone")), element.string("text") ?: "")
            else -> Command.Unknown(name)
        }
    }

    private fun fromArray(parts: List<String?>): Command {
        val name = parts.getOrNull(0) ?: return Command.Unknown("")
        fun arg(index: Int): String = parts.getOrNull(index) ?: ""
        return when (name) {
            "toggle" -> Command.Toggle(arg(1))
            "show" -> Command.Show(arg(1))
            "hide" -> Command.Hide(arg(1))
            "open" -> Command.Open(arg(1))
            "close" -> Command.Close(arg(1))
            "copy" -> Command.Copy(arg(1))
            "focus" -> Command.Focus(arg(1))
            "submit" -> Command.SubmitForm(arg(1))
            "text" -> Command.Text(arg(1), arg(2))
            "count" -> Command.Count(arg(1), parts.getOrNull(2)?.toIntOrNull())
            "value" -> Command.Value(arg(1), arg(2))
            "toast" -> Command.Toast(tone(parts.getOrNull(1)), arg(2))
            else -> Command.Unknown(name)
        }
    }

    override fun serialize(encoder: Encoder, value: Command) = readOnly()
}

/** Reads an op. The name is in `op`, as the browser runtime has it. */
object OpSerializer : KSerializer<Op> {
    override val descriptor: SerialDescriptor = buildClassSerialDescriptor("com.navree.client.protocol.Op")

    override fun deserialize(decoder: Decoder): Op {
        val json = (decoder as? JsonDecoder)?.json ?: throw SerializationException("The protocol is read from JSON only")
        val element = decoder.element() as? JsonObject ?: return Op.Unknown("")
        val name = element.string("op") ?: return Op.Unknown("")
        val target = element.string("target", "id") ?: ""
        fun node(): Node = element["node"]?.let { json.decodeFromJsonElement(NodeSerializer, it) } ?: UnknownNode()
        fun nodes(): List<Node> = when (val raw = element["nodes"]) {
            is JsonArray -> raw.map { json.decodeFromJsonElement(NodeSerializer, it) }
            // One node where a list is meant is a list of one.
            null -> element["node"]?.let { listOf(json.decodeFromJsonElement(NodeSerializer, it)) }.orEmpty()
            else -> emptyList()
        }
        return when (name) {
            "morph" -> Op.Morph(target, node())
            "replace" -> Op.Replace(target, node())
            "append" -> Op.Append(target, nodes(), element.string("more"))
            "prepend" -> Op.Prepend(target, nodes())
            "remove" -> Op.Remove(target)
            "errors" -> Op.Errors(element.string("form") ?: target, fields(element["fields"]))
            "toast" -> Op.Toast(
                tone = tone(element.string("tone")),
                text = element.string("text") ?: "",
                title = element.string("title"),
                id = element.string("id"),
                action = (element["action"] as? JsonObject)?.let { action ->
                    ToastAction(
                        label = action.string("label") ?: "",
                        href = action.string("href") ?: "",
                        post = (action["post"] as? JsonPrimitive)?.booleanOrNull ?: false,
                    )
                },
            )
            "open" -> Op.Open(target)
            "close" -> Op.Close(target)
            "focus" -> Op.Focus(target)
            "url" -> Op.Url(element.string("href") ?: "", element.string("mode") == "replace")
            "navigate" -> Op.Navigate(element.string("href") ?: "")
            "navigate_external" -> Op.NavigateExternal(element.string("href") ?: "")
            "reload" -> Op.Reload
            "invalidate" -> Op.Invalidate(target)
            else -> Op.Unknown(name)
        }
    }

    private fun fields(element: JsonElement?): Map<String, List<String>> {
        val fields = element as? JsonObject ?: return emptyMap()
        return fields.mapValues { (_, messages) -> strings(messages) }
    }

    override fun serialize(encoder: Encoder, value: Op) = readOnly()
}

private fun strings(element: JsonElement?): List<String> = when (element) {
    null, is JsonNull -> emptyList()
    is JsonPrimitive -> listOf(element.content)
    is JsonArray -> element.mapNotNull { (it as? JsonPrimitive)?.takeIf { part -> part !is JsonNull }?.content }
    else -> emptyList()
}

/** A string that may arrive as a number or a boolean: a count on a badge, an option's value. */
object LooseString : KSerializer<String> {
    override val descriptor: SerialDescriptor =
        PrimitiveSerialDescriptor("com.navree.client.protocol.LooseString", PrimitiveKind.STRING)

    override fun deserialize(decoder: Decoder): String = when (val element = decoder.element()) {
        is JsonPrimitive -> element.content
        else -> element.toString()
    }

    override fun serialize(encoder: Encoder, value: String) = readOnly()
}

/** A list of strings that may arrive as one string: a field's error, a set of chosen values. */
object LooseStringList : KSerializer<List<String>> {
    override val descriptor: SerialDescriptor = ListSerializer(String.serializer()).descriptor

    override fun deserialize(decoder: Decoder): List<String> = strings(decoder.element())

    override fun serialize(encoder: Encoder, value: List<String>) = readOnly()
}

/**
 * Hidden fields as a list of `{name, value, id}`, in order and with repeats,
 * or as an object of names to values.
 */
object HiddenFields : KSerializer<List<Hidden>> {
    override val descriptor: SerialDescriptor = ListSerializer(Hidden.serializer()).descriptor

    override fun deserialize(decoder: Decoder): List<Hidden> = when (val element = decoder.element()) {
        is JsonObject -> element.map { (name, value) -> Hidden(name, text(value)) }
        is JsonArray -> element.mapNotNull { item ->
            (item as? JsonObject)?.let { Hidden(it.string("name") ?: "", text(it["value"]), it.string("id")) }
        }
        else -> emptyList()
    }

    private fun text(element: JsonElement?): String? = when (element) {
        null, is JsonNull -> null
        is JsonPrimitive -> element.content
        else -> element.toString()
    }

    override fun serialize(encoder: Encoder, value: List<Hidden>) = readOnly()
}
