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
import java.time.LocalTime
import org.json.JSONArray
import org.json.JSONObject

/** A task's reminder, as the app hands them over (`apps/web/src/mobile.ts`): at `at`, in
 * milliseconds; `due`, when the time it is of is due, the same for a task's reminders of one time;
 * `waiting`, how many of its prerequisites are not done; `start` and `startTime`, when it starts, at
 * the start of the day without a time. */
data class Reminder(
    val task: String,
    val at: Long,
    val due: Long,
    val title: String,
    val body: String,
    val waiting: Int,
    val start: LocalDate?,
    val startTime: LocalTime?,
) {
    /** Whether it can be done straight from the reminder `now`: it waits for nothing. */
    fun ready(now: LocalDateTime) = waiting == 0 && toStart(now) == null

    /** What the reminder card says under the title: what the notification says, and what it waits for. */
    fun meta(now: LocalDateTime): String {
        val waits = if (waiting > 0) "等 $waiting 项" else toStart(now)
        return listOfNotNull(body, waits).joinToString(" · ")
    }

    /** Its start, if still to come `now`, as the app writes it: `20:00 开始`, `明天开始`, `周五 09:30 开始`. */
    private fun toStart(now: LocalDateTime): String? {
        val date = start ?: return null
        if (!date.atTime(startTime ?: LocalTime.MIDNIGHT).isAfter(now)) return null
        val day = Due(date, startTime)
        return when {
            startTime == null -> "${day.label(now)}开始"
            day.today(now) -> "%02d:%02d 开始".format(startTime.hour, startTime.minute)
            else -> "${day.label(now)} 开始"
        }
    }

    companion object {
        fun from(o: JSONObject) = Reminder(
            o.getString("task"),
            o.getLong("at"),
            // Kept by a version before several reminders a task, without it: each a time of its own.
            o.optLong("due", o.getLong("at")),
            o.getString("title"),
            o.getString("body"),
            o.getInt("waiting"),
            if (o.isNull("start")) null else LocalDate.parse(o.getString("start")),
            o.optString("start_time").takeIf { it.isNotEmpty() }?.let(LocalTime::parse),
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

    /** Reminders put off, by task: `{at, until}`, the time of the reminder and when to show it again. One
     * a task: the next of its reminders to come takes the place of the one put off. */
    private const val SNOOZED = "snoozed"

    /** Whether the reminder card is on (docs/android.md#弹窗提醒). */
    private const val CARD = "card"

    /** Reminders that came due for the card while the phone was locked or its screen off, by task:
     * their time. The card shows them once it is unlocked. */
    private const val LOCKED = "locked"

    /** The reminder each task showed last, by task: its time. A later one takes the place of the one
     * before, in the notification and on the card. */
    private const val SHOWN = "shown"

    /** Whether the app has asked for notifications, which it does once. */
    private const val ASKED = "asked"

    /** Notification extras: which task and reminder time a notification is of, and when that time of
     * the task is due. */
    private const val TASK = "nexttodo.task"
    private const val AT = "nexttodo.at"
    private const val DUE = "nexttodo.due"

    /** And whether it has “完成”. */
    private const val READY = "nexttodo.ready"

    fun list(context: Context): List<Reminder> {
        val list = JSONArray(prefs(context).getString(LIST, "[]"))
        return (0 until list.length()).map { Reminder.from(list.getJSONObject(it)) }
    }

    /** `task`'s reminder shown last, that at `at` or one shown since in its place; none once done,
     * deleted, moved or put off. */
    fun shown(context: Context, task: String, at: Long): Reminder? {
        val last = maxOf(at, shown(context).optLong(task, at))
        return current(context).find { it.task == task && it.at == last }
    }

    private fun shown(context: Context) = JSONObject(prefs(context).getString(SHOWN, "{}"))

    /** The time of the reminder `task`'s notification shows, while it does. */
    fun notified(context: Context, task: String) = context.getSystemService(NotificationManager::class.java).activeNotifications
        .find { it.notification.extras.getString(TASK) == task }?.notification?.extras?.getLong(AT)

    /** When the time `task`'s reminders are of is due, to know them by once it is done. */
    fun due(context: Context, task: String) = list(context).find { it.task == task }?.due

    private fun snoozed(context: Context) = JSONObject(prefs(context).getString(SNOOZED, "{}"))

    /** Whether `r` is the reminder put off, of those in `snoozed`. */
    private fun JSONObject.puts(r: Reminder) = optJSONObject(r.task)?.getLong("at") == r.at

    /** The reminders still to show: none done, deleted or put off since. */
    fun current(context: Context): List<Reminder> {
        val snoozed = snoozed(context)
        return list(context).filter { !snoozed.puts(it) }
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
     * to wait for another or its start come, shows as it is, without a sound. The alarm moves to
     * the next. */
    private fun keep(context: Context, list: String) {
        val now = LocalDateTime.now()
        val wanted = JSONArray(list).let { a -> (0 until a.length()).map { Reminder.from(a.getJSONObject(it)) } }.associateBy { it.task to it.at }
        val snoozed = snoozed(context)
        for ((task, s) in snoozed.entries()) if ((task to s.getLong("at")) !in wanted) snoozed.remove(task)
        val shown = shown(context)
        for (task in shown.keys().asSequence().toList()) if ((task to shown.getLong(task)) !in wanted) shown.remove(task)
        prefs(context).edit().putString(LIST, list).putString(SNOOZED, snoozed.toString()).putString(SHOWN, shown.toString()).commit()
        val manager = context.getSystemService(NotificationManager::class.java)
        for (shown in manager.activeNotifications) {
            // Reminders only, as in `withdraw`.
            val extras = shown.notification.extras
            val task = extras.getString(TASK) ?: continue
            val r = wanted[task to extras.getLong(AT)]
            // Of a time done since, should the next have a reminder at the same moment. Shown by an
            // earlier version, without it: of the time it is now.
            if (r == null || extras.getLong(DUE, r.due) != r.due) {
                manager.cancel(shown.tag, shown.id)
            } else if (shown.tag != task) {
                // Shown by an earlier version, by the task's number: again by its id, without a sound.
                manager.cancel(shown.tag, shown.id)
                post(context, r, quiet = true, again = true)
            } else if (
                extras.getCharSequence(Notification.EXTRA_TITLE).toString() != r.title ||
                extras.getCharSequence(Notification.EXTRA_TEXT).toString() != r.body ||
                extras.getBoolean(READY) != r.ready(now)
            ) {
                post(context, r, quiet = shown.notification.channelId == Channels.DUE_QUIET, again = true)
            }
        }
        schedule(context)
    }

    /** Shows the reminders that came due since the last time, those put off too, of those `which` picks,
     * and sets the alarm for the next. Returns those to show on the reminder card as well, when it is to
     * be: with those that came due while the phone was locked, should it have been unlocked unheard. */
    @Synchronized
    fun show(context: Context, which: (Reminder, at: Long) -> Boolean = { _, _ -> true }): List<Reminder> {
        val now = System.currentTimeMillis()
        val mark = mark(context, now)
        val snoozed = snoozed(context)
        val due = times(context).filter { (r, at) -> at in (mark + 1)..now && which(r, at) }.map { it.first }
        val shown = shown(context)
        for (r in due) {
            snoozed.remove(r.task)
            shown.put(r.task, maxOf(r.at, shown.optLong(r.task, r.at)))
        }
        prefs(context).edit().putLong(MARK, now).putString(SNOOZED, snoozed.toString()).putString(SHOWN, shown.toString()).commit()
        schedule(context)
        val card = card(context)
        val present = present(context)
        for (r in due) post(context, r, quiet = card && present, again = false)
        if (!card) return emptyList()
        if (present) return unlocked(context) + due
        val locked = locked(context)
        for (r in due) locked.put(r.task, r.at)
        prefs(context).edit().putString(LOCKED, locked.toString()).commit()
        return emptyList()
    }

    /** The phone has been unlocked: the reminders that came due while it was locked, for the card. Only
     * those still in a notification and not done, deleted, moved or put off since. */
    @Synchronized
    fun unlocked(context: Context): List<Reminder> {
        val locked = locked(context)
        if (locked.length() == 0) return emptyList()
        prefs(context).edit().remove(LOCKED).commit()
        if (!card(context)) return emptyList()
        val shown = context.getSystemService(NotificationManager::class.java).activeNotifications
            .mapNotNull { n -> n.notification.extras.getString(TASK)?.let { it to n.notification.extras.getLong(AT) } }.toSet()
        return current(context).filter { locked.optLong(it.task) == it.at && (it.task to it.at) in shown }
    }

    private fun locked(context: Context) = JSONObject(prefs(context).getString(LOCKED, "{}"))

    /** Puts reminder `r` off for `minutes`: its notification goes, and comes again then. */
    @Synchronized
    fun snooze(context: Context, r: Reminder, minutes: Int) {
        val snoozed = snoozed(context).put(r.task, JSONObject().put("at", r.at).put("until", System.currentTimeMillis() + minutes * 60_000L))
        prefs(context).edit().putString(SNOOZED, snoozed.toString()).commit()
        withdraw(context, r.task)
        schedule(context)
    }

    /** `task` is done, its time due at `due`: those reminders go, shown or not, put off or not. Those the
     * app has handed over since, of a repeating task's next time, stay. */
    @Synchronized
    fun completed(context: Context, task: String, due: Long) {
        keep(context, JSONArray(list(context).filter { it.task != task || it.due != due }.map(::json)).toString())
    }

    private fun json(r: Reminder) = JSONObject().put("task", r.task).put("at", r.at).put("due", r.due).put("title", r.title)
        .put("body", r.body).put("waiting", r.waiting).put("start", r.start?.toString()).put("start_time", r.startTime?.toString())

    /** Signed out, or the data is gone: no reminders, none put off, none shown. */
    @Synchronized
    fun clear(context: Context) {
        prefs(context).edit().remove(LIST).remove(SNOOZED).remove(LOCKED).remove(SHOWN).commit()
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
            if (which(task)) manager.cancel(shown.tag, shown.id)
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
        return list(context).map { r -> r to if (snoozed.puts(r)) snoozed.getJSONObject(r.task).getLong("until") else r.at }
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

    /** Whether reminders go on the card: on, allowed, and not to disturb. Otherwise the banner does. */
    private fun card(context: Context) =
        cardOn(context) && Settings.canDrawOverlays(context) &&
            context.getSystemService(NotificationManager::class.java).currentInterruptionFilter == NotificationManager.INTERRUPTION_FILTER_ALL

    /** Whether the user is at the phone: its screen on and unlocked. */
    private fun present(context: Context) =
        context.getSystemService(PowerManager::class.java).isInteractive && !context.getSystemService(KeyguardManager::class.java).isKeyguardLocked

    /** Whether to ask for notifications now: the first time there are reminders. */
    fun ask(context: Context): Boolean {
        if (prefs(context).getBoolean(ASKED, false)) return false
        prefs(context).edit().putBoolean(ASKED, true).commit()
        return true
    }

    /** Shows `r`; `quiet` while the card shows it, without a banner; `again`, shown already and only
     * brought up to date, without a sound. Its tag is the task's id: numbers are each owner's, and those
     * in shared folders may be the same as the user's own. So a task's next reminder takes the place of
     * the one shown before, and sounds. */
    private fun post(context: Context, r: Reminder, quiet: Boolean, again: Boolean) {
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
            .setOnlyAlertOnce(again)
        val ready = r.ready(LocalDateTime.now())
        builder.addExtras(Bundle().apply { putString(TASK, r.task); putLong(AT, r.at); putLong(DUE, r.due); putBoolean(READY, ready) })
        if (ready) {
            val done = Intent(context, ReminderReceiver::class.java).setAction(ReminderReceiver.DONE).setData(uri)
                .putExtra(ReminderReceiver.TITLE, r.title)
            builder.addAction(Notification.Action.Builder(null, "完成", PendingIntent.getBroadcast(context, 0, done, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)).build())
        }
        val snooze = Intent(context, SnoozeActivity::class.java).setData(uri)
        builder.addAction(Notification.Action.Builder(null, "稍后提醒", PendingIntent.getActivity(context, 0, snooze, PendingIntent.FLAG_IMMUTABLE)).build())
        context.getSystemService(NotificationManager::class.java).notify(r.task, 0, builder.build())
    }
}
