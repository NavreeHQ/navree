package com.navree.client.runtime

import com.navree.client.protocol.Action
import com.navree.client.protocol.ButtonNode

/** A request to the app's origin. `url` is a path and query; the transport adds the rest. */
data class HttpRequest(
    val method: String,
    val url: String,
    val body: Payload? = null,
)

/** What came back. */
sealed interface HttpResponse {
    /**
     * An answer from the origin, after any redirects. `url` is where it came
     * from in the end, as a path and query.
     */
    data class Answer(
        val url: String,
        val status: Int,
        val contentType: String?,
        val body: String,
    ) : HttpResponse

    /** A redirect led to another site. Nothing was fetched from it. */
    data class Left(val url: String) : HttpResponse

    /** No answer. */
    data class Failed(val failure: Failure) : HttpResponse
}

/**
 * How requests reach the app: over HTTPS to a server, or by a call into the
 * app when it runs inside the client. Nothing above this knows which.
 */
fun interface Transport {
    suspend fun send(request: HttpRequest): HttpResponse
}

/** Something that happened. The reducer turns the state and an event into the next state. */
sealed interface Event {
    /** The app started, or the user asked to try the shell again. */
    data object Started : Event

    /** A request the reducer asked for was answered. */
    data class Answered(val request: Long, val response: HttpResponse) : Event

    /** A top-level destination was chosen. Choosing the one in front goes back to its first screen. */
    data class DestinationChosen(val index: Int) : Event

    /** An element's action ran: a row was tapped, a link followed. */
    data class Acted(val entry: Long, val action: Action, val source: String? = null) : Event

    /**
     * A button was pressed. A destructive one that would change something on
     * the server asks first, unless `confirmed` says the user already agreed
     * (the button of a dialog is its own confirmation).
     */
    data class Pressed(val entry: Long, val button: ButtonNode, val confirmed: Boolean = false) : Event

    /** The user answered the question a destructive button asked. */
    data class ConfirmAnswered(val entry: Long, val accepted: Boolean) : Event

    /** The system's back. */
    data object BackPressed : Event

    /** The user took a presented screen away: swiped a sheet down, tapped outside it. */
    data class ModalDismissed(val entry: Long) : Event

    /** The user closed a sheet, a dialog or a disclosure of the tree, or opened a disclosure. */
    data class OverlayToggled(val entry: Long, val id: String, val open: Boolean) : Event

    /** The user changed a field. */
    data class FieldChanged(val entry: Long, val form: String?, val name: String, val value: FieldValue) : Event

    /** The user typed in the search field. Nothing is fetched until [SearchRun]. */
    data class SearchChanged(val entry: Long, val query: String) : Event

    /** The typed query is to be asked for now: typing paused, or the keyboard's search key. */
    data class SearchRun(val entry: Long) : Event

    /** The user pulled to refresh, or chose to try again after a failure. */
    data class Refreshed(val entry: Long) : Event

    /** A list was scrolled near its end. */
    data class EndReached(val entry: Long, val list: String) : Event

    /** A message was shown and went away. */
    data class MessageShown(val id: Long) : Event

    /** The action of a message was used. */
    data class MessageActed(val id: Long) : Event

    /** A link was tapped in the web view. */
    data class LinkFollowed(val entry: Long, val url: String) : Event

    /** The app was opened with an address: a deep link, a notification. */
    data class Opened(val url: String) : Event

    /** The focus request was carried out. */
    data class FocusTaken(val entry: Long, val serial: Long) : Event
}

/** Something the reducer wants done. It never does anything itself. */
sealed interface Effect {
    /** Send this and come back with [Event.Answered] carrying the same number. */
    data class Fetch(val request: Long, val call: HttpRequest) : Effect

    /** Open this address outside the app: the browser, the dialler, the mail app. */
    data class OpenOutside(val url: String) : Effect

    /** Put this text on the clipboard. */
    data class Copy(val text: String) : Effect

    /** There is nothing to go back to: leave the app to the system. */
    data object Leave : Effect
}

/** The next state and what to do about it. */
data class Step(
    val state: ClientState,
    val effects: List<Effect> = emptyList(),
)
