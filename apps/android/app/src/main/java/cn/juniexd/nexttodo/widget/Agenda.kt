package cn.juniexd.nexttodo.widget

import android.app.AlarmManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.content.SharedPreferences
import androidx.glance.appwidget.updateAll
import cn.juniexd.nexttodo.Due
import cn.juniexd.nexttodo.HttpError
import cn.juniexd.nexttodo.Occurrence
import cn.juniexd.nexttodo.Server
import cn.juniexd.nexttodo.reminders.Reminders
import java.time.LocalDate
import java.time.LocalDateTime
import java.time.temporal.ChronoUnit
import kotlinx.coroutines.channels.awaitClose
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.callbackFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import org.json.JSONArray
import org.json.JSONObject

/** A task on a day of the agenda: due then, or `later`, a time a repeating one is due after the one
 * due now; `ready` to do, not waiting for others or its start; `occurrence` the time due now, should it
 * repeat, for completing it. */
data class Entry(val id: String, val title: String, val folder: String, val due: Due, val later: Boolean, val ready: Boolean, val occurrence: Occurrence?) {
    /** Whether both are the time due now of a task, the same one. */
    fun same(other: Entry) = !later && !other.later && id == other.id && occurrence == other.occurrence
}

/** What is due on `date`; on none, what is overdue. */
data class Day(val date: LocalDate?, val entries: List<Entry>)

/** What the agenda widget shows: the days with something due, the first tasks of them and how many
 * `more` there are; or that the app is signed out. With the `look` the user chose. As of `now`, to
 * the minute, which the days and what is late go by: a part of it, so that the widget is drawn again
 * as the time alone moves on. */
data class Agendas(val signedIn: Boolean, val days: List<Day>, val more: Int, val look: Look, val now: LocalDateTime)

/** What the “一周” widget shows: its seven `days`, what is due on each of them, open and due before
 * today for the days before it, and what is overdue; the tasks being ticked off; as `Agendas`. */
data class Week(
    val signedIn: Boolean,
    val days: List<LocalDate>,
    val due: Map<LocalDate, List<Entry>>,
    val overdue: List<Entry>,
    val ticked: List<Entry>,
    val look: Look,
    val now: LocalDateTime,
)

/** The agenda widget's data, the `agenda` tool's for the next `DAYS` days, kept in its own preferences:
 * written by the app, or fetched from the server, as the “接下来” widget's list is (`Widget`). */
object Agenda {
    private fun prefs(context: Context) = context.getSharedPreferences("agenda", Context.MODE_PRIVATE)

    /** How many days from today it lists. */
    const val DAYS = 7

    /** The most tasks it lists, the rest only counted: see `Widget`'s. */
    const val MOST = 30

    /** The data, as of the change `SEQ`, for the folders `FOLDERS` (`Widget.folders`), as `Widget`
     * keeps its list. */
    private const val DATA = "agenda"
    private const val SEQ = "seq"
    private const val FOLDERS = "folders"

    /** Until when what is drawn holds: the next deadline, which turns a time red, midnight, when the
     * days move on, or the next hour, which the time grid shows (`until`). */
    private const val UNTIL = "until"

    private const val EMPTY = """{"overdue":[],"days":[]}"""

    /** Whether a widget of its data, “日程”, “一周” or “时间轴”, is on the home screen. */
    fun placed(context: Context) = listOf(AgendaWidgetReceiver::class.java, WeekWidgetReceiver::class.java, TimelineWidgetReceiver::class.java).any { Widget.placed(context, it) }

    private val writing = Mutex()

    /** Runs `change` on the preferences in turn, and draws the widget again when it says so, or when
     * what is drawn no longer holds: not on each list the app hands over as it starts, as Glance draws
     * on the main thread and would hold back the app's first frame. */
    private suspend fun update(context: Context, change: (SharedPreferences) -> Boolean) {
        val draw = writing.withLock {
            val prefs = prefs(context)
            val now = LocalDateTime.now()
            val holdsUntil = prefs.getString(UNTIL, null)?.let(LocalDateTime::parse)?.takeIf { it.isAfter(now) }
            val draw = change(prefs) || holdsUntil == null
            redrawAt(context, holdsUntil.takeUnless { draw } ?: until(context, prefs, now).also { prefs.edit().putString(UNTIL, it.toString()).apply() })
            draw
        }
        if (draw) {
            AgendaWidget().updateAll(context)
            WeekWidget().updateAll(context)
            TimelineWidget().updateAll(context)
        }
    }

