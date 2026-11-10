package cn.juniexd.nexttodo

import android.Manifest
import android.content.ContentProviderOperation
import android.content.ContentUris
import android.content.ContentValues
import android.content.Context
import android.content.pm.PackageManager
import android.net.Uri
import android.provider.CalendarContract
import android.provider.CalendarContract.Calendars
import android.provider.CalendarContract.Events
import cn.juniexd.nexttodo.widget.Due
import java.time.ZoneId
import java.time.ZoneOffset
import java.util.TimeZone
import org.json.JSONArray
import org.json.JSONObject

/** Writes the open tasks with a deadline into a calendar of the system's, one event each:
 * docs/android.md#写进系统日历. Calls come one at a time. */
object DeviceCalendar {
    /** Where the events go: the app's own calendar, or one of the device's that it may write to. */
    sealed interface Target {
        data object Own : Target

        data class Device(val id: Long) : Target
    }

    /** An event as written: all-day ones start at midnight UTC, as the provider wants them. */
    private data class Event(val title: String, val description: String, val start: Long, val end: Long, val allDay: Boolean)

    private const val TARGET = "target"

    /** The events written, by task: `{task: event id}`. */
    private const val WRITTEN = "written"

    /** The app's own calendar is the one of this local account. */
    private const val ACCOUNT = "NextTodo"
    private const val OWN = "${Calendars.ACCOUNT_NAME} = ? AND ${Calendars.ACCOUNT_TYPE} = ?"
    private val OWN_ARGS = arrayOf(ACCOUNT, CalendarContract.ACCOUNT_TYPE_LOCAL)

    /** The app's green, as in the web app's theme. */
    private const val COLOR = 0xFF1B6B51.toInt()

    /** Marks the events the app writes, so that in its own calendar those it wrote but has not noted,
     * such as one written just before the app was ended, can be told from others'. Not in an account
     * calendar: there the app on another device writes events marked the same. */
    private const val MARK = BuildConfig.APPLICATION_ID

    private val COLUMNS = arrayOf(Events._ID, Events.TITLE, Events.DESCRIPTION, Events.DTSTART, Events.DTEND, Events.ALL_DAY, Events.CUSTOM_APP_PACKAGE)

    private fun prefs(context: Context) = context.getSharedPreferences("calendar", Context.MODE_PRIVATE)

    private fun allowed(context: Context) = listOf(Manifest.permission.READ_CALENDAR, Manifest.permission.WRITE_CALENDAR)
        .all { context.checkSelfPermission(it) == PackageManager.PERMISSION_GRANTED }

    /** Where the events go, if anywhere. Writing ends when a calendar of the device's has gone, as with
     * the account removed, or may no longer be written to; what was written stays noted, for `clear`
     * to delete should it still be there. */
    @Synchronized
    fun target(context: Context): Target? {
        val target = when (val t = prefs(context).getString(TARGET, null)) {
            null -> return null
            "own" -> Target.Own
            else -> Target.Device(t.toLong())
        }
        if (target is Target.Device && allowed(context) && writable(context, "${Calendars._ID} = ?", arrayOf("${target.id}")).length() == 0) {
            prefs(context).edit().remove(TARGET).commit()
            return null
        }
        return target
    }

    /** The device's calendars the app may write to, but its own: `[{id, name, account, synced}]`,
     * `synced` unless the calendar is on this device alone. */
    @Synchronized
    fun calendars(context: Context): JSONArray = writable(context, "NOT ($OWN)", OWN_ARGS)

    private fun writable(context: Context, selection: String, args: Array<String>): JSONArray {
        val columns = arrayOf(Calendars._ID, Calendars.CALENDAR_DISPLAY_NAME, Calendars.ACCOUNT_NAME, Calendars.ACCOUNT_TYPE)
        val access = "${Calendars.CALENDAR_ACCESS_LEVEL} >= ${Calendars.CAL_ACCESS_CONTRIBUTOR}"
        val list = JSONArray()
        context.contentResolver.query(Calendars.CONTENT_URI, columns, "$access AND $selection", args, null)!!.use {
            while (it.moveToNext()) {
                list.put(
                    JSONObject().put("id", it.getLong(0)).put("name", it.getString(1)).put("account", it.getString(2))
                        .put("synced", it.getString(3) != CalendarContract.ACCOUNT_TYPE_LOCAL),
                )
            }
        }
        return list
    }

