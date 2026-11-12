package com.navree.client.protocol

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

/** What an element does when it is used. */
@Serializable(with = ActionSerializer::class)
sealed interface Action

@Serializable
enum class Present {
    @SerialName("push") Push,
    @SerialName("replace") Replace,
    @SerialName("root") Root,
    @SerialName("sheet") Sheet,
    @SerialName("full") Full,
    @SerialName("external") External,
}

/** A name and a value a request carries. */
@Serializable
data class NameValue(
    val name: String = "",
    @Serializable(with = LooseString::class) val value: String? = null,
)

/**
 * Goes to `href`. Without `present` the shell's routes decide, and then it is
 * a push. `replace` stays on this screen and takes the answer in its place.
 */
@Serializable
data class Navigate(
    val href: String = "",
    val present: Present? = null,
) : Action

/**
 * Sends a form: the one named, or the one the button stands in. `href` sends
 * it somewhere other than its own action, and `with` adds the button's own
 * pair, as a named submit button does on the web.
 */
@Serializable
data class Submit(
    val form: String? = null,
    val href: String? = null,
    val with: NameValue? = null,
) : Action

/** A button that asks the server by itself. With no method it is a `get`. */
@Serializable
data class Request(
    val method: String = "get",
    val href: String = "",
    val fields: List<NameValue> = emptyList(),
) : Action

/** Changes local state with no request. */
@Serializable
data class Commands(
    val list: List<Command> = emptyList(),
) : Action

@Serializable
data object Back : Action

@Serializable
data object Refresh : Action

@Serializable
data object Dismiss : Action

/** An action from a newer vocabulary. It does nothing. */
@Serializable
data class UnknownAction(
    @SerialName("t") val kind: String = "",
) : Action

/**
 * One command of the closed list: `{"c":"open","id":"details"}`. The states a
 * command may set are named, never free text: `open`, `hidden`, `checked`,
 * `selected`, `disabled`, `active`, `invalid`, `pending`.
 */
@Serializable(with = CommandSerializer::class)
sealed interface Command {
    data class Toggle(val id: String) : Command
    data class Show(val id: String) : Command
    data class Hide(val id: String) : Command
    data class Open(val id: String) : Command
    data class Close(val id: String) : Command
    data class Copy(val text: String) : Command
    data class Set(val id: String, val state: String, val on: Boolean) : Command
    data class Flip(val id: String, val state: String) : Command
    data class Focus(val id: String) : Command
    data class SubmitForm(val id: String) : Command
    data class Text(val id: String, val text: String) : Command
    data class Count(val id: String, val max: Int?) : Command
    data class Value(val id: String, val value: String) : Command
    data class Toast(val tone: Tone, val text: String) : Command
    data class Unknown(val name: String) : Command
}