    /** Draws the widgets again at `at`, when what they show of the time comes to be otherwise, offline
     * too, which the refreshes are not; not waking the phone for it, none seeing them meanwhile. */
    private fun redrawAt(context: Context, at: LocalDateTime) {
        val alarms = context.getSystemService(AlarmManager::class.java)
        val alarm = PendingIntent.getBroadcast(
            context,
            0,
            Intent(context, AgendaWidgetReceiver::class.java).setAction(AgendaWidgetReceiver.REDRAW),
            PendingIntent.FLAG_IMMUTABLE,
        )
        if (!placed(context)) {
            alarms.cancel(alarm)
            return
        }
        // As deadlines are told by the clock here.
        val millis = Widget.firstReads(at)
        when {
            Reminders.exact(context) -> alarms.setExact(AlarmManager.RTC, millis, alarm)
            else -> alarms.setWindow(AlarmManager.RTC, millis, WINDOW_MS, alarm)
        }
    }

    /** How late the alarm may come without the permission for exact ones: the least the system allows. */
    private const val WINDOW_MS = 10 * 60_000L

    /** Draws the widgets again, `anyway` or should what they show of the time no longer hold. */
    suspend fun redraw(context: Context, anyway: Boolean = false) = update(context) { anyway }

    /** A widget of its data put on the home screen, or the last of a kind taken off, which may change
     * until when what is drawn holds: drawn again at once, offline too, and the alarm set for what is
     * drawn then; or it goes. */
    fun placedChanged(context: Context) {
        prefs(context).edit().remove(UNTIL).apply()
        redrawAt(context, LocalDateTime.now())
    }

    /** The next deadline, midnight, or with a “时间轴” widget, which shows the hour now, the next hour. */
    private fun until(context: Context, prefs: SharedPreferences, now: LocalDateTime): LocalDateTime {
        val (overdue, days) = kept(prefs)
        val deadlines = (overdue + days.flatMap { it.second }).map { it.due.deadline }
        val hour = now.truncatedTo(ChronoUnit.HOURS).plusHours(1).takeIf { Widget.placed(context, TimelineWidgetReceiver::class.java) }
        return (deadlines + listOfNotNull(hour) + now.toLocalDate().plusDays(1).atStartOfDay()).filter { it.isAfter(now) }.min()
    }

    /** Keeps `data` for `folders` as of the change `seq`, unless what is kept is as of a later one;
     * returns whether it was kept, and whether that changed what the widgets draw. */
    private fun keep(prefs: SharedPreferences, data: String, folders: String?, seq: Long?): Pair<Boolean, Boolean> {
        if (seq != null && prefs.contains(SEQ) && seq < prefs.getLong(SEQ, 0)) return false to false
        val was = drawn(prefs)
        val edit = prefs.edit().putString(DATA, data).putString(FOLDERS, folders)
        if (seq == null) edit.remove(SEQ) else edit.putLong(SEQ, seq)
        Done.keep(prefs, edit, seq)
        edit.apply()
        return true to (drawn(prefs) != was)
    }

    /** What the widgets draw of the preferences, but for what they say of the time, as `Widget`'s. */
    private fun drawn(prefs: SharedPreferences) = kept(prefs) to prefs.contains(SEQ)

    /** Shows `data`, the app's, in the shape of the `agenda` tool's, as of the change `seq` its copy has.
     * A list of other folders than the one kept, but older, is not taken either: the widgets fetch their
     * lists of those folders themselves, as `Widget.show` says. */
    suspend fun show(context: Context, data: String, seq: Long?) = update(context) { prefs ->
        val folders = Widget.folders(context)
        val refiltered = prefs.getString(FOLDERS, null) != folders
        val (kept, changed) = keep(prefs, data, folders, seq)
        if (refiltered && !kept) Widget.started(context)
        changed
    }