    /** Writes to `target` from now on, or nowhere without one: what was written before goes. */
    @Synchronized
    fun setTarget(context: Context, target: Target?) {
        if (target == target(context)) return
        clear(context)
        val value = when (target) {
            null -> null
            Target.Own -> "own"
            is Target.Device -> "${target.id}"
        }
        prefs(context).edit().putString(TARGET, value).commit()
    }

    /** Deletes the events written, in the own calendar also those not noted, and the own calendar
     * unless another app added events to it, keeping where to write: as when signing out. The own calendar goes
     * whoever made it, such as an earlier install of the app. Without the permission nothing goes,
     * and what was written stays noted: the next update, once allowed, deletes what is no longer
     * wanted. */
    @Synchronized
    fun clear(context: Context) {
        if (!allowed(context)) return
        val ids = written(context).values
        val noted = if (ids.isEmpty()) "" else " OR ${Events._ID} IN (${ids.joinToString()})"
        context.contentResolver.delete(Events.CONTENT_URI, "(${Events.CUSTOM_APP_PACKAGE} = ? AND $OWN)$noted", arrayOf(MARK, *OWN_ARGS))
        val others = context.contentResolver.query(Events.CONTENT_URI, arrayOf(Events._ID), "$OWN AND ${Events.DELETED} = 0", OWN_ARGS, null)!!.use { it.count }
        if (others == 0) context.contentResolver.delete(asOwner(Calendars.CONTENT_URI), OWN, OWN_ARGS)
        prefs(context).edit().remove(WRITTEN).commit()
    }

    /** Makes the calendar show `events`, `[{task, title, description, due}]`: what differs from what
     * was written, or is no longer there, as when the user deleted or changed it, is written again. */
    @Synchronized
    fun update(context: Context, events: String) {
        val target = target(context) ?: return
        if (!allowed(context)) return
        val calendar = when (target) {
            Target.Own -> own(context)
            is Target.Device -> target.id
        }
        val wanted = parse(events)
        val written = written(context)
        val ours = written.values.toSet()
        val found = HashMap<Long, Event>()
        val ops = ArrayList<ContentProviderOperation>()
        val selection = "${Events.CALENDAR_ID} = ? AND ${Events.DELETED} = 0"
        context.contentResolver.query(Events.CONTENT_URI, COLUMNS, selection, arrayOf("$calendar"), null)!!.use {
            while (it.moveToNext()) {
                val id = it.getLong(0)
                if (id in ours) {
                    found[id] = Event(it.getString(1).orEmpty(), it.getString(2).orEmpty(), it.getLong(3), it.getLong(4), it.getInt(5) == 1)
                } else if (target == Target.Own && it.getString(6) == MARK) {
                    // Written by a run that ended before it could note the event, or an earlier install.
                    ops += ContentProviderOperation.newDelete(event(id)).build()
                }
            }
        }
        val kept = HashMap<String, Long>()
        // The tasks of the events inserted, by the index of their insert.
        val inserted = HashMap<Int, String>()
        for ((task, e) in wanted) {
            val id = written[task]
            val now = id?.let(found::get)
            if (now == null) {
                inserted[ops.size] = task
                ops += ContentProviderOperation.newInsert(Events.CONTENT_URI).withValues(values(e)).withValue(Events.CALENDAR_ID, calendar).build()
                continue
            }
            kept[task] = id
            if (now != e) ops += ContentProviderOperation.newUpdate(event(id)).withValues(values(e)).build()
        }
        for ((task, id) in written) if (task !in wanted && id in found) ops += ContentProviderOperation.newDelete(event(id)).build()
        if (ops.isNotEmpty()) {
            val results = context.contentResolver.applyBatch(CalendarContract.AUTHORITY, ops)
            for ((i, task) in inserted) kept[task] = ContentUris.parseId(results[i].uri!!)
        }
        prefs(context).edit().putString(WRITTEN, JSONObject(kept).toString()).commit()
    }

