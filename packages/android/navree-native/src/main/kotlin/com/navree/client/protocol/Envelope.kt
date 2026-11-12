package com.navree.client.protocol

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

/** The media type a native client asks for and a server answers with. */
const val NATIVE_CONTENT_TYPE = "application/vnd.navree.native+json"

/** The protocol major this client speaks. A reply with another major is refused. */
const val PROTOCOL_MAJOR = 1

/** The vocabulary this client can draw. The server lowers a page for it. */
const val VOCABULARY = 1

/**
 * What a server answers: a whole screen, changes to the screen in front, a
 * page to show in the web view, or a notice that the app is too old.
 */
@Serializable
data class Envelope(
    val navree: Int = PROTOCOL_MAJOR,
    val build: String? = null,
    val vocab: Int = VOCABULARY,
    val screen: Screen? = null,
    val ops: List<Op> = emptyList(),
    val web: WebPage? = null,
    val upgrade: Upgrade? = null,
)

/** A page the server could not lower: the client shows `url` in its web view. */
@Serializable
data class WebPage(
    val url: String = "",
    val title: String? = null,
    val why: List<String> = emptyList(),
)

/** The app is too old to be served safely. */
@Serializable
data class Upgrade(
    @SerialName("min_app") val minApp: String? = null,
    val url: String? = null,
)

/** One page as a native tree. Actions carry roles, never positions. */
@Serializable
data class Screen(
    val id: String? = null,
    val title: String = "",
    val subtitle: String? = null,
    val back: BackLink? = null,
    /** The whole way to this screen, when it has more than one step. */
    val trail: List<BackLink> = emptyList(),
    /** The address of the shell's destination this screen belongs to. */
    val nav: String? = null,
    val actions: List<Node> = emptyList(),
    val search: Search? = null,
    val refresh: Boolean = false,
    val body: Node? = null,
)

/** Where "up" leads when the screen was opened directly, with no screen under it. */
@Serializable
data class BackLink(
    val title: String? = null,
    val href: String? = null,
)

/** A screen's search: the query is sent to `action` as the field `name`. */
@Serializable
data class Search(
    val name: String = "q",
    val value: String = "",
    val placeholder: String? = null,
    val action: String? = null,
    /** What goes with the query: the sort, the filters in force. */
    val hidden: List<NameValue> = emptyList(),
)
