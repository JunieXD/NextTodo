package cn.juniexd.nexttodo

import android.app.Notification
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.net.Uri
import androidx.compose.ui.graphics.toArgb
import androidx.work.BackoffPolicy
import androidx.work.Constraints
import androidx.work.CoroutineWorker
import androidx.work.ExistingWorkPolicy
import androidx.work.NetworkType
import androidx.work.OneTimeWorkRequestBuilder
import androidx.work.WorkManager
import androidx.work.WorkerParameters
import cn.juniexd.nexttodo.reminders.Reminders
import cn.juniexd.nexttodo.widget.Widget
import java.io.IOException
import java.time.LocalDate
import java.time.LocalDateTime
import java.util.concurrent.TimeUnit
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import org.json.JSONArray
import org.json.JSONObject

/**
 * Changes made natively, without the app's page: ticking a task off in the widget, on a reminder, and
 * adding from the add popup (docs/android.md#原生发出的改动). They go to the server in the order made;
 * offline they are kept, and sent once online; the app takes them in too when it opens.
 */
object Changes {
    private fun prefs(context: Context) = context.getSharedPreferences("changes", Context.MODE_PRIVATE)

    /** The changes not sent yet, oldest first: `{id, ops, what, user}`, `what` naming it for the user,
     * as in `完成“交房租”`, and `user` who was signed in. */
    private const val KEPT = "kept"

    /** Who is signed in, as the app last said; none before it first has. */
    private const val USER = "user"

    sealed interface Outcome {
        /** The server has it, as of its change `seq`. */
        data class Sent(val seq: Long) : Outcome

        /** Kept, to send later: offline, or signed out meanwhile. */
        data object Kept : Outcome

        /** Turned down by the server; `message` says why, for the user. */
        data class Refused(val message: String) : Outcome
    }

    /** One sending at a time, the order of the changes kept with it. */
    private val sending = Mutex()

    private fun kept(context: Context) = JSONArray(prefs(context).getString(KEPT, "[]"))

    private fun keep(context: Context, changes: List<JSONObject>) {
        prefs(context).edit().putString(KEPT, JSONArray(changes).toString()).commit()
    }

    private fun JSONArray.objects() = (0 until length()).map(::getJSONObject)

    /** Marks `task`, titled `title`, done; a repeating one moves on from today here. Done or kept, its
     * reminder goes, and the widget stops showing it. */
    suspend fun complete(context: Context, task: String, title: String): Outcome {
        val token = Server.token(context)
        // The one it was done from: the app may hand over the next by the time the server answers.
        val at = Reminders.find(context, task)?.at
        val op = JSONObject().put("op", "task.update").put("id", task).put("done", true).put("today", LocalDate.now().toString())
        val outcome = submit(context, op, "完成“$title”")
        // Signed in again meanwhile, maybe as someone else, whose reminders these are.
        if (outcome is Outcome.Refused || token == null || Server.token(context) != token) return outcome
        if (at != null) Reminders.completed(context, task, at)
        return outcome
    }

    /** Adds a task titled `title` to the root, last. */
    suspend fun add(context: Context, title: String) =
        submit(context, JSONObject().put("op", "task.create").put("id", Server.uuid7()).put("title", title), "添加“$title”")

    /** Hands `text` to AI organizing, into the root. */
    suspend fun organize(context: Context, text: String): Outcome {
        val now = LocalDateTime.now()
        // As the app writes it: `2026-11-04 16:30 星期三`.
        val time = "%s %02d:%02d 星期%s".format(now.toLocalDate(), now.hour, now.minute, "一二三四五六日"[now.dayOfWeek.value - 1])
        return submit(context, JSONObject().put("op", "ai_job.create").put("id", Server.uuid7()).put("text", text).put("local_time", time), "交给 AI 整理")
    }

    /** Submits `op` as a change, after those kept from before. */
    private suspend fun submit(context: Context, op: JSONObject, what: String): Outcome = sending.withLock {
        val token = Server.token(context)
        val id = Server.uuid7()
        val change = JSONObject().put("id", id).put("ops", JSONArray().put(op)).put("what", what).put("user", prefs(context).getString(USER, null))
        keep(context, kept(context).objects() + change)
        val (outcome, offline) = send(context, id)
        // A task it marks done goes from the widget until it is sent; hidden here, in turn, so that
        // the change it becomes, once sent from elsewhere, comes after.
        if (outcome is Outcome.Kept && token != null) for (task in completed(change)) Widget.hide(context, task, token, null)
        if (offline) retry(context)
        outcome
    }