    private fun event(id: Long) = ContentUris.withAppendedId(Events.CONTENT_URI, id)

    /** The app's own calendar, made if there is none. Read-only to calendar apps, which then neither
     * add events to it nor change the app's: changes there would not reach the tasks. */
    private fun own(context: Context): Long {
        context.contentResolver.query(Calendars.CONTENT_URI, arrayOf(Calendars._ID), OWN, OWN_ARGS, null)!!.use {
            if (it.moveToFirst()) return it.getLong(0)
        }
        val values = ContentValues().apply {
            put(Calendars.ACCOUNT_NAME, ACCOUNT)
            put(Calendars.ACCOUNT_TYPE, CalendarContract.ACCOUNT_TYPE_LOCAL)
            put(Calendars.OWNER_ACCOUNT, ACCOUNT)
            put(Calendars.NAME, ACCOUNT)
            put(Calendars.CALENDAR_DISPLAY_NAME, "NextTodo")
            put(Calendars.CALENDAR_COLOR, COLOR)
            put(Calendars.CALENDAR_ACCESS_LEVEL, Calendars.CAL_ACCESS_READ)
            put(Calendars.CALENDAR_TIME_ZONE, TimeZone.getDefault().id)
            put(Calendars.VISIBLE, 1)
            put(Calendars.SYNC_EVENTS, 1)
        }
        return ContentUris.parseId(context.contentResolver.insert(asOwner(Calendars.CONTENT_URI), values)!!)
    }

    /** Only the account's sync adapter makes and deletes its calendars: for a local account, the app. */
    private fun asOwner(uri: Uri) = uri.buildUpon()
        .appendQueryParameter(CalendarContract.CALLER_IS_SYNCADAPTER, "true")
        .appendQueryParameter(Calendars.ACCOUNT_NAME, ACCOUNT)
        .appendQueryParameter(Calendars.ACCOUNT_TYPE, CalendarContract.ACCOUNT_TYPE_LOCAL)
        .build()

    private fun written(context: Context): Map<String, Long> {
        val o = JSONObject(prefs(context).getString(WRITTEN, null) ?: "{}")
        return o.keys().asSequence().associateWith { o.getLong(it) }
    }

    /** A date alone is that day's all-day event; one with a time, an event at that moment, here. */
    private fun parse(events: String): Map<String, Event> {
        val list = JSONArray(events)
        return (0 until list.length()).associate { i ->
            val o = list.getJSONObject(i)
            val due = Due.from(o.getJSONObject("due"))
            val (start, end) = when (val time = due.time) {
                null -> due.date.atStartOfDay(ZoneOffset.UTC).let { it to it.plusDays(1) }
                else -> due.date.atTime(time).atZone(ZoneId.systemDefault()).let { it to it }
            }
            o.getString("task") to Event(o.getString("title"), o.getString("description"), start.toInstant().toEpochMilli(), end.toInstant().toEpochMilli(), due.time == null)
        }
    }

    /** Free, so that a task's day does not show busy to whoever sees the calendar's availability. */
    private fun values(e: Event) = ContentValues().apply {
        put(Events.TITLE, e.title)
        put(Events.DESCRIPTION, e.description)
        put(Events.DTSTART, e.start)
        put(Events.DTEND, e.end)
        put(Events.ALL_DAY, if (e.allDay) 1 else 0)
        put(Events.EVENT_TIMEZONE, if (e.allDay) "UTC" else TimeZone.getDefault().id)
        put(Events.AVAILABILITY, Events.AVAILABILITY_FREE)
        put(Events.CUSTOM_APP_PACKAGE, MARK)
    }
}
