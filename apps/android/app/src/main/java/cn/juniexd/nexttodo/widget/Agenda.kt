package cn.juniexd.nexttodo.widget

import android.content.Context
import android.content.SharedPreferences
import androidx.glance.appwidget.updateAll
import cn.juniexd.nexttodo.Due
import cn.juniexd.nexttodo.HttpError
import cn.juniexd.nexttodo.Server
import java.time.LocalDate
import java.time.LocalDateTime
import java.time.temporal.ChronoUnit
import kotlinx.coroutines.channels.awaitClose
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.callbackFlow
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import org.json.JSONArray
import org.json.JSONObject

/** A task on a day of the agenda: due then, or `later`, a time a repeating one is due after the one
 * due now. */
data class Entry(val id: String, val title: String, val folder: String, val due: Due, val later: Boolean)

/** What is due on `date`; on none, what is overdue. */
data class Day(val date: LocalDate?, val entries: List<Entry>)

/** What the agenda widget shows: the days with something due, the first tasks of them and how many
 * `more` there are; or that the app is signed out. With the `look` the user chose. As of `now`, to
 * the minute, which the days and what is late go by: a part of it, so that the widget is drawn again
 * as the time alone moves on. */
data class Agendas(val signedIn: Boolean, val days: List<Day>, val more: Int, val look: Look, val now: LocalDateTime)

/** The agenda widget's data, the `agenda` tool's for the next `DAYS` days, kept in its own preferences:
 * written by the app, or fetched from the server, as the “接下来” widget's list is (`Widget`). */
object Agenda {
    private fun prefs(context: Context) = context.getSharedPreferences("agenda", Context.MODE_PRIVATE)

    /** How many days from today it lists. */
    const val DAYS = 7

    /** The most tasks it lists, the rest only counted: see `Widget`'s. */
    private const val MOST = 30

    /** The data, as of the change `SEQ`, for the folders `FOLDERS` (`Widget.folders`), as `Widget`
     * keeps its list. */
    private const val DATA = "agenda"
    private const val SEQ = "seq"
    private const val FOLDERS = "folders"

    /** Until when what is drawn holds: the next deadline, which turns a time red, or midnight, when the
     * days move on. */
    private const val UNTIL = "until"

    private const val EMPTY = """{"overdue":[],"days":[]}"""

    fun placed(context: Context) = Widget.placed(context, AgendaWidgetReceiver::class.java)

    private val writing = Mutex()

    /** Runs `change` on the preferences in turn, and draws the widget again when it says so, or when
     * what is drawn no longer holds: not on each list the app hands over as it starts, as Glance draws
     * on the main thread and would hold back the app's first frame. */
    private suspend fun update(context: Context, change: (SharedPreferences) -> Boolean) {
        val draw = writing.withLock {
            val prefs = prefs(context)
            val now = LocalDateTime.now()
            val holds = prefs.getString(UNTIL, null)?.let { LocalDateTime.parse(it).isAfter(now) } == true
            (change(prefs) || !holds).also { if (it) prefs.edit().putString(UNTIL, until(prefs, now).toString()).apply() }
        }
        if (draw) AgendaWidget().updateAll(context)
    }

    private fun until(prefs: SharedPreferences, now: LocalDateTime): LocalDateTime {
        val (overdue, days) = kept(prefs)
        val deadlines = (overdue + days.flatMap { it.second }).map { it.due.deadline }
        return (deadlines + now.toLocalDate().plusDays(1).atStartOfDay()).filter { it.isAfter(now) }.min()
    }

    /** Keeps `data` for `folders` as of the change `seq`, unless what is kept is as of a later one;
     * returns whether it was kept, and whether that changed it. */
    private fun keep(prefs: SharedPreferences, data: String, folders: String?, seq: Long?): Pair<Boolean, Boolean> {
        if (seq != null && prefs.contains(SEQ) && seq < prefs.getLong(SEQ, 0)) return false to false
        val changed = prefs.getString(DATA, EMPTY) != data
        val edit = prefs.edit().putString(DATA, data).putString(FOLDERS, folders)
        if (seq == null) edit.remove(SEQ) else edit.putLong(SEQ, seq)
        edit.apply()
        return true to changed
    }

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

    /** Empties it: signed out, or its data is there no more. */
    suspend fun forget(context: Context) = update(context) { keep(it, EMPTY, null, null).second }

    /** Fetches it from the server when signed in, as `Widget.refresh` does its list. */
    suspend fun refresh(context: Context) {
        // What is drawn may no longer hold, whether or not the server answers.
        update(context) { false }
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

    /** What to show now: what was due before today and is still open goes with the overdue, and the
     * later times of repeating ones then go. */
    fun shown(context: Context): Agendas {
        val now = LocalDateTime.now().truncatedTo(ChronoUnit.MINUTES)
        val (before, days) = kept(prefs(context))
        val (past, coming) = days.partition { (date, _) -> date.isBefore(now.toLocalDate()) }
        val overdue = before + past.flatMap { (_, entries) -> entries.filter { !it.later } }
        val all = listOf(Day(null, overdue)) + coming.map { (date, entries) -> Day(date, entries) }
        // The first `MOST` tasks, and the days they are on.
        var left = MOST
        val listed = all.mapNotNull { day -> Day(day.date, day.entries.take(left)).also { left -= it.entries.size }.takeIf { it.entries.isNotEmpty() } }
        return Agendas(Server.token(context) != null, listed, all.sumOf { it.entries.size } - MOST + left, Widget.look(context), now)
    }

    /** What is kept: the tasks overdue, and those due each day. */
    private fun kept(prefs: SharedPreferences): Pair<List<Entry>, List<Pair<LocalDate, List<Entry>>>> {
        val data = JSONObject(prefs.getString(DATA, EMPTY)!!)
        val days = data.getJSONArray("days")
        return entries(data.getJSONArray("overdue")) to
            (0 until days.length()).map(days::getJSONObject).map { LocalDate.parse(it.getString("date")) to entries(it.getJSONArray("tasks")) }
    }

    private fun entries(list: JSONArray) = (0 until list.length()).map(list::getJSONObject).map { t ->
        // `/学习/毕业论文` reads `学习 / 毕业论文`; the root shows nothing.
        Entry(t.getString("id"), t.getString("title"), t.getString("folder").removePrefix("/").replace("/", " / "), Due.from(t.getJSONObject("due")), t.optBoolean("later"))
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
}
