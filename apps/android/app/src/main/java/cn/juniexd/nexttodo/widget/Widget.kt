package cn.juniexd.nexttodo.widget

import android.app.AlarmManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.content.SharedPreferences
import android.os.Build
import android.os.SystemClock
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
import cn.juniexd.nexttodo.Local
import cn.juniexd.nexttodo.forUser
import com.capacitorjs.plugins.localnotifications.NotificationStorage
import com.capacitorjs.plugins.localnotifications.TimedNotificationPublisher
import java.time.LocalDate
import java.util.concurrent.TimeUnit
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.channels.awaitClose
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.callbackFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import org.json.JSONArray
import org.json.JSONObject

/** A task as the widget shows it. Its number is the id of its reminder; `lastDone` is the last day a
 * repeating one was done; `step`, which step it is along its dependencies, such as `2/5`. */
data class Item(val id: String, val number: Int, val title: String, val folder: String, val due: Due?, val lastDone: LocalDate?, val step: String?)

/** What the widget shows: the list with the tasks just ticked off in it, or that the app is signed out;
 * its text `scale`d as the user chose. */
data class Shown(val signedIn: Boolean, val items: List<Item>, val ticked: Set<String>, val scale: Float)

/** The widget's list, kept in its own preferences: written by the app, or fetched from the server. */
object Widget {
    private fun prefs(context: Context) = context.getSharedPreferences("widget", Context.MODE_PRIVATE)

    /** A list is as of a change on the server, by its sequence number, kept with it: one as of an
     * earlier change than the list kept is not written. The app's copy can be behind the server's,
     * just started or back from being frozen in the background, and its list older than the one the
     * widget fetched. A list as of no change, the empty one of signing out, replaces any, and any
     * replaces it. */
    private const val SEQ = "seq"

    /** Tasks done here that the list kept still has, being from before, by the change that did each:
     * hidden from it until a list as of that change comes. Kept apart from the list, which stays
     * what it was as of its change. */
    private const val DONE = "done"

    /** How large the text is, chosen in the app's settings (docs/android.md#小组件), and the scale of each. */
    private const val TEXT_SIZE = "text_size"
    private val SCALES = mapOf("small" to 0.88f, "standard" to 1f, "large" to 1.15f)

    fun textSize(context: Context): String = prefs(context).getString(TEXT_SIZE, "standard")!!

    suspend fun setTextSize(context: Context, size: String) = update(context) {
        it.edit().putString(TEXT_SIZE, size).apply()
        true
    }

    /** Changes go one at a time, each checking, the session too, on what the one before left. */
    private val writing = Mutex()

    /** Runs `change` on the preferences in turn, and shows the list again when it changed them. */
    private suspend fun update(context: Context, change: (SharedPreferences) -> Boolean) {
        if (writing.withLock { change(prefs(context)) }) NextWidget().updateAll(context)
    }

    /** Keeps `tasks`, as of the change `seq`, unless the list kept is as of a later one. */
    private fun keep(prefs: SharedPreferences, tasks: String, seq: Long?): Boolean {
        if (seq != null && prefs.contains(SEQ) && seq < prefs.getLong(SEQ, 0)) return false
        val edit = prefs.edit().putString("tasks", tasks)
        if (seq == null) {
            edit.remove(SEQ).remove(DONE)
        } else {
            val done = done(prefs)
            done.keys().asSequence().toList().filter { done.getLong(it) <= seq }.forEach(done::remove)
            edit.putLong(SEQ, seq).putString(DONE, done.toString())
        }
        edit.apply()
        return true
    }

    private fun done(prefs: SharedPreferences) = JSONObject(prefs.getString(DONE, "{}"))

    /** Shows `tasks`, the app's list, as JSON in the shape of the `next` tool's `tasks`, as of the
     * change `seq` its copy has. The app's lists come in the order of its sign-ins and sign-outs. */
    suspend fun show(context: Context, tasks: String, seq: Long?) = update(context) { keep(it, tasks, seq) }

    private fun items(context: Context): List<Item> {
        val prefs = prefs(context)
        val list = JSONArray(prefs.getString("tasks", "[]"))
        val done = done(prefs)
        return (0 until list.length()).map(list::getJSONObject).filter { !done.has(it.getString("id")) }.map { t ->
            val due = t.optJSONObject("due")?.let { Due.from(it) }
            // `/学习/毕业论文` reads `学习 / 毕业论文`; the root shows nothing.
            val lastDone = t.optJSONObject("completions")?.let { LocalDate.parse(it.getString("last")) }
            val step = t.optJSONArray("step")?.let { "${it.getInt(0)}/${it.getInt(1)}" }
            Item(t.getString("id"), t.getInt("number"), t.getString("title"), t.getString("folder").removePrefix("/").replace("/", " / "), due, lastDone, step)
        }
    }

    /** A task ticked off and being completed, with where it was in the list, in the session `token`. */
    private data class Ticked(val row: IndexedValue<Item>, val token: String)

