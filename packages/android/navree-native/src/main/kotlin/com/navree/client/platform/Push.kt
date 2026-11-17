package com.navree.client.platform

import android.content.Context
import java.io.File

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
     * is deleted, so nothing sent to it reaches the device. [done] runs when
     * the service has answered, on whatever thread, and is told whether the
     * token is gone. `false` says the deletion failed: the token stays owed
     * its deletion ([OwedToken]) and the client asks again. A provider with
     * nothing to delete answers `true` at once.
     */
    fun forget(done: (gone: Boolean) -> Unit) {
        done(true)
    }
}

/**
 * The push service's token of a session that ended, as something owed: from
 * the moment the client is told the session is ending until the push service
 * has said the token is deleted. The mark is a file no backup takes, written
 * whole and forced to the disk before [owe] returns ([WipeRules.put]), so it
 * outlives the process: a run of the app that ends before the service has
 * answered leaves the deletion to the next start, and a message that arrives
 * meanwhile, in whatever process the system starts for it, is not drawn (the
 * messaging service of `navree-push-fcm` asks [owed] first).
 */
class OwedToken(private val mark: File) {
    private val lock = Any()

    /** What the mark says, once it has been read or written in this process. */
    private var known: Boolean? = null

    /** Whether the token of a session that ended is still to delete. */
    val owed: Boolean get() = synchronized(lock) { known ?: mark.exists().also { known = it } }

    /** A session is ending: its token is owed its deletion from now, and that has reached the disk when this returns. */
    fun owe() {
        synchronized(lock) {
            known = true
            runCatching { WipeRules.put(mark, "owed\n") }
        }
    }

    /** The push service said the token is deleted: nothing is owed. */
    fun gone() {
        synchronized(lock) {
            known = false
            runCatching { mark.delete() }
        }
    }

    companion object {
        private const val MARK = "navree-push-token"

        @Volatile
        private var store: OwedToken? = null

        /** The one of the app's process. */
        fun of(context: Context): OwedToken = store ?: synchronized(this) {
            store ?: OwedToken(File(context.applicationContext.noBackupFilesDir, MARK)).also { store = it }
        }
    }
}

/**
 * The push service's side of the end of a session, in the order it has to
 * happen and with nothing of the platform in it, so it can be tested without
 * a device.
 *
 * The token's deletion is owed, and written down, the moment the session is
 * ending ([owe]). The push service is asked to delete the token then and
 * there ([delete]): that needs nothing of the server, so it does not wait for
 * the server to answer the token's removal. When the service has said the
 * token is gone, the shade is emptied again, since what was sent to that
 * token and drawn while it still stood belongs to the person who left, and a
 * new token is asked for. A deletion that failed stays owed and is asked
 * again by the next [delete]: at the next start of the app, each time the app
 * comes to the front, and whenever a token is wanted. While it is owed no
 * token is asked for ([owed]): the one the service would give is the one to
 * delete. Call on the thread the client is driven on.
 */
internal class PushEnd(
    private val provider: PushProvider?,
    private val debt: OwedToken,
    /** Runs a piece of work on the thread the client is driven on: a provider answers on a thread of its own choosing. */
    private val onMain: (() -> Unit) -> Unit,
    /** Takes every notification of the app out of the shade. */
    private val emptyShade: () -> Unit,
    /** Asks the push service for this install's token and hands it to the core. */
    private val ask: () -> Unit,
) {
    /** A deletion is with the push service and has not been answered. */
    private var asked = false

    /** A session ended while a deletion was with the push service: the service is asked once more when it answers. */
    private var again = false

    init {
        // An app without a provider has no token to delete, and nothing here that could: a mark an earlier build of it left is let go.
        if (provider == null && debt.owed) debt.gone()
    }

    /** Whether the token of a session that ended is still to delete. */
    val owed: Boolean get() = provider != null && debt.owed

    /** A session is ending: its token is owed its deletion from now, on the disk when this returns. */
    fun owe() {
        if (provider == null) return
        debt.owe()
        if (asked) again = true
    }

    /** Asks the push service to delete the token, while that is owed and it is not being asked already. */
    fun delete() {
        val provider = provider ?: return
        if (!debt.owed || asked) return
        asked = true
        try {
            provider.forget { gone -> onMain { answered(gone) } }
        } catch (_: Throwable) {
            // Not asked after all: still owed, and the next call asks.
            asked = false
        }
    }

    private fun answered(gone: Boolean) {
        // A deletion is answered once: what a provider says after that is not heard.
        if (!asked) return
        asked = false
        if (again) {
            // The answer is for a deletion asked before the last session ended: only one asked after it ends what is owed.
            again = false
            return delete()
        }
        if (!gone) return
        debt.gone()
        emptyShade()
        ask()
    }
}

/**
 * The core's two words for the end of a session and what each begins, in
 * order and with nothing of the platform in it, so it can be tested without a
 * device.
 *
 * `session_ending` comes where a push token is registered, in the update in
 * which the core asks the server to remove it: the end of the session begins
 * then and does not wait for that answer, and the push service is asked to
 * delete the token at once. `forget_session` follows when the removal is
 * answered or its time has passed. It finds the end of the session begun or
 * done and begins nothing a second time; the shade was emptied when the end
 * began, which was before the server stopped sending to the token, so it is
 * emptied again now. Where there was no token to remove, `forget_session`
 * comes alone and the end of the session begins with it.
 */
internal class SessionEnd(
    /** Begins the end of the session: what it owes is written down ([PushEnd.owe] among it), and its deletion takes its turn. */
    private val begin: () -> Unit,
    private val tokens: PushEnd,
    /** Takes every notification of the app out of the shade. */
    private val emptyShade: () -> Unit,
) {
    /** The core said the session is ending and has not yet said it is forgotten: the end of the session is begun. */
    private var begun = false

    /** The core says the session is ending. */
    fun ending() {
        begin()
        begun = true
        tokens.delete()
    }

    /**
     * The core says the session is forgotten. Either way the push service is
     * asked to delete the token unless it is being asked already or has said
     * the token is gone ([PushEnd.delete]).
     */
    fun forgotten() {
        if (begun) {
            begun = false
            emptyShade()
        } else {
            begin()
        }
        tokens.delete()
    }
}

/** What a locked screen may show of a notification the app builds itself. */
enum class LockScreen {
    /**
     * The system's way (`Notification.VISIBILITY_PRIVATE`, the system's own
     * default): the person's setting decides whether the title and text show
     * on a locked screen; where it hides them, the app's name and "New
     * notification" show instead.
     */
    System,

    /** Nothing of it on a secure locked screen (`Notification.VISIBILITY_SECRET`), whatever the person's setting. */
    Hidden,
    ;

    companion object {
        /**
         * The option a resource names: empty for [System], `nothing` for
         * [Hidden]. A name this build does not know shows the least,
         * [Hidden].
         */
        fun of(name: String): LockScreen = if (name.trim().isEmpty()) System else Hidden
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
