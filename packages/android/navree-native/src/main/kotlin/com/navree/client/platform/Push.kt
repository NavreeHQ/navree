package com.navree.client.platform

import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import okhttp3.Callback
import okhttp3.HttpUrl
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody

/**
 * Where a device's push token comes from. The client knows nothing of the
 * service behind it: an app that sends notifications adds a provider (the
 * Firebase one is in the `navree-push-fcm` module) and names it in
 * [com.navree.client.NavreeConfig.push].
 */
interface PushProvider {
    /** The protocol's name for the service: `fcm`. */
    val provider: String

    /** Asks the service for this install's token; `done(null)` when it cannot give one. */
    fun token(done: (String?) -> Unit)
}

/**
 * Registers the device's push token with the app's server, and takes it away
 * at sign-out: `POST` and `DELETE /api/notifications/push-devices` on the
 * origin, with the session cookie and the `X-XSRF-TOKEN` header, as the
 * server's notification module expects. These are the client's own requests
 * because no screen asks for them; they go nowhere but the origin.
 */
class PushRegistration(
    private val client: OkHttpClient,
    private val origin: HttpUrl,
    private val appId: String,
    private val provider: PushProvider,
) {
    private var token: String? = null

    /** At each launch, and whenever the service hands out a new token. */
    fun register() {
        provider.token { token ->
            if (token == null) return@token
            this.token = token
            send(
                "POST",
                buildJsonObject {
                    put("platform", "android")
                    put("token", token)
                    put("app_id", appId)
                    put("environment", "production")
                }.toString(),
            )
        }
    }

    /** At sign-out, before the cookies go. The server ends the device with the session anyway. */
    fun unregister() {
        val token = token ?: return
        send("DELETE", buildJsonObject { put("token", token) }.toString())
    }

    private fun send(method: String, json: String) {
        val url = origin.newBuilder().encodedPath(PATH).build()
        val builder = Request.Builder()
            .url(url)
            .header("Accept", "application/json")
            .method(method, json.toRequestBody("application/json".toMediaType()))
        xsrfToken(client.cookieJar, url)?.let { builder.header("X-XSRF-TOKEN", it) }
        client.newCall(builder.build()).enqueue(
            object : Callback {
                override fun onFailure(call: okhttp3.Call, e: java.io.IOException) = Unit

                override fun onResponse(call: okhttp3.Call, response: okhttp3.Response) = response.close()
            },
        )
    }

    companion object {
        const val PATH = "/api/notifications/push-devices"
    }
}
