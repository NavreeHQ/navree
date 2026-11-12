package com.navree.client.platform

import androidx.lifecycle.ViewModel
import androidx.lifecycle.ViewModelProvider
import androidx.lifecycle.viewModelScope
import com.navree.client.runtime.ClientState
import com.navree.client.runtime.Effect
import com.navree.client.runtime.Event
import com.navree.client.runtime.Origin
import com.navree.client.runtime.Reducer
import com.navree.client.runtime.Transport
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.receiveAsFlow
import kotlinx.coroutines.launch

/**
 * Holds the state and runs what the reducer asks for. Events come in on the
 * main thread, one at a time; requests go out through the transport and come
 * back as events. It outlives a rotation, so a screen in the middle of a
 * request is still there afterwards.
 */
class NavreeStore(
    val origin: Origin,
    val transport: Transport,
) : ViewModel() {
    private val current = MutableStateFlow(ClientState(origin))

    /** The state, as the reducer last left it. */
    val state: StateFlow<ClientState> = current.asStateFlow()

    private val outward = Channel<Effect>(Channel.UNLIMITED)

    /** The effects that need an activity: opening another app, the clipboard, leaving. */
    val effects: Flow<Effect> = outward.receiveAsFlow()

    /** Takes an event. Call it on the main thread. */
    fun dispatch(event: Event) {
        val step = Reducer.reduce(current.value, event)
        current.value = step.state
        for (effect in step.effects) perform(effect)
    }

    private fun perform(effect: Effect) {
        when (effect) {
            is Effect.Fetch -> viewModelScope.launch {
                val response = transport.send(effect.call)
                dispatch(Event.Answered(effect.request, response))
            }
            is Effect.OpenOutside, is Effect.Copy, Effect.Leave -> outward.trySend(effect)
        }
    }

    class Factory(private val origin: Origin, private val transport: Transport) : ViewModelProvider.Factory {
        @Suppress("UNCHECKED_CAST")
        override fun <T : ViewModel> create(modelClass: Class<T>): T = NavreeStore(origin, transport) as T
    }
}
