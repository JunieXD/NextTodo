package cn.juniexd.nexttodo.widget

import android.app.AlarmManager
import android.app.PendingIntent
import android.appwidget.AppWidgetManager
import android.appwidget.AppWidgetProviderInfo
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.SharedPreferences
import android.os.Build
import android.widget.Toast
import androidx.collection.intSetOf
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.DpSize
import androidx.glance.appwidget.GlanceAppWidgetManager
import androidx.glance.appwidget.SizeMode
import androidx.glance.appwidget.updateAll
import androidx.work.Constraints
import androidx.work.CoroutineWorker
import androidx.work.ExistingPeriodicWorkPolicy
import androidx.work.ExistingWorkPolicy
import androidx.work.OneTimeWorkRequestBuilder
import androidx.work.PeriodicWorkRequestBuilder
import androidx.work.WorkManager
import androidx.work.WorkerParameters
import cn.juniexd.nexttodo.Due
import cn.juniexd.nexttodo.HttpError
import cn.juniexd.nexttodo.Local
import cn.juniexd.nexttodo.Occurrence
import cn.juniexd.nexttodo.Server
import cn.juniexd.nexttodo.reminders.Reminders
import java.time.LocalDate
import java.time.LocalDateTime
import java.time.LocalTime
import java.time.ZoneId
import java.time.ZonedDateTime
import java.time.temporal.ChronoUnit
import java.util.concurrent.TimeUnit
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.channels.awaitClose
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

/** A task as the widget shows it. `dueBy` is the deadline of a task after it that it is to make, should
 * that come first (docs/design/dates.md#后续的截止); `lastDone` the last day a repeating one was done, and
 * `occurrence` the time shown; `step`, which step it is along its dependencies, such as `2/5`. */
data class Item(val id: String, val title: String, val folder: String, val due: Due?, val dueBy: Due?, val lastDone: LocalDate?, val occurrence: Occurrence?, val step: String?)

/** What the widget shows: the list with the tasks just ticked off in it, each `ticked` at its time
 * should it repeat, and how many `more` there are past it; or that the app is signed out. With the `look` the user chose. As of `now`, to the minute,
 * which what it says of the time goes by: a part of it, so that the widget is drawn again as the time
 * alone moves on. */
data class Shown(val signedIn: Boolean, val items: List<Item>, val more: Int, val ticked: Set<Pair<String, Occurrence?>>, val look: Look, val now: LocalDateTime)

/** How the launcher's list of widgets shows one laid out for its size: as it starts, 4×2 to 4×4. Drawn
 * at its least, it would leave out what it leaves out there, the month's titles say. */
internal fun previewAt(width: Dp, height: Dp) = SizeMode.Responsive(setOf(DpSize(width, height)))

/** The widget's list, kept in its own preferences: written by the app, or fetched from the server. */
object Widget {
    fun prefs(context: Context) = context.getSharedPreferences("widget", Context.MODE_PRIVATE)

    /** A list is as of a change on the server, by its sequence number, kept with it: one as of an
     * earlier change than the list kept is not written. The app's copy can be behind the server's,
     * just started or back from being frozen in the background, and its list older than the one the
     * widget fetched. A list as of no change, the empty one of signing out, replaces any, and any
     * replaces it. */
    private const val SEQ = "seq"

    /** The most tasks the widget lists, the rest only counted: the launcher is handed all of them at
     * once, some 14KB each, and takes no more than 1MB. */
    private const val MOST = 20

    /** How many tasks there are past the list kept: those the server did not give. */
    private const val MORE = "more"

    /** The paths of the folders “接下来” lists on this phone, as a JSON array, which the widget's own
     * fetches ask for too (docs/android/widgets.md#小组件); none for all of them. */
    private const val FOLDERS = "folders"

    /** Until when what the widget last drawn says of the time holds, as `until` gave it then. */
    private const val UNTIL = "until"

