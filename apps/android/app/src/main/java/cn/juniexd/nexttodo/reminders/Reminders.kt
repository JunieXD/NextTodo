package cn.juniexd.nexttodo.reminders

import android.app.AlarmManager
import android.app.KeyguardManager
import android.app.Notification
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.content.SharedPreferences
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.os.PowerManager
import android.provider.Settings
import androidx.compose.ui.graphics.toArgb
import cn.juniexd.nexttodo.Channels
import cn.juniexd.nexttodo.Due
import cn.juniexd.nexttodo.MainActivity
import cn.juniexd.nexttodo.Palette
import cn.juniexd.nexttodo.R
import java.time.LocalDate
import java.time.LocalDateTime
import org.json.JSONArray
import org.json.JSONObject

/** A task's reminder, as the app hands them over (`apps/web/src/android.ts`): at `at`, in
 * milliseconds; `waiting`, how many of its prerequisites are not done; `start`, its start date. */
data class Reminder(val task: String, val number: Int, val at: Long, val title: String, val body: String, val waiting: Int, val start: LocalDate?) {
    /** Whether it can be done straight from the reminder on `today`: it waits for nothing. */
    fun ready(today: LocalDate) = waiting == 0 && (start == null || !start.isAfter(today))

    /** What the reminder card says under the title: what the notification says, and what it waits for. */
    fun meta(now: LocalDateTime): String {
        val waits = when {
            waiting > 0 -> "等 $waiting 项"
            start != null && start.isAfter(now.toLocalDate()) -> "${Due(start, null).label(now)}开始"
            else -> null
        }
        return listOfNotNull(body, waits).joinToString(" · ")
    }

    companion object {
        fun from(o: JSONObject) = Reminder(
            o.getString("task"),
            o.getInt("number"),
            o.getLong("at"),
            o.getString("title"),
            o.getString("body"),
            o.getInt("waiting"),
            if (o.isNull("start")) null else LocalDate.parse(o.getString("start")),
        )
    }
}

/**
 * The reminders, scheduled and shown here (docs/android.md#截止提醒): the list the app hands over,
 * kept in preferences, and one alarm at the next of them. Each time the alarm goes off or a list comes,
 * the reminders due since the last time are shown. Everything here runs in turn.
 */
object Reminders {
    private fun prefs(context: Context) = context.getSharedPreferences("reminders", Context.MODE_PRIVATE)

    /** The reminders handed over, as JSON. */
    private const val LIST = "list"

    /** Up to when reminders have been shown, in milliseconds. */
    private const val MARK = "mark"

    /** Reminders put off, by task: `{at, until}`, the time of the reminder and when to show it again. */
    private const val SNOOZED = "snoozed"

    /** Whether the reminder card is on (docs/android.md#弹窗提醒). */
    private const val CARD = "card"

    /** Whether the app has asked for notifications, which it does once. */
    private const val ASKED = "asked"

    /** Notification extras: which task and reminder time a notification is of. */
    private const val TASK = "nexttodo.task"
    private const val AT = "nexttodo.at"

    /** And whether it has “完成”. */
    private const val READY = "nexttodo.ready"

    fun list(context: Context): List<Reminder> {
        val list = JSONArray(prefs(context).getString(LIST, "[]"))
        return (0 until list.length()).map { Reminder.from(list.getJSONObject(it)) }
    }

    fun find(context: Context, task: String) = list(context).find { it.task == task }

    private fun snoozed(context: Context) = JSONObject(prefs(context).getString(SNOOZED, "{}"))

    /** The reminders still to show: none done, deleted or put off since. */
    fun current(context: Context): List<Reminder> {
        val snoozed = snoozed(context)
        return list(context).filter { !snoozed.has(it.task) }
    }

    /** Calls `changed`, on the main thread, each time the reminders change; returns how to stop. */
    fun watch(context: Context, changed: () -> Unit): () -> Unit {
        val listener = SharedPreferences.OnSharedPreferenceChangeListener { _, _ -> changed() }
        prefs(context).registerOnSharedPreferenceChangeListener(listener)
        return { prefs(context).unregisterOnSharedPreferenceChangeListener(listener) }
    }

    private fun JSONObject.entries() = keys().asSequence().toList().map { it to getJSONObject(it) }

    /** Keeps `list` from now on, as JSON, and shows what came due of what was scheduled already: the
     * alarm may have come late, or been held back. One just set in the app, already past, is not
     * shown. Returns the reminders for the card, as `show` does. */
    @Synchronized
    fun update(context: Context, list: String): List<Reminder> {
        val scheduled = times(context).map { (r, at) -> r.task to at }.toSet()
        keep(context, list)
        return show(context) { r, at -> (r.task to at) in scheduled }
    }

