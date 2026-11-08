package cn.juniexd.nexttodo.widget

import android.content.Context
import cn.juniexd.nexttodo.Local
import java.net.HttpURLConnection
import java.net.Proxy
import java.net.URL
import java.nio.ByteBuffer
import java.security.SecureRandom
import java.time.LocalDate
import java.util.UUID
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.json.JSONArray
import org.json.JSONObject

/** A request the server answered with an error, with its message. */
class HttpError(val status: Int, message: String) : Exception(message)

/** The server, reached with the address and session token the app keeps in Capacitor Preferences. */
object Server {
    fun prefs(context: Context) = context.getSharedPreferences("CapacitorStorage", Context.MODE_PRIVATE)

    /** The session's token; `null` when signed out. */
    fun token(context: Context): String? = prefs(context).getString("token", null)

    /** Signs out after the server said the session `token` has ended, unless the app has signed in
     * again since. Returns whether it did. */
    fun ended(context: Context, token: String): Boolean {
        val prefs = prefs(context)
        if (prefs.getString("token", null) != token) return false
        prefs.edit().remove("token").apply()
        return true
    }

    /** The ready tasks, as a JSON array in the shape of the `next` tool's `tasks`, and the change they
     * are as of. */
    suspend fun next(context: Context, token: String): Pair<String, Long> {
        val reply = JSONObject(post(context, token, "/tools/next", JSONObject().put("limit", LIMIT)))
        return reply.getJSONObject("data").getJSONArray("tasks").toString() to reply.getLong("seq")
    }

    /** The most `next` gives, as the app's list has them all. */
    private const val LIMIT = 500

    /** Marks a task done; one done already stays so, a repeating one moves on from today here.
     * Returns the change that did it. */
    suspend fun complete(context: Context, token: String, task: String): Long {
        val op = JSONObject().put("op", "task.update").put("id", task).put("done", true).put("today", LocalDate.now().toString())
        return JSONObject(post(context, token, "/mutations", JSONObject().put("id", uuid7()).put("ops", JSONArray().put(op)))).getLong("seq")
    }

    /** Throws `HttpError` with the server's message when the server turns the request down. */
    private suspend fun post(context: Context, token: String, path: String, body: JSONObject): String = withContext(Dispatchers.IO) {
        val url = URL("${prefs(context).getString("server", null)}/api/v1$path")
        // The system's proxy would get the token for local mode's server, in the app (docs/local.md#页面直连).
        val c = (if (Local.on(context)) url.openConnection(Proxy.NO_PROXY) else url.openConnection()) as HttpURLConnection
        try {
            c.requestMethod = "POST"
            c.connectTimeout = 10_000
            c.readTimeout = 10_000
            c.setRequestProperty("Authorization", "Bearer $token")
            c.setRequestProperty("Content-Type", "application/json")
            c.doOutput = true
            c.outputStream.use { it.write(body.toString().toByteArray()) }
            if (c.responseCode !in 200..299) {
                val error = c.errorStream?.bufferedReader()?.use { it.readText() }
                throw HttpError(c.responseCode, error?.let { runCatching { JSONObject(it).getString("message") }.getOrNull() } ?: "服务器错误（${c.responseCode}），请稍后重试")
            }
            c.inputStream.bufferedReader().use { it.readText() }
        } finally {
            c.disconnect()
        }
    }

    /** Change ids are UUIDv7: the time in milliseconds, then random bits. */
    private fun uuid7(): String {
        val bytes = ByteArray(16).also(SecureRandom()::nextBytes)
        val ms = System.currentTimeMillis()
        for (i in 0..5) bytes[i] = (ms shr (40 - 8 * i)).toByte()
        bytes[6] = (bytes[6].toInt() and 0x0F or 0x70).toByte()
        bytes[8] = (bytes[8].toInt() and 0x3F or 0x80).toByte()
        val b = ByteBuffer.wrap(bytes)
        return UUID(b.long, b.long).toString()
    }
}