    /** When what the widget says of the time, drawn with `items` at `now`, comes to be otherwise: at the
     * next midnight, for dates read as from today, or as a task falls due; never, when it says nothing
     * of the time. Worked out again later, it gives the same only while that still holds, also should
     * the clock have gone back or the time zone changed. */
    private fun until(items: List<Item>, now: LocalDateTime): LocalDateTime {
        if (items.none { it.due != null || it.dueBy != null || it.lastDone != null }) return LocalDateTime.MAX
        return (items.mapNotNull { (it.due ?: it.dueBy)?.deadline } + now.toLocalDate().plusDays(1).atStartOfDay()).filter { it.isAfter(now) }.min()
    }

    /** How the widgets look (docs/android/widgets.md#小组件的设置). */
    fun look(context: Context) = Look.read(prefs(context))

    suspend fun setLook(context: Context, look: Look) {
        update(context) {
            look.write(it.edit()).apply()
            true
        }
        AgendaWidget().updateAll(context)
        WeekWidget().updateAll(context)
        TimelineWidget().updateAll(context)
        MonthWidget().updateAll(context)
        setPreviews(context)
    }

    private val previews = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private val previewing = Mutex()

    /** Has the launcher's list of widgets show each with example tasks in the look chosen now
     * (`Sample`), on Android 15 and later; before, it shows the app's icon. Apart from what asks for
     * it, and one at a time, so that the latest look is the one shown. The system takes only a few a
     * while, leaving the one set before: the next try sets them again. */
    fun setPreviews(context: Context) {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.VANILLA_ICE_CREAM) return
        val app = context.applicationContext
        previews.launch {
            previewing.withLock {
                val manager = GlanceAppWidgetManager(app)
                for (receiver in listOf(NextWidgetReceiver::class, AgendaWidgetReceiver::class, WeekWidgetReceiver::class, TimelineWidgetReceiver::class, MonthWidgetReceiver::class)) {
                    manager.setWidgetPreviews(receiver, intSetOf(AppWidgetProviderInfo.WIDGET_CATEGORY_HOME_SCREEN))
                }
            }
        }
    }

    /** The paths of the folders the widgets list, as a JSON array; none for all of them. */
    fun folders(context: Context): String? = prefs(context).getString(FOLDERS, null)

    /** Changes go one at a time, each checking, the session too, on what the one before left. */
    private val writing = Mutex()

    /** Runs `change` on the preferences in turn, and shows the list again when it says so. */
    private suspend fun update(context: Context, change: (SharedPreferences) -> Boolean) {
        val draw = writing.withLock {
            val prefs = prefs(context)
            change(prefs).also { if (it) prefs.edit().putString(UNTIL, until(listed(prefs).first, LocalDateTime.now()).toString()).apply() }
        }
        if (draw) NextWidget().updateAll(context)
    }

    /** Keeps `tasks`, with `more` past them, as of the change `seq`, unless the list kept is as of a later
     * one; and fetches it again when `start`, the next start it comes with, does. */
    private fun keep(context: Context, prefs: SharedPreferences, tasks: String, more: Int, seq: Long?, start: String?): Boolean {
        if (seq != null && prefs.contains(SEQ) && seq < prefs.getLong(SEQ, 0)) return false
        val edit = prefs.edit().putString("tasks", tasks).putInt(MORE, more)
        if (seq == null) edit.remove(SEQ) else edit.putLong(SEQ, seq)
        Done.keep(prefs, edit, seq)
        edit.apply()
        refreshAt(context, start)
        return true
    }

    /** Shows `tasks`, the app's list, as JSON in the shape of the `next` tool's `tasks`, as of the
     * change `seq` its copy has. The app's lists come in the order of its sign-ins and sign-outs. Drawn
     * again only when the tasks it lists change, it signs in or out, or what it says of the time no
     * longer holds: the app hands its list over each time it starts, and Glance draws on the main
     * thread, which holds back the app's first frame. With the list, `start`: see `refreshAt`; and
     * `folders`: see `FOLDERS`. A list of other folders than the one kept, but older, is not taken
     * either: the widget fetches the list of those folders itself. */
    suspend fun show(context: Context, tasks: String, seq: Long?, start: String?, folders: String?) = update(context) { prefs ->
        val refiltered = prefs.getString(FOLDERS, null) != folders
        prefs.edit().putString(FOLDERS, folders).apply()
        val was = drawn(prefs)
        // A list older than the one kept is not taken, but what that one says of the time may be stale.
        val kept = keep(context, prefs, tasks, 0, seq, start)
        if (refiltered && !kept) started(context)
        val changed = kept && drawn(prefs) != was
        changed || prefs.getString(UNTIL, null) != until(listed(prefs).first, LocalDateTime.now()).toString()
    }

    /** What the widget draws of the preferences, but for what it says of the time. */
    private fun drawn(prefs: SharedPreferences) = listed(prefs) to prefs.contains(SEQ)

    /** The tasks the widget lists, the first of those kept but for those done here, with the rows of
     * those being completed where they were, `back`; and how many more there are. */
    private fun listed(prefs: SharedPreferences, back: List<IndexedValue<Item>> = emptyList()): Pair<List<Item>, Int> {
        val items = items(prefs)
        val listed = items.take(MOST).toMutableList()
        val shown = { item: Item -> listed.any { it.id == item.id && it.occurrence == item.occurrence } }
        for ((i, item) in back.sortedBy { it.index }) {
            if (!shown(item)) listed.add(minOf(i, listed.size), item)
        }
        return listed to items.count { !shown(it) } + prefs.getInt(MORE, 0)
    }

    private fun items(prefs: SharedPreferences): List<Item> {
        val list = JSONArray(prefs.getString("tasks", "[]"))
        val done = Done.of(prefs)
        return (0 until list.length()).map(list::getJSONObject).map { t ->
            val due = t.optJSONObject("due")?.let { Due.from(it) }
            val dueBy = t.optJSONObject("due_by")?.let { Due.from(it) }
            // `/学习/毕业论文` reads `学习 / 毕业论文`; the root shows nothing.
            val lastDone = t.optJSONObject("completions")?.let { LocalDate.parse(it.getString("last")) }
            val step = t.optJSONArray("step")?.let { "${it.getInt(0)}/${it.getInt(1)}" }
            Item(t.getString("id"), t.getString("title"), t.getString("folder").removePrefix("/").replace("/", " / "), due, dueBy, lastDone, Occurrence.of(t), step)
        }.filter { !Done.hides(done, it.id, it.occurrence) }
    }

    /** A task ticked off and being completed, at its time `occurrence` should it repeat, in the session
     * `token`, with where it was in the list, and in the “一周” widget's list of its day
     * (`Agenda.place`); in either, none when not there. */
    private data class Ticked(val occurrence: Occurrence?, val row: IndexedValue<Item>?, val entry: IndexedValue<Entry>?, val token: String)

    /** The tasks being completed: each stays where it was, shown done, until it goes, whatever lists
     * come meanwhile; in its own session only. Kept in memory, as the completing is: should the app be
     * stopped, both end. */
    private val ticked = MutableStateFlow<Map<String, Ticked>>(emptyMap())

    /** Each time a task is ticked off, or done being completed. */
    val ticking: Flow<*> = ticked

    /** The tasks being completed in this session, as the “一周” widget lists them. */
    fun ticked(context: Context): List<IndexedValue<Entry>> = Server.token(context).let { token -> ticked.value.values.filter { it.token == token }.mapNotNull { it.entry } }

    /** What to show now. */
    fun shown(context: Context): Shown {
        val token = Server.token(context)
        val ticked = ticked.value.filterValues { it.token == token }
        val (items, more) = listed(prefs(context), ticked.values.mapNotNull { it.row })
        return Shown(token != null, items, more, ticked.map { (task, t) -> task to t.occurrence }.toSet(), look(context), LocalDateTime.now().truncatedTo(ChronoUnit.MINUTES))
    }

    /** Hides a task done in session `token` from the list, as `Done.hide` says, unless signed in again
     * since. */
    suspend fun hide(context: Context, task: String, occurrence: Occurrence?, token: String, seq: Long?) = update(context) { prefs ->
        val mine = Server.token(context) == token
        if (mine) Done.hide(prefs, task, occurrence, seq, prefs.getLong(SEQ, 0).takeIf { prefs.contains(SEQ) })
        mine
    }

    /** Shows a task again whose completion, of its time `occurrence`, the server turned down. */
    suspend fun unhide(context: Context, task: String, occurrence: Occurrence?) = update(context) { Done.unhide(it, task, occurrence) }

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
     * meanwhile is dropped. A folder listed that has been renamed or deleted elsewhere leaves the list
     * as it is until the app hands over the paths as they are now. */
    suspend fun refresh(context: Context) {
        val token = Server.token(context) ?: return
        val picked = prefs(context).getString(FOLDERS, null)
        val folders = picked?.let(::JSONArray)
        val (data, seq) = try {
            Server.next(context, token, MOST, folders)
        } catch (e: HttpError) {
            if (e.status == 401) return ended(context, token)
            if (e.status == 404 && folders != null) return
            throw e
        }
        val tasks = data.getJSONArray("tasks")
        val start = data.optString("next_start").ifEmpty { null }
        // Other folders chosen meanwhile, it is a list of the ones before.
        update(context) {
            Server.token(context) == token && it.getString(FOLDERS, null) == picked &&
                keep(context, it, tasks.toString(), data.getInt("total") - tasks.length(), seq, start)
        }
    }

    /** Shows `task` at its time `occurrence` done where it is in the lists, whatever lists come, while it
     * is being completed in session `token`. */
    suspend fun tick(context: Context, task: String, occurrence: Occurrence?, token: String) {
        val row = shown(context).items.withIndex().find { it.value.id == task && it.value.occurrence == occurrence }
        ticked.update { it + (task to Ticked(occurrence, row, Agenda.place(context, task, occurrence), token)) }
        NextWidget().updateAll(context)
        WeekWidget().updateAll(context)
    }

    suspend fun untick(context: Context, task: String, token: String) {
        // Signed in again, the same task may have been ticked off in the new session meanwhile.
        ticked.update { if (it[task]?.token == token) it - task else it }
        NextWidget().updateAll(context)
        WeekWidget().updateAll(context)
    }

    /** The server says the session `token` has ended: unless the app has signed in again since, sign
     * out, and the widgets ask to sign in again. */
    suspend fun ended(context: Context, token: String) {
        var out = false
        update(context) { prefs -> Server.ended(context, token).also { out = it } && keep(context, prefs, "[]", 0, null, null) }
        if (out) Agenda.forget(context)
    }

    /** Empties the list: it is of data that is there no more (docs/local.md#数据代号). */
    suspend fun forget(context: Context) = update(context) { keep(context, it, "[]", 0, null, null) }

    suspend fun toast(context: Context, text: String) = withContext(Dispatchers.Main) {
        Toast.makeText(context, text, Toast.LENGTH_LONG).show()
    }

    /** When, in milliseconds, the clock here first reads `local`: now, should it have already; a time
     * skipped as summer time begins comes as it ends the skip; in the hour that leaving it repeats, the
     * one still to come. */
    fun firstReads(local: LocalDateTime): Long {
        val zone = ZoneId.systemDefault()
        val now = ZonedDateTime.now(zone)
        val change = zone.rules.getTransition(local)
        return when {
            !local.isAfter(now.toLocalDateTime()) -> now
            change == null -> local.atZone(zone)
            change.isGap -> change.instant.atZone(zone)
            else -> local.atZone(zone).let { if (it.isBefore(now)) it.withLaterOffsetAtOverlap() else it }
        }.toInstant().toEpochMilli()
    }

    /** Whether there is a “接下来” widget on the home screen to keep up to date. */
    fun placed(context: Context) = placed(context, NextWidgetReceiver::class.java)

    /** Whether there is a widget on the home screen that shows which tasks can be done, “接下来”, “一周”,
     * “时间轴” or “月历”: that changes as tasks start (`refreshAt`). */
    fun showsReady(context: Context) =
        placed(context) || listOf(WeekWidgetReceiver::class.java, TimelineWidgetReceiver::class.java, MonthWidgetReceiver::class.java).any { placed(context, it) }

    /** Whether there is a widget of `receiver`'s on the home screen. */
    fun placed(context: Context, receiver: Class<*>) =
        AppWidgetManager.getInstance(context).getAppWidgetIds(ComponentName(context, receiver)).isNotEmpty()

    /** Refreshes come while online, but in local mode not syncing, whose server is in the app. */
    private fun online(context: Context) = Constraints.Builder().setRequiredNetworkType(Local.network(context)).build()

    /** Every 15 minutes, the shortest WorkManager allows, for both widgets. Updates the refreshes there
     * are to the mode now, keeping when they come. */
    fun schedule(context: Context) {
        val request = PeriodicWorkRequestBuilder<RefreshWorker>(15, TimeUnit.MINUTES).setConstraints(online(context)).build()
        WorkManager.getInstance(context).enqueueUniquePeriodicWork(WORK, ExistingPeriodicWorkPolicy.UPDATE, request)
    }

    /** Fetches the list when the next task starts, `2026-11-16` or `2026-11-16 20:05` here, as the
     * `next` tool's `next_start` gives it: it comes into the list also with the app not running, and
     * that list sets the one after. An alarm, as reminders have, on time and in place of the one set
     * before; none without a start, or without a widget that shows which tasks can be done. Set only
     * with the list it came with, as that is kept. */
    private fun refreshAt(context: Context, start: String?) {
        val alarms = context.getSystemService(AlarmManager::class.java)
        val alarm = PendingIntent.getBroadcast(
            context,
            0,
            Intent(context, NextWidgetReceiver::class.java).setAction(NextWidgetReceiver.STARTED),
            PendingIntent.FLAG_IMMUTABLE,
        )
        if (start == null || !showsReady(context)) {
            alarms.cancel(alarm)
            return
        }
        val (date, time) = start.split(' ').let { it[0] to it.getOrNull(1) }
        // As starts are told by the clock here.
        val at = firstReads(LocalDate.parse(date).atTime(time?.let(LocalTime::parse) ?: LocalTime.MIDNIGHT))
        when {
            Reminders.exact(context) -> alarms.setExactAndAllowWhileIdle(AlarmManager.RTC_WAKEUP, at, alarm)
            else -> alarms.setAndAllowWhileIdle(AlarmManager.RTC_WAKEUP, at, alarm)
        }
    }

    /** A task has started, by the alarm `refreshAt` set, or other folders are chosen than the list
     * kept is of: the list now, as soon as online. */
    fun started(context: Context) {
        val request = OneTimeWorkRequestBuilder<RefreshWorker>().setConstraints(online(context)).build()
        WorkManager.getInstance(context).enqueueUniqueWork(START, ExistingWorkPolicy.REPLACE, request)
    }

    /** The last of a kind of widget is gone: the refreshes go with the last of all, the start's alarm
     * with the last that shows which can be done. */
    fun unschedule(context: Context) {
        if (!placed(context) && !Agenda.placed(context)) WorkManager.getInstance(context).cancelUniqueWork(WORK)
        if (!showsReady(context)) refreshAt(context, null)
    }

    private const val WORK = "widget-refresh"
    private const val START = "widget-start"
}

class RefreshWorker(context: Context, params: WorkerParameters) : CoroutineWorker(context, params) {
    override suspend fun doWork(): Result =
        try {
            // Syncing through a drive, with what other devices did meanwhile.
            if (Local.on(applicationContext)) withContext(Dispatchers.IO) { Local.pull(PULL_WAIT_MS) }
            // For the “一周” widget too, the list's next start.
            if (Widget.showsReady(applicationContext)) Widget.refresh(applicationContext)
            if (Agenda.placed(applicationContext)) Agenda.refresh(applicationContext)
            Result.success()
        } catch (e: Exception) {
            Result.retry()
        }

    private companion object {
        /** How long the list waits for what is new in the drive: it is fetched all the same then. */
        const val PULL_WAIT_MS = 5_000L
    }
}