    /** The tasks being completed: each stays where it was, shown done, until it goes, whatever lists
     * come meanwhile; in its own session only. Kept in memory, as the completing is: should the app be
     * stopped, both end. */
    private val ticked = MutableStateFlow<Map<String, Ticked>>(emptyMap())

    /** What to show now. */
    fun shown(context: Context): Shown {
        val token = Server.token(context)
        val ticked = ticked.value.filterValues { it.token == token }
        val items = items(context).toMutableList()
        for ((i, item) in ticked.values.map { it.row }.sortedBy { it.index }) {
            if (items.none { it.id == item.id }) items.add(minOf(i, items.size), item)
        }
        return Shown(token != null, items, ticked.keys, SCALES.getValue(textSize(context)))
    }

    /** Hides a task done on the server in session `token` by the change `seq` while the list kept
     * is from before it, unless signed in again since. */
    private suspend fun drop(context: Context, task: String, token: String, seq: Long) = update(context) { prefs ->
        if (Server.token(context) != token || (prefs.contains(SEQ) && prefs.getLong(SEQ, 0) >= seq)) return@update false
        // The latest of its completions, should it be completed twice, from two widgets say.
        val done = done(prefs)
        prefs.edit().putString(DONE, done.put(task, maxOf(seq, done.optLong(task, seq))).toString()).apply()
        true
    }

    /** What to show, each time the list or the sign-in changes: an empty list stays empty when the
     * app signs in or out. */
    fun changes(context: Context): Flow<Shown> = callbackFlow {
        val watched = listOf(prefs(context), Server.prefs(context))
        val listener = SharedPreferences.OnSharedPreferenceChangeListener { _, _ -> trySend(shown(context)) }
        watched.forEach { it.registerOnSharedPreferenceChangeListener(listener) }
        // Also sends what to show at once: it may have changed since read for the first composition,
        // before the listener was there.
        launch { ticked.collect { trySend(shown(context)) } }
        awaitClose { watched.forEach { it.unregisterOnSharedPreferenceChangeListener(listener) } }
    }

    /** Fetches the list from the server when signed in. An answer for a session that has ended
     * meanwhile is dropped. */
    suspend fun refresh(context: Context) {
        val token = Server.token(context) ?: return
        val (tasks, seq) = try {
            Server.next(context, token)
        } catch (e: HttpError) {
            if (e.status == 401) return ended(context, token)
            throw e
        }
        update(context) { Server.token(context) == token && keep(it, tasks, seq) }
    }

    /** Marks a task done, takes back its reminder (`number`), and fetches the list again. The task
     * shows as done at once, and stays a moment before it goes, as in the app. */
    suspend fun complete(context: Context, task: String, number: Int) {
        val token = Server.token(context) ?: return
        val start = SystemClock.elapsedRealtime()
        shown(context).items.withIndex().find { it.value.id == task }?.let { row -> ticked.update { it + (task to Ticked(row, token)) } }
        try {
            NextWidget().updateAll(context)
            val seq = try {
                Server.complete(context, token, task)
            } catch (e: Exception) {
                if (e is HttpError && e.status == 401) ended(context, token) else toast(context, forUser("完成事项", e))
                return
            }
            // Signed in again meanwhile, maybe as someone else, whose reminder this number may be.
            if (Server.token(context) != token) return
            cancelReminder(context, number)
            delay(LINGER_MS - (SystemClock.elapsedRealtime() - start))
            drop(context, task, token, seq)
        } finally {
            // Signed in again, the same task may have been ticked off in the new session meanwhile.
            ticked.update { if (it[task]?.token == token) it - task else it }
            NextWidget().updateAll(context)
        }
        try {
            refresh(context)
        } catch (e: Exception) {
            toast(context, forUser("刷新", e))
        }
    }

    /** How long a task ticked off stays in the list, as in the app (`apps/web/src/views/marks.tsx`). */
    private const val LINGER_MS = 900L

    /** The server says the session `token` has ended: unless the app has signed in again since, sign
     * out, and the widget asks to sign in again. */
    private suspend fun ended(context: Context, token: String) {
        update(context) { Server.ended(context, token) && keep(it, "[]", null) }
    }

    /** Empties the list and takes away every reminder, those shown already too: they are of data that
     * is there no more (docs/local.md#数据代号). */
    suspend fun forget(context: Context) {
        update(context) { keep(it, "[]", null) }
        NotificationStorage(context).getSavedNotificationIds().forEach { cancelReminder(context, it.toInt()) }
        NotificationManagerCompat.from(context).cancelAll()
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

    /** Every 15 minutes, the shortest WorkManager allows; while online, but in local mode, whose server
     * is in the app. Updates the refreshes there are to the mode now, keeping when they come. */
    fun schedule(context: Context) {
        val network = if (Local.on(context)) NetworkType.NOT_REQUIRED else NetworkType.CONNECTED
        val request = PeriodicWorkRequestBuilder<RefreshWorker>(15, TimeUnit.MINUTES)
            .setConstraints(Constraints.Builder().setRequiredNetworkType(network).build())
            .build()
        WorkManager.getInstance(context).enqueueUniquePeriodicWork(WORK, ExistingPeriodicWorkPolicy.UPDATE, request)
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