    /** Keeps `list`, as JSON. A reminder put off stays so while its time is the same. Notifications
     * shown for reminders gone or moved go; one shown otherwise than it is now, its task renamed, come
     * to wait for another or its start date come, shows as it is, without a sound. The alarm moves to
     * the next. */
    private fun keep(context: Context, list: String) {
        val today = LocalDate.now()
        val wanted = JSONArray(list).let { a -> (0 until a.length()).map { Reminder.from(a.getJSONObject(it)) } }.associateBy { it.task }
        val snoozed = snoozed(context)
        for ((task, s) in snoozed.entries()) if (wanted[task]?.at != s.getLong("at")) snoozed.remove(task)
        prefs(context).edit().putString(LIST, list).putString(SNOOZED, snoozed.toString()).commit()
        val manager = context.getSystemService(NotificationManager::class.java)
        for (shown in manager.activeNotifications) {
            // Reminders only, as in `withdraw`.
            val extras = shown.notification.extras
            val task = extras.getString(TASK) ?: continue
            val r = wanted[task]
            if (r == null || r.at != extras.getLong(AT)) {
                manager.cancel(shown.id)
            } else if (
                extras.getCharSequence(Notification.EXTRA_TITLE).toString() != r.title ||
                extras.getCharSequence(Notification.EXTRA_TEXT).toString() != r.body ||
                extras.getBoolean(READY) != r.ready(today)
            ) {
                post(context, r, shown.notification.channelId == Channels.DUE_QUIET)
            }
        }
        schedule(context)
    }

    /** Shows the reminders that came due since the last time, those put off too, of those `which` picks,
     * and sets the alarm for the next. Returns those to show on the reminder card as well, when it is to be. */
    @Synchronized
    fun show(context: Context, which: (Reminder, at: Long) -> Boolean = { _, _ -> true }): List<Reminder> {
        val now = System.currentTimeMillis()
        val mark = mark(context, now)
        val snoozed = snoozed(context)
        val due = times(context).filter { (r, at) -> at in (mark + 1)..now && which(r, at) }.map { it.first }
        for (r in due) snoozed.remove(r.task)
        prefs(context).edit().putLong(MARK, now).putString(SNOOZED, snoozed.toString()).commit()
        schedule(context)
        val card = cardNow(context)
        for (r in due) post(context, r, card)
        return if (card) due else emptyList()
    }

    /** Puts `task`'s reminder off for `minutes`: its notification goes, and comes again then. */
    @Synchronized
    fun snooze(context: Context, task: String, minutes: Int) {
        val r = find(context, task) ?: return
        val snoozed = snoozed(context).put(task, JSONObject().put("at", r.at).put("until", System.currentTimeMillis() + minutes * 60_000L))
        prefs(context).edit().putString(SNOOZED, snoozed.toString()).commit()
        context.getSystemService(NotificationManager::class.java).cancel(r.number)
        schedule(context)
    }

    /** `task` is done from its reminder `at`: that reminder goes, shown or not, put off or not. One the
     * app has handed over since, a repeating task's next, stays. */
    @Synchronized
    fun completed(context: Context, task: String, at: Long) {
        keep(context, JSONArray(list(context).filter { it.task != task || it.at != at }.map(::json)).toString())
    }

    private fun json(r: Reminder) = JSONObject().put("task", r.task).put("number", r.number).put("at", r.at).put("title", r.title)
        .put("body", r.body).put("waiting", r.waiting).put("start", r.start?.toString())

    /** Signed out, or the data is gone: no reminders, none put off, none shown. */
    @Synchronized
    fun clear(context: Context) {
        prefs(context).edit().remove(LIST).remove(SNOOZED).commit()
        schedule(context)
        withdraw(context) { true }
    }

    /** Takes back the reminders shown of `task`, as when it is done from one. */
    fun withdraw(context: Context, task: String) = withdraw(context) { it == task }

    /** Takes back the reminders shown of the tasks `which` picks. */
    private fun withdraw(context: Context, which: (task: String) -> Boolean) {
        val manager = context.getSystemService(NotificationManager::class.java)
        for (shown in manager.activeNotifications) {
            // Only reminders carry a task. The summary the system groups several under has none:
            // cancelling it would take them all, and it goes with the last of them anyway.
            val task = shown.notification.extras.getString(TASK) ?: continue
            if (which(task)) manager.cancel(shown.id)
        }
    }