    /** Hides a task done in session `token` from what is due now, its later times staying, as
     * `Widget.hide` does from its list. */
    suspend fun hide(context: Context, task: String, occurrence: Occurrence?, token: String, seq: Long?) = update(context) { prefs ->
        val mine = Server.token(context) == token
        if (mine) Done.hide(prefs, task, occurrence, seq, prefs.getLong(SEQ, 0).takeIf { prefs.contains(SEQ) })
        mine
    }

    /** Shows a task again whose completion, of its time `occurrence`, the server turned down. */
    suspend fun unhide(context: Context, task: String, occurrence: Occurrence?) = update(context) { Done.unhide(it, task, occurrence) }

    /** Empties it: signed out, or its data is there no more. Drawn again, empty as it may have been, to
     * say so. */
    suspend fun forget(context: Context) = update(context) { keep(it, EMPTY, null, null).first }

    /** Fetches it from the server when signed in, as `Widget.refresh` does its list. */
    suspend fun refresh(context: Context) {
        // What is drawn may no longer hold, whether or not the server answers.
        redraw(context)
        val token = Server.token(context) ?: return
        val picked = Widget.folders(context)
        val (data, seq) = try {
            Server.agenda(context, token, DAYS, picked?.let(::JSONArray))
        } catch (e: HttpError) {
            if (e.status == 401) return Widget.ended(context, token)
            if (e.status == 404 && picked != null) return
            throw e
        }
        update(context) { Server.token(context) == token && Widget.folders(context) == picked && keep(it, data.toString(), picked, seq).second }
    }

    /** What to show now. */
    fun shown(context: Context): Agendas {
        val now = LocalDateTime.now().truncatedTo(ChronoUnit.MINUTES)
        val today = now.toLocalDate()
        val (overdue, coming) = dated(kept(prefs(context)), today)
        // The clock turned back, what is kept may go on past the days from today.
        val days = coming.filter { (date, _) -> date.isBefore(today.plusDays(DAYS.toLong())) }
        val all = listOf(Day(null, overdue)) + days.map { (date, entries) -> Day(date, entries) }
        // The first `MOST` tasks, and the days they are on.
        var left = MOST
        val listed = all.mapNotNull { day -> Day(day.date, day.entries.take(left)).also { left -= it.entries.size }.takeIf { it.entries.isNotEmpty() } }
        return Agendas(Server.token(context) != null, listed, all.sumOf { it.entries.size } - MOST + left, Widget.look(context), now)
    }

    /** What the “一周” widget shows now: from today, or from this week's Monday as its look says; the
     * days before today have what was due then and is open still, which the overdue has. */
    fun week(context: Context): Week {
        val now = LocalDateTime.now().truncatedTo(ChronoUnit.MINUTES)
        val today = now.toLocalDate()
        val look = Widget.look(context)
        val first = if (look.week == "monday") today.minusDays(today.dayOfWeek.value - 1L) else today
        val ticked = Widget.ticked(context)
        val stored = kept(prefs(context), ticked.map { it.value })
        val (before, coming) = dated(stored, today)
        // One being completed stays where it was, whatever lists come meanwhile, as in the “接下来” widget.
        val listed = stored.first + stored.second.flatMap { it.second }
        val (late, soon) = ticked.filter { (_, t) -> listed.none(t::same) }.sortedBy { it.index }.partition { it.value.due.date.isBefore(today) }
        val overdue = before.toMutableList()
        for ((i, t) in late) overdue.add(minOf(i, overdue.size), t)
        val due = coming.associate { (date, entries) -> date to entries.toMutableList() }.toMutableMap()
        for ((i, t) in soon) due.getOrPut(t.due.date) { mutableListOf() }.let { it.add(minOf(i, it.size), t) }
        return Week(Server.token(context) != null, (0L..6L).map(first::plusDays), due + overdue.groupBy { it.due.date }, overdue, ticked.map { it.value }, look, now)
    }

