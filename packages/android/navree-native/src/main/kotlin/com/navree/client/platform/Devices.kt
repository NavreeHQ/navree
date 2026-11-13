package com.navree.client.platform

import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import androidx.activity.ComponentActivity
import androidx.core.content.ContextCompat
import androidx.credentials.CreatePublicKeyCredentialRequest
import androidx.credentials.CreatePublicKeyCredentialResponse
import androidx.credentials.CredentialManager
import androidx.credentials.CustomCredential
import androidx.credentials.GetCredentialRequest
import androidx.credentials.GetPublicKeyCredentialOption
import androidx.credentials.PublicKeyCredential
import androidx.credentials.exceptions.CreateCredentialCancellationException
import androidx.credentials.exceptions.GetCredentialCancellationException
import androidx.credentials.exceptions.NoCredentialException
import androidx.lifecycle.lifecycleScope
import com.google.android.libraries.identity.googleid.GetGoogleIdOption
import com.google.android.libraries.identity.googleid.GoogleIdTokenCredential
import com.navree.client.core.Effect
import com.navree.client.core.Events
import kotlinx.coroutines.launch
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.contentOrNull
import okhttp3.HttpUrl

/**
 * The device calls this build carries out, each from a tap, each answered to
 * the core with the result's fields or one of the protocol's refusals. What
 * the core then posts, where, and with which ticket is the core's business.
 *
 * - `share`: the system's share sheet; `completed` says whether a target was chosen.
 * - `platform_sign_in`: Sign in with Google through Credential Manager, with the nonce the core asked for.
 * - `passkey`: creating or using a passkey through Credential Manager.
 * - `confirm`: a passkey over the challenge the core asked the server for.
 */