    /** Whether the alarm can be exact; without the permission it may come a few minutes late. */
    fun exact(context: Context) = Build.VERSION.SDK_INT < Build.VERSION_CODES.S || context.getSystemService(AlarmManager::class.java).canScheduleExactAlarms()

    /** Up to when reminders have been shown: from now, should the clock have gone back, or nothing
     * have been shown yet. */
    private fun mark(context: Context, now: Long) = minOf(prefs(context).getLong(MARK, now), now)

    /** When each reminder is to show: at its time, or when it was put off to. */
    private fun times(context: Context): List<Pair<Reminder, Long>> {
        val snoozed = snoozed(context)
        return list(context).map { r -> r to if (snoozed.has(r.task)) snoozed.getJSONObject(r.task).getLong("until") else r.at }
    }

    /** Sets the alarm for the next reminder not shown yet, which may be due already: it goes off at
     * once and `show` shows it, the card too. */
    private fun schedule(context: Context) {
        val mark = mark(context, System.currentTimeMillis())
        val at = times(context).map { it.second }.filter { it > mark }.minOrNull()
        val alarms = context.getSystemService(AlarmManager::class.java)
        val intent = PendingIntent.getBroadcast(context, 0, Intent(context, ReminderReceiver::class.java).setAction(ReminderReceiver.ALARM), PendingIntent.FLAG_IMMUTABLE)
        when {
            at == null -> alarms.cancel(intent)
            exact(context) -> alarms.setExactAndAllowWhileIdle(AlarmManager.RTC_WAKEUP, at, intent)
            else -> alarms.setAndAllowWhileIdle(AlarmManager.RTC_WAKEUP, at, intent)
        }
    }

    fun cardOn(context: Context) = prefs(context).getBoolean(CARD, false)

    fun setCard(context: Context, on: Boolean) {
        prefs(context).edit().putBoolean(CARD, on).commit()
    }

    /** Whether to show the reminder card now: on, allowed, and the user is at the phone, unlocked and
     * not to be disturbed. Otherwise the banner does. */
    private fun cardNow(context: Context) =
        cardOn(context) && Settings.canDrawOverlays(context) &&
            context.getSystemService(PowerManager::class.java).isInteractive &&
            !context.getSystemService(KeyguardManager::class.java).isKeyguardLocked &&
            context.getSystemService(NotificationManager::class.java).currentInterruptionFilter == NotificationManager.INTERRUPTION_FILTER_ALL

    /** Whether to ask for notifications now: the first time there are reminders. */
    fun ask(context: Context): Boolean {
        if (prefs(context).getBoolean(ASKED, false)) return false
        prefs(context).edit().putBoolean(ASKED, true).commit()
        return true
    }

    /** Shows `r`; `quiet` while the card shows it, without a banner. Its id is the task's number. */
    private fun post(context: Context, r: Reminder, quiet: Boolean) {
        val uri = Uri.parse("nexttodo://task/${r.task}")
        val open = PendingIntent.getActivity(context, 0, Intent(Intent.ACTION_VIEW, uri, context, MainActivity::class.java), PendingIntent.FLAG_IMMUTABLE)
        val builder = Notification.Builder(context, if (quiet) Channels.DUE_QUIET else Channels.DUE)
            .setSmallIcon(R.drawable.ic_stat_nexttodo)
            .setColor(Palette.light.primary.toArgb())
            .setContentTitle(r.title)
            .setContentText(r.body)
            .setCategory(Notification.CATEGORY_REMINDER)
            .setWhen(r.at)
            .setShowWhen(true)
            .setContentIntent(open)
            .setAutoCancel(true)
            // Shown again as the task changes: the sound only the first time.
            .setOnlyAlertOnce(true)
        val ready = r.ready(LocalDate.now())
        builder.addExtras(Bundle().apply { putString(TASK, r.task); putLong(AT, r.at); putBoolean(READY, ready) })
        if (ready) {
            val done = Intent(context, ReminderReceiver::class.java).setAction(ReminderReceiver.DONE).setData(uri)
                .putExtra(ReminderReceiver.TITLE, r.title)
            builder.addAction(Notification.Action.Builder(null, "完成", PendingIntent.getBroadcast(context, 0, done, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)).build())
        }
        val snooze = Intent(context, SnoozeActivity::class.java).setData(uri)
        builder.addAction(Notification.Action.Builder(null, "稍后提醒", PendingIntent.getActivity(context, 0, snooze, PendingIntent.FLAG_IMMUTABLE)).build())
        context.getSystemService(NotificationManager::class.java).notify(r.number, builder.build())
    }
}
