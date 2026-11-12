package com.navree.client.protocol

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

/**
 * The control of a form's field. A client draws each with its system's own
 * control and sends the value under the field's name, as a browser's form
 * would.
 */
@Serializable(with = InputSerializer::class)
sealed interface Input

@Serializable
enum class TextKind {
    @SerialName("plain") Plain,
    @SerialName("email") Email,
    @SerialName("password") Password,
    @SerialName("number") Number,
    @SerialName("decimal") Decimal,
    @SerialName("phone") Phone,
    @SerialName("url") Url,
    @SerialName("search") Search,
    @SerialName("multiline") Multiline,
    @SerialName("one_time_code") OneTimeCode,
}

@Serializable
data class TextInput(
    val kind: TextKind = TextKind.Plain,
    @Serializable(with = LooseString::class) val value: String? = null,
    val placeholder: String? = null,
    /** The most characters it takes. */
    @SerialName("max_length") val maxLength: Int? = null,
    /** The least, the most and the step of a number. */
    @Serializable(with = LooseString::class) val min: String? = null,
    @Serializable(with = LooseString::class) val max: String? = null,
    @Serializable(with = LooseString::class) val step: String? = null,
    /** What the system may fill in: `name`, `email`, `tel` and the rest of the web's list. */
    val autofill: String? = null,
    val readonly: Boolean = false,
) : Input

@Serializable
enum class ToggleKind {
    @SerialName("switch") Switch,
    @SerialName("checkbox") Checkbox,
}

@Serializable
data class ToggleInput(
    val kind: ToggleKind = ToggleKind.Switch,
    val on: Boolean = false,
    /** What is sent when it is on (`on` when there is none). Nothing is sent when it is off. */
    @Serializable(with = LooseString::class) val value: String? = null,
) : Input

@Serializable
data class Option(
    @Serializable(with = LooseString::class) val value: String? = null,
    val label: String = "",
    /** A few words beside the label: how many records the choice holds. */
    val detail: String? = null,
    val disabled: Boolean = false,
)

@Serializable
enum class ChoiceKind {
    @SerialName("menu") Menu,
    @SerialName("segmented") Segmented,
    @SerialName("radio") Radio,
    @SerialName("list") List,
}

@Serializable
data class ChoiceInput(
    val kind: ChoiceKind = ChoiceKind.Menu,
    val options: List<Option> = emptyList(),
    @Serializable(with = LooseString::class) val value: String? = null,
    val placeholder: String? = null,
) : Input

@Serializable
data class ChoicesInput(
    val options: List<Option> = emptyList(),
    @Serializable(with = LooseStringList::class) val values: List<String> = emptyList(),
) : Input

@Serializable
enum class DateKind {
    @SerialName("date") Date,
    @SerialName("time") Time,
    @SerialName("datetime") DateTime,
}

/** Values are ISO 8601: `2026-11-12`, `14:30`, `2026-11-12T14:30`. */
@Serializable
data class DateInput(
    val kind: DateKind = DateKind.Date,
    val value: String? = null,
    val min: String? = null,
    val max: String? = null,
) : Input

@Serializable
data class SliderInput(
    @Serializable(with = LooseString::class) val min: String? = null,
    @Serializable(with = LooseString::class) val max: String? = null,
    @Serializable(with = LooseString::class) val step: String? = null,
    @Serializable(with = LooseString::class) val value: String? = null,
) : Input

@Serializable
data class ColourInput(
    val value: String? = null,
) : Input

@Serializable
data class FileInput(
    val accept: String? = null,
    val multiple: Boolean = false,
    val capture: String? = null,
) : Input

/** A kind of input from a newer vocabulary. */
@Serializable
data class UnknownInput(
    @SerialName("t") val kind: String = "",
) : Input