class DeviceCalls(
    private val origin: HttpUrl,
    private val googleClientId: String?,
    private val answer: (JsonObject) -> Unit,
) {
    /** The features this build carries out, by the protocol's names. */
    val features: List<String> = listOf("share", "platform_sign_in", "passkey", "confirm")

    private var sharing: Pair<Long, BroadcastReceiver>? = null
    private var chosen = false

    fun run(activity: ComponentActivity?, effect: Effect.Device) {
        val call = effect.device
        val f = (call["f"] as? JsonPrimitive)?.contentOrNull
        if (activity == null) {
            if (effect.posts) answer(Events.deviceRefused(effect.call, "unavailable"))
            return
        }
        when (f) {
            "share" -> share(activity, effect.call, call)
            "platform_sign_in" -> signIn(activity, effect.call, call, effect.options)
            "passkey" -> passkey(activity, effect.call, call)
            "confirm" -> confirm(activity, effect.call, effect.options)
            else -> if (effect.posts) answer(Events.deviceRefused(effect.call, "not_built"))
        }
    }

    /** The share sheet closed and the activity is in front again: say whether something was chosen. */
    fun resumed(context: Context) {
        val (call, receiver) = sharing ?: return
        sharing = null
        runCatching { context.unregisterReceiver(receiver) }
        answer(Events.deviceDone(call, listOf("completed" to chosen.toString())))
    }

    private fun share(activity: ComponentActivity, call: Long, device: JsonObject) {
        fun s(name: String) = (device[name] as? JsonPrimitive)?.contentOrNull
        val link = s("url") ?: s("href")?.let { href -> origin.inside(href)?.toString() }
        val text = listOfNotNull(s("text"), link).joinToString("\n").ifEmpty { s("title") ?: "" }
        val send = Intent(Intent.ACTION_SEND).apply {
            type = "text/plain"
            putExtra(Intent.EXTRA_TEXT, text)
            s("title")?.let { putExtra(Intent.EXTRA_TITLE, it) }
        }
        // The system tells which target was chosen through a broadcast only this app can receive.
        val action = "${activity.packageName}.navree.SHARED"
        chosen = false
        val receiver = object : BroadcastReceiver() {
            override fun onReceive(context: Context, intent: Intent) {
                chosen = true
            }
        }
        ContextCompat.registerReceiver(activity, receiver, IntentFilter(action), ContextCompat.RECEIVER_NOT_EXPORTED)
        val pending = PendingIntent.getBroadcast(
            activity,
            call.toInt(),
            Intent(action).setPackage(activity.packageName),
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_MUTABLE,
        )
        sharing = call to receiver
        activity.startActivity(Intent.createChooser(send, s("title"), pending.intentSender))
    }

    private fun signIn(activity: ComponentActivity, call: Long, device: JsonObject, options: JsonObject?) {
        val provider = (device["provider"] as? JsonPrimitive)?.contentOrNull
        // The nonce the core asked the server for, not the call's route to ask it at.
        val nonce = (options?.get("nonce") as? JsonPrimitive)?.contentOrNull
        val clientId = (device["client_id"] as? JsonPrimitive)?.contentOrNull ?: googleClientId
        if (provider != "google") {
            // Apple's sign-in on Android is its web flow, which the server offers as a link.
            answer(Events.deviceRefused(call, "unsupported"))
            return
        }
        if (nonce == null || clientId == null) {
            answer(Events.deviceRefused(call, "unavailable"))
            return
        }
        val option = GetGoogleIdOption.Builder()
            .setServerClientId(clientId)
            .setNonce(nonce)
            .setFilterByAuthorizedAccounts(false)
            .build()
        activity.lifecycleScope.launch {
            val event = try {
                val result = CredentialManager.create(activity).getCredential(activity, GetCredentialRequest(listOf(option)))
                val credential = result.credential
                if (credential is CustomCredential && credential.type == GoogleIdTokenCredential.TYPE_GOOGLE_ID_TOKEN_CREDENTIAL) {
                    val google = GoogleIdTokenCredential.createFrom(credential.data)
                    // The core posts the token to the app's sign-in route with the state it kept.
                    Events.deviceDone(call, listOf("id_token" to google.idToken))
                } else {
                    Events.deviceRefused(call, "failed")
                }
            } catch (_: GetCredentialCancellationException) {
                Events.deviceRefused(call, "cancelled")
            } catch (_: NoCredentialException) {
                Events.deviceRefused(call, "unavailable")
            } catch (_: Exception) {
                Events.deviceRefused(call, "failed")
            }
            answer(event)
        }
    }

    private fun passkey(activity: ComponentActivity, call: Long, device: JsonObject) {
        val options = (device["options"] as? JsonPrimitive)?.contentOrNull
        val mode = (device["mode"] as? JsonPrimitive)?.contentOrNull
        if (options == null) {
            answer(Events.deviceRefused(call, "failed"))
            return
        }
        activity.lifecycleScope.launch {
            val manager = CredentialManager.create(activity)
            val event = try {
                val json = if (mode == "register") {
                    val created = manager.createCredential(activity, CreatePublicKeyCredentialRequest(options))
                    (created as? CreatePublicKeyCredentialResponse)?.registrationResponseJson
                } else {
                    val got = manager.getCredential(activity, GetCredentialRequest(listOf(GetPublicKeyCredentialOption(options))))
                    (got.credential as? PublicKeyCredential)?.authenticationResponseJson
                }
                if (json != null) Events.deviceDone(call, listOf("credential" to json)) else Events.deviceRefused(call, "failed")
            } catch (_: GetCredentialCancellationException) {
                Events.deviceRefused(call, "cancelled")
            } catch (_: CreateCredentialCancellationException) {
                Events.deviceRefused(call, "cancelled")
            } catch (_: NoCredentialException) {
                Events.deviceRefused(call, "unavailable")
            } catch (_: Exception) {
                Events.deviceRefused(call, "failed")
            }
            answer(event)
        }
    }

    /**
     * A confirmation: the core has asked the server for the challenge of
     * this action; its answer is `options` (`{"publicKey": {...}}`). A passkey
     * with user verification signs it, and the signature goes back to the
     * core, which posts it.
     */
    private fun confirm(activity: ComponentActivity, call: Long, options: JsonObject?) {
        val publicKey = options?.get("publicKey") as? JsonObject
        val challenge = (publicKey?.get("challenge") as? JsonPrimitive)?.contentOrNull
        if (publicKey == null || challenge == null) {
            answer(Events.deviceRefused(call, "failed"))
            return
        }
        activity.lifecycleScope.launch {
            val event = try {
                val got = CredentialManager.create(activity)
                    .getCredential(activity, GetCredentialRequest(listOf(GetPublicKeyCredentialOption(publicKey.toString()))))
                val json = (got.credential as? PublicKeyCredential)?.authenticationResponseJson
                if (json == null) {
                    Events.deviceRefused(call, "failed")
                } else {
                    Events.deviceDone(call, listOf("challenge" to challenge, "method" to "passkey", "credential" to json))
                }
            } catch (_: GetCredentialCancellationException) {
                Events.deviceRefused(call, "cancelled")
            } catch (_: NoCredentialException) {
                Events.deviceRefused(call, "unavailable")
            } catch (_: Exception) {
                Events.deviceRefused(call, "failed")
            }
            answer(event)
        }
    }
}

/** An address inside the origin, or null when `href` leads elsewhere. */
private fun HttpUrl.inside(href: String): HttpUrl? =
    resolve(href)?.takeIf { it.scheme == scheme && it.host == host && it.port == port }
