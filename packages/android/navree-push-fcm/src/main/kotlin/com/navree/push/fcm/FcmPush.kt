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
 * Receives the messages the server's push channel sends. A message the
 * system shows by itself (the app is not in front) opens the app with its
 * `href` when tapped; one that arrives while the app is open is shown here,
 * with the same link. A token that changes goes to the runtime core of the app
 * while it runs (and is asked for again at every launch); the core registers it.
 *
 * This module's manifest declares it, so an app that adds the module has it.
 */
class NavreeMessagingService : FirebaseMessagingService() {
    override fun onMessageReceived(message: RemoteMessage) {
        val shown = message.notification ?: return
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
        val notification = NotificationCompat.Builder(this, CHANNEL)
            .setSmallIcon(applicationInfo.icon)
            .setContentTitle(shown.title)
            .setContentText(shown.body)
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