    /** Sends the changes kept, in order, until one cannot go: what became of `mine`, and whether
     * sending stopped for want of the network. Without a session they wait for the next. */
    private suspend fun send(context: Context, mine: String?): Pair<Outcome, Boolean> {
        var outcome: Outcome = Outcome.Kept
        val token = Server.token(context) ?: return outcome to false
        for (change in kept(context).objects()) {
            val id = change.getString("id")
            try {
                val seq = Server.mutate(context, token, id, change.getJSONArray("ops"))
                // Before it goes from the queue: stopped in between, it is sent again, and the
                // server gives the same `seq`.
                for (task in completed(change)) Widget.hide(context, task, token, seq)
                keep(context, kept(context).objects().filter { it.getString("id") != id })
                if (id == mine) outcome = Outcome.Sent(seq)
            } catch (e: HttpError) {
                if (e.status == 401) {
                    // Kept for when the same user signs in again, as the app keeps its own.
                    Widget.ended(context, token)
                    return outcome to false
                }
                if (e.status >= 500) return outcome to true
                keep(context, kept(context).objects().filter { it.getString("id") != id })
                val message = forUser(change.getString("what"), e)
                if (id == mine) outcome = Outcome.Refused(message) else refused(context, change, message)
            } catch (e: IOException) {
                return outcome to true
            }
        }
        return outcome to false
    }

    /** The tasks a change marks done. */
    private fun completed(change: JSONObject) =
        change.getJSONArray("ops").objects().filter { it.getString("op") == "task.update" && it.optBoolean("done") }.map { it.getString("id") }

    /** The server turned down a change kept from before: it goes, and a notification says which and
     * why. A task it marked done shows again. */
    private suspend fun refused(context: Context, change: JSONObject, message: String) {
        for (task in completed(change)) Widget.unhide(context, task)
        val open = PendingIntent.getActivity(context, 0, Intent(Intent.ACTION_VIEW, Uri.parse("nexttodo://home"), context, MainActivity::class.java), PendingIntent.FLAG_IMMUTABLE)
        val notification = Notification.Builder(context, Channels.REFUSED)
            .setSmallIcon(R.drawable.ic_stat_nexttodo)
            .setColor(Palette.light.primary.toArgb())
            .setContentTitle("${change.getString("what")}失败")
            .setContentText(message)
            .setStyle(Notification.BigTextStyle().bigText(message))
            .setContentIntent(open)
            .setAutoCancel(true)
            .build()
        // One for each change, by its id.
        context.getSystemService(NotificationManager::class.java).notify(change.getString("id"), 0, notification)
    }

    /** Sends what is kept once online: after those waiting, should some be. */
    private fun retry(context: Context) {
        val network = if (Local.on(context)) NetworkType.NOT_REQUIRED else NetworkType.CONNECTED
        val request = OneTimeWorkRequestBuilder<SendWorker>()
            .setConstraints(Constraints.Builder().setRequiredNetworkType(network).build())
            .setBackoffCriteria(BackoffPolicy.EXPONENTIAL, 30, TimeUnit.SECONDS)
            .build()
        WorkManager.getInstance(context).enqueueUniqueWork(WORK, ExistingWorkPolicy.APPEND_OR_REPLACE, request)
    }

    private const val WORK = "changes"

    /** Sends what is kept; whether it stopped for want of the network. */
    suspend fun flush(context: Context): Boolean = sending.withLock { send(context, null).second }

    /** Signing in as `user`, before the session's token is written: the changes kept of anyone else
     * go, so that none is sent under it. So do those from before the app first said who was signed
     * in: whose they are is not known. */
    suspend fun signIn(context: Context, user: String) = sending.withLock {
        val theirs = kept(context).objects().filter { it.optString("user") == user }
        prefs(context).edit().putString(KEPT, JSONArray(theirs).toString()).putString(USER, user).commit()
    }

    /** The app opened a session of `user`: the changes kept for them, as `[{id, ops}]`, which it takes
     * in, to show and send as its own. They stay kept here, sent from here too, the server taking each
     * once: none is lost should the app be stopped before it has saved them, and the widget learns
     * the change each of its ticks became. Those of anyone else go. Ones from before the app first
     * said who is signed in were made in this session, signing in since having deleted them. */
    suspend fun takeOver(context: Context, user: String): String = sending.withLock {
        val theirs = kept(context).objects().filter { it.optString("user", user) == user }.onEach { it.put("user", user) }
        prefs(context).edit().putString(KEPT, JSONArray(theirs).toString()).putString(USER, user).commit()
        if (theirs.isNotEmpty()) retry(context)
        JSONArray(theirs.map { JSONObject().put("id", it.getString("id")).put("ops", it.getJSONArray("ops")) }).toString()
    }

    /** Deletes the changes kept, and who is signed in: signed out, or the data is gone. */
    suspend fun forget(context: Context) = sending.withLock {
        prefs(context).edit().clear().commit()
    }
}

class SendWorker(context: Context, params: WorkerParameters) : CoroutineWorker(context, params) {
    override suspend fun doWork(): Result {
        if (Changes.flush(applicationContext)) return Result.retry()
        // A task added shows in the widget, one done goes.
        Widget.refresh(applicationContext)
        return Result.success()
    }
}
