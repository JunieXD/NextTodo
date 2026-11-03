package cn.juniexd.nexttodo.widget

import android.app.AlarmManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.content.SharedPreferences
import android.os.Build
import android.widget.Toast
import androidx.core.app.NotificationManagerCompat
import androidx.glance.appwidget.updateAll
import androidx.work.Constraints
import androidx.work.CoroutineWorker
import androidx.work.ExistingPeriodicWorkPolicy
import androidx.work.NetworkType
import androidx.work.PeriodicWorkRequestBuilder
import androidx.work.WorkManager
import androidx.work.WorkerParameters
import com.capacitorjs.plugins.localnotifications.NotificationStorage
import com.capacitorjs.plugins.localnotifications.TimedNotificationPublisher
import java.time.LocalDate
import java.time.LocalTime
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicLong
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.channels.awaitClose
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.callbackFlow
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import org.json.JSONArray

/** A task as the widget shows it. Its number is the id of its reminder. */
data class Item(val id: String, val number: Int, val title: String, val folder: String, val due: Due?)

/** What the widget shows: the list, or that the app is signed out. */
data class Shown(val signedIn: Boolean, val items: List<Item>)

/** The widget's list, kept in its own preferences: written by the app, or fetched from the server. */
object Widget {
    private fun prefs(context: Context) = context.getSharedPreferences("widget", Context.MODE_PRIVATE)

    /** Lists are numbered as they are asked for, and one is written only when no later one has been. */
    private val asked = AtomicLong()
    private var written = 0L
    private val writing = Mutex()

    /** Shows `tasks`, the app's list, as JSON in the shape of the `next` tool's data. */
    suspend fun show(context: Context, tasks: String) = write(context, asked.incrementAndGet(), tasks)

    private suspend fun write(context: Context, number: Long, tasks: String) {
        writing.withLock {
            if (number < written) return
            written = number
            prefs(context).edit().putString("tasks", tasks).apply()
        }
        NextWidget().updateAll(context)
    }

    private fun items(context: Context): List<Item> {
        val list = JSONArray(prefs(context).getString("tasks", "[]"))
        return (0 until list.length()).map { i ->
            val t = list.getJSONObject(i)
            val due = t.optJSONObject("due")?.let { d -> Due(LocalDate.parse(d.getString("date")), d.optString("time").takeIf { it.isNotEmpty() }?.let(LocalTime::parse)) }
            // `/学习/毕业论文` reads `学习 / 毕业论文`; the root shows nothing.
            Item(t.getString("id"), t.getInt("number"), t.getString("title"), t.getString("folder").removePrefix("/").replace("/", " / "), due)
        }
    }

    /** What to show now. */
    fun shown(context: Context) = Shown(Server.token(context) != null, items(context))

    /** What to show, each time the list or the sign-in changes: an empty list stays empty when the
     * app signs in or out. */
    fun changes(context: Context): Flow<Shown> = callbackFlow {
        val watched = listOf(prefs(context), Server.prefs(context))
        val listener = SharedPreferences.OnSharedPreferenceChangeListener { _, _ -> trySend(shown(context)) }
        watched.forEach { it.registerOnSharedPreferenceChangeListener(listener) }
        // Changed maybe since read for the first composition, before the listener was there.
        trySend(shown(context))
        awaitClose { watched.forEach { it.unregisterOnSharedPreferenceChangeListener(listener) } }
    }

    /** Fetches the list from the server when signed in. An answer for a session that has ended
     * meanwhile, or older than a list written since, is dropped. */
    suspend fun refresh(context: Context) {
        val token = Server.token(context) ?: return
        val number = asked.incrementAndGet()
        val tasks = try {
            Server.next(context, token)
        } catch (e: HttpError) {
            if (e.status == 401) return ended(context, token)
            throw e
        }
        if (Server.token(context) == token) write(context, number, tasks)
    }

    /** Marks a task done, takes back its reminder (`number`), and fetches the list again. */
    suspend fun complete(context: Context, task: String, number: Int) {
        val token = Server.token(context) ?: return
        try {
            Server.complete(context, token, task)
        } catch (e: Exception) {
            if (e is HttpError && e.status == 401) ended(context, token) else toast(context, "没能完成：${e.message}")
            return
        }
        // Signed in again meanwhile, maybe as someone else, whose reminder this number may be.
        if (Server.token(context) != token) return
        cancelReminder(context, number)
        try {
            refresh(context)
        } catch (e: Exception) {
            toast(context, "没能刷新：${e.message}")
        }
    }

    /** The server says the session `token` has ended: unless the app has signed in again since, sign
     * out, and the widget asks to sign in again. */
    private suspend fun ended(context: Context, token: String) {
        if (Server.ended(context, token)) write(context, asked.incrementAndGet(), "[]")
    }

    /** Takes back a reminder the app scheduled with Capacitor's LocalNotifications (8.3), the way the
     * plugin does: the app may not be running to notice that the task is done. */
    private fun cancelReminder(context: Context, id: Int) {
        val flags = PendingIntent.FLAG_NO_CREATE or if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) PendingIntent.FLAG_MUTABLE else 0
        PendingIntent.getBroadcast(context, id, Intent(context, TimedNotificationPublisher::class.java), flags)?.let {
            context.getSystemService(AlarmManager::class.java).cancel(it)
            it.cancel()
        }
        NotificationStorage(context).deleteNotification(id.toString())
        NotificationManagerCompat.from(context).cancel(id)
    }

    suspend fun toast(context: Context, text: String) = withContext(Dispatchers.Main) {
        Toast.makeText(context, text, Toast.LENGTH_LONG).show()
    }

    /** Every 15 minutes while online, the shortest WorkManager allows. */
    fun schedule(context: Context) {
        val request = PeriodicWorkRequestBuilder<RefreshWorker>(15, TimeUnit.MINUTES)
            .setConstraints(Constraints.Builder().setRequiredNetworkType(NetworkType.CONNECTED).build())
            .build()
        WorkManager.getInstance(context).enqueueUniquePeriodicWork(WORK, ExistingPeriodicWorkPolicy.KEEP, request)
    }

    fun unschedule(context: Context) {
        WorkManager.getInstance(context).cancelUniqueWork(WORK)
    }

    private const val WORK = "widget-refresh"
}

class RefreshWorker(context: Context, params: WorkerParameters) : CoroutineWorker(context, params) {
    override suspend fun doWork(): Result =
        try {
            Widget.refresh(applicationContext)
            Result.success()
        } catch (e: Exception) {
            Result.retry()
        }
}
