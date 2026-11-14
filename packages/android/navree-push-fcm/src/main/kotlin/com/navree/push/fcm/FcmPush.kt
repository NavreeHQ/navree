package com.navree.push.fcm

import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import com.google.firebase.FirebaseApp
import com.google.firebase.FirebaseOptions
import com.google.firebase.messaging.FirebaseMessaging
import com.google.firebase.messaging.FirebaseMessagingService
import com.google.firebase.messaging.RemoteMessage
import com.navree.client.NavreeActivity
import com.navree.client.platform.PushProvider
import com.navree.client.platform.PushTokens

/**
 * Push tokens from Firebase Cloud Messaging. The project's values come from
 * the app's resources (`navree_fcm_project_id`, `navree_fcm_application_id`,
 * `navree_fcm_api_key`, `navree_fcm_sender_id`), so no `google-services.json`
 * and no Gradle plugin are needed. With any of them empty there is no token,
 * and nothing is registered.
 *
 * ```
 * class StoreActivity : NavreeActivity() {
 *     override fun config() = super.config().copy(push = FcmPush(this))
 * }
 * ```
 */
class FcmPush(context: Context) : PushProvider {
    private val app = context.applicationContext

    override val provider: String = "fcm"

    override fun token(done: (String?) -> Unit) {
        firebase(app) ?: return done(null)
        FirebaseMessaging.getInstance().token
            .addOnSuccessListener { done(it) }
            .addOnFailureListener { done(null) }
    }

    /** Deletes this install's token at Firebase; the next one asked for is a new token. */
    override fun forget(done: () -> Unit) {
        firebase(app) ?: return done()
        try {
            @Suppress("DEPRECATION")
            FirebaseMessaging.getInstance().deleteToken().addOnCompleteListener { done() }
        } catch (_: Exception) {
            done()
        }
    }

    companion object {
        /** The app's Firebase project, made from its resources, or null when the app names none. */
        fun firebase(context: Context): FirebaseApp? {
            FirebaseApp.getApps(context).firstOrNull()?.let { return it }
            fun value(name: String): String? {
                val id = context.resources.getIdentifier(name, "string", context.packageName)
                return if (id == 0) null else context.getString(id).ifEmpty { null }
            }
            val options = FirebaseOptions.Builder()
                .setProjectId(value("navree_fcm_project_id") ?: return null)
                .setApplicationId(value("navree_fcm_application_id") ?: return null)
                .setApiKey(value("navree_fcm_api_key") ?: return null)
                .setGcmSenderId(value("navree_fcm_sender_id") ?: return null)
                .build()
            return FirebaseApp.initializeApp(context, options)
        }
    }
}

/**
 * Receives the messages the server's push channel sends, and shows each one
 * private on the lock screen: there the system shows a public version that
 * names the app and says only that something arrived, never the title or the
 * text. A tap opens the app with the message's `href`.
 *
 * This code sees a message only when Firebase hands it over: a data message
 * (`title`, `body` and `href` in `data`, no `notification`) always, a message
 * with a `notification` only while the app is in front. A `notification`
 * message that arrives while the app is in the background is shown by the
 * Firebase library itself, as the server wrote it, and never reaches here.
 *
 * A token that changes goes to the runtime core of the app while it runs (and
 * is asked for again at every launch); the core registers it. This module's
 * manifest declares the service, so an app that adds the module has it.
 */
class NavreeMessagingService : FirebaseMessagingService() {
    override fun onMessageReceived(message: RemoteMessage) {
        val shown = notificationText(message.notification?.title, message.notification?.body, message.data) ?: return
        // Only a path inside the app is kept; the core checks it again before anything opens.
        val href = message.data["href"]?.takeIf { it.startsWith("/") && !it.startsWith("//") }
        val manager = getSystemService(NotificationManager::class.java) ?: return
        if (manager.getNotificationChannel(CHANNEL) == null) {
            manager.createNotificationChannel(NotificationChannel(CHANNEL, getString(R.string.navree_notifications), NotificationManager.IMPORTANCE_DEFAULT))
        }
        val open = Intent(this, launcher()).apply {
            flags = Intent.FLAG_ACTIVITY_SINGLE_TOP or Intent.FLAG_ACTIVITY_CLEAR_TOP
            href?.let { putExtra(NavreeActivity.EXTRA_HREF, it) }
        }
        val tap = PendingIntent.getActivity(this, message.messageId.hashCode(), open, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)
        // What the lock screen shows: the app's name and that something arrived.
        val onLockScreen = NotificationCompat.Builder(this, CHANNEL)
            .setSmallIcon(applicationInfo.icon)
            .setContentTitle(applicationInfo.loadLabel(packageManager))
            .setContentText(getString(R.string.navree_notification_public))
            .build()
        val notification = NotificationCompat.Builder(this, CHANNEL)
            .setSmallIcon(applicationInfo.icon)
            .setContentTitle(shown.first)
            .setContentText(shown.second)
            .setVisibility(NotificationCompat.VISIBILITY_PRIVATE)
            .setPublicVersion(onLockScreen)
            .setAutoCancel(true)
            .setContentIntent(tap)
            .build()
        if (NotificationManagerCompat.from(this).areNotificationsEnabled()) {
            runCatching { NotificationManagerCompat.from(this).notify(message.collapseKey ?: message.messageId ?: "navree", 0, notification) }
        }
    }

    override fun onNewToken(token: String) {
        PushTokens.changed(token)
    }

    private fun launcher(): Class<*> =
        packageManager.getLaunchIntentForPackage(packageName)?.component?.className?.let { Class.forName(it) } ?: NavreeActivity::class.java

    private companion object {
        const val CHANNEL = "navree"
    }
}

/**
 * The title and text of a message: its `notification` part when it has one,
 * else `title` and `body` in its data. Null when it has neither: a message
 * with nothing to show is not shown.
 */
fun notificationText(title: String?, body: String?, data: Map<String, String>): Pair<String?, String?>? {
    val shownTitle = title ?: data["title"]
    val shownBody = body ?: data["body"]
    return if (shownTitle.isNullOrEmpty() && shownBody.isNullOrEmpty()) null else shownTitle to shownBody
}
