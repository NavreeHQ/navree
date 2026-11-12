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
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.put
import okhttp3.HttpUrl
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody

/**
 * The device calls this build carries out, each from a tap, each answered to
 * the core with the result's fields or one of the protocol's refusals. What
 * the core then posts, where, and with which ticket is the core's business.
 *
 * - `share`: the system's share sheet; `completed` says whether a target was chosen.
 * - `platform_sign_in`: Sign in with Google through Credential Manager, with the server's nonce.
 * - `passkey`: creating or using a passkey through Credential Manager.
 * - `confirm`: the server's challenge for the action, asked with the ticket, then a passkey over it.
 */
class DeviceCalls(
    private val client: OkHttpClient,
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
            "platform_sign_in" -> signIn(activity, effect.call, call)
            "passkey" -> passkey(activity, effect.call, call)
            "confirm" -> confirm(activity, effect.call, effect.ticket)
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

    private fun signIn(activity: ComponentActivity, call: Long, device: JsonObject) {
        val provider = (device["provider"] as? JsonPrimitive)?.contentOrNull
        val nonce = (device["nonce"] as? JsonPrimitive)?.contentOrNull
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
                    Events.deviceDone(
                        call,
                        listOfNotNull(
                            "provider" to "google",
                            "id_token" to google.idToken,
                            google.givenName?.let { "given_name" to it },
                            google.familyName?.let { "family_name" to it },
                            "email" to google.id,
                        ),
                    )
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
     * A confirmation: the ticket buys the challenge for this action at the
     * server's challenge route, a passkey with user verification signs it,
     * and the signature goes back to the core, which posts it.
     */
    private fun confirm(activity: ComponentActivity, call: Long, ticket: String?) {
        if (ticket == null) {
            answer(Events.deviceRefused(call, "failed"))
            return
        }
        activity.lifecycleScope.launch {
            val event = try {
                val options = withContext(Dispatchers.IO) { challenge(ticket) }
                val publicKey = options?.get("publicKey") as? JsonObject
                val challenge = (publicKey?.get("challenge") as? JsonPrimitive)?.contentOrNull
                if (publicKey == null || challenge == null) {
                    Events.deviceRefused(call, "failed")
                } else {
                    val got = CredentialManager.create(activity)
                        .getCredential(activity, GetCredentialRequest(listOf(GetPublicKeyCredentialOption(publicKey.toString()))))
                    val json = (got.credential as? PublicKeyCredential)?.authenticationResponseJson
                    if (json == null) {
                        Events.deviceRefused(call, "failed")
                    } else {
                        Events.deviceDone(call, listOf("challenge" to challenge, "method" to "passkey", "credential" to json))
                    }
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

    /** `POST /api/auth/confirm/options {"ticket"}` on the origin, with the session. */
    private fun challenge(ticket: String): JsonObject? {
        val url = origin.newBuilder().encodedPath(CONFIRM_PATH).build()
        val builder = Request.Builder()
            .url(url)
            .header("Accept", "application/json")
            .post(buildJsonObject { put("ticket", ticket) }.toString().toRequestBody("application/json".toMediaType()))
        xsrfToken(client.cookieJar, url)?.let { builder.header("X-XSRF-TOKEN", it) }
        client.newCall(builder.build()).execute().use { response ->
            if (!response.isSuccessful) return null
            return Json.parseToJsonElement(response.body.string()) as? JsonObject
        }
    }

    companion object {
        const val CONFIRM_PATH = "/api/auth/confirm/options"
    }
}

/** An address inside the origin, or null when `href` leads elsewhere. */
private fun HttpUrl.inside(href: String): HttpUrl? =
    resolve(href)?.takeIf { it.scheme == scheme && it.host == host && it.port == port }