    /** Where `task`'s time `occurrence`, the one due now, is in the “一周” widget's lists: with what is
     * overdue, or due on its day. */
    fun place(context: Context, task: String, occurrence: Occurrence?): IndexedValue<Entry>? {
        val week = week(context)
        return (listOf(week.overdue) + week.due.values).firstNotNullOfOrNull { list ->
            list.withIndex().find { it.value.id == task && !it.value.later && it.value.occurrence == occurrence }
        }
    }

    /** What is `kept`, as of `today`: what is overdue, with what was due on the days before and is open
     * still, but for the later times of repeating ones; and what is due each day from today. What the
     * list has as overdue but is not, the clock having been turned back, goes first on its day. */
    private fun dated(kept: Pair<List<Entry>, List<Pair<LocalDate, List<Entry>>>>, today: LocalDate): Pair<List<Entry>, List<Pair<LocalDate, List<Entry>>>> {
        val (overdue, ahead) = kept.first.partition { it.due.date.isBefore(today) }
        val (past, coming) = kept.second.partition { (date, _) -> date.isBefore(today) }
        val days = (ahead.groupBy { it.due.date }.toList() + coming).groupBy({ it.first }, { it.second }).mapValues { it.value.flatten() }.toSortedMap()
        return overdue + past.flatMap { (_, entries) -> entries.filter { !it.later } } to days.toList()
    }

    /** What is kept: the tasks overdue, and those due each day; but for those done here, unless
     * `ticked` off and being completed, which stay where they are meanwhile. */
    private fun kept(prefs: SharedPreferences, ticked: List<Entry> = emptyList()): Pair<List<Entry>, List<Pair<LocalDate, List<Entry>>>> {
        val data = JSONObject(prefs.getString(DATA, EMPTY)!!)
        val done = Done.of(prefs)
        val entries = { list: JSONArray -> entries(objects(list)).filter { it.later || !Done.hides(done, it.id, it.occurrence) || ticked.any(it::same) } }
        return entries(data.getJSONArray("overdue")) to objects(data.getJSONArray("days")).map { LocalDate.parse(it.getString("date")) to entries(it.getJSONArray("tasks")) }
    }

    private fun objects(list: JSONArray) = (0 until list.length()).map(list::getJSONObject)

    private fun entries(list: List<JSONObject>) = list.map { t ->
        // `/学习/毕业论文` reads `学习 / 毕业论文`; the root shows nothing. Kept before there was a
        // status, one is not to be ticked off until the app or the server writes the data again: it may
        // be waiting, and its time due now is not known.
        val ready = t.optString("status") == "ready"
        Entry(t.getString("id"), t.getString("title"), t.getString("folder").removePrefix("/").replace("/", " / "), Due.from(t.getJSONObject("due")), t.optBoolean("later"), ready, Occurrence.of(t))
    }

    /** What to show, each time the data, the sign-in or the look changes. */
    fun changes(context: Context): Flow<Agendas> = callbackFlow {
        val watched = listOf(prefs(context), Server.prefs(context), Widget.prefs(context))
        val listener = SharedPreferences.OnSharedPreferenceChangeListener { _, _ -> trySend(shown(context)) }
        watched.forEach { it.registerOnSharedPreferenceChangeListener(listener) }
        // What to show at once too: it may have changed since read for the first composition.
        trySend(shown(context))
        awaitClose { watched.forEach { it.unregisterOnSharedPreferenceChangeListener(listener) } }
    }

    /** What the “一周” widget shows, each time the data, the sign-in or the look changes, or a task is
     * ticked off. */
    fun weekChanges(context: Context): Flow<Week> = callbackFlow {
        val watched = listOf(prefs(context), Server.prefs(context), Widget.prefs(context))
        val listener = SharedPreferences.OnSharedPreferenceChangeListener { _, _ -> trySend(week(context)) }
        watched.forEach { it.registerOnSharedPreferenceChangeListener(listener) }
        // Also sends what to show at once, as `changes` does.
        launch { Widget.ticking.collect { trySend(week(context)) } }
        awaitClose { watched.forEach { it.unregisterOnSharedPreferenceChangeListener(listener) } }
    }
}
