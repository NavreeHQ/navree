package com.navree.client.platform

/**
 * Where a device's push token comes from. The client knows nothing of the
 * service behind it: an app that sends notifications adds a provider (the
 * Firebase one is in the `navree-push-fcm` module) and names it in
 * [com.navree.client.NavreeConfig.push].
 *
 * The token goes to the core, which registers it with the app's server and
 * takes it away at sign-out. The client makes no request of its own for it.
 */
interface PushProvider {
    /** The protocol's name for the service: `fcm`. */
    val provider: String

    /** Asks the service for this install's token; `done(null)` when it cannot give one. */
    fun token(done: (String?) -> Unit)

    /**
     * The session on this device ended: the service's token for this install
     * is deleted, so nothing sent to it reaches the device, and [done] runs
     * when that has finished (or failed). A provider with nothing to delete
     * answers at once.
     */
    fun forget(done: () -> Unit) {
        done()
    }
}

/**
 * Where a token the service changed while the app runs is handed on: the
 * service's callback calls [changed], and the session listening tells the core.
 */
object PushTokens {
    @Volatile
    var listener: ((String) -> Unit)? = null

    fun changed(token: String) {
        listener?.invoke(token)
    }
}
