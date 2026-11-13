package cn.juniexd.nexttodo

import android.app.AlarmManager
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import java.io.File

/** The app's notification channels, which users can tune in the system settings. */
object Channels {
    /** Reminders: a banner at the top of the screen (docs/android.md#通知). */
    const val DUE = "due"

    /** Reminders shown on the reminder card as well: a sound, no banner (docs/android.md#弹窗提醒). */
    const val DUE_QUIET = "due-quiet"

    /** Changes made natively that the server turned down later (docs/android.md#原生发出的改动). */
    const val REFUSED = "refused"

    /** Made at every start: the names follow the app, the user's settings stay. */
    fun create(context: Context) {
        val manager = context.getSystemService(NotificationManager::class.java)
        manager.createNotificationChannels(
            listOf(
                NotificationChannel(DUE, "截止提醒", NotificationManager.IMPORTANCE_HIGH),
                NotificationChannel(DUE_QUIET, "截止提醒（弹窗时）", NotificationManager.IMPORTANCE_DEFAULT),
                NotificationChannel(REFUSED, "未生效的改动", NotificationManager.IMPORTANCE_DEFAULT),
            ),
        )
        dropPlugin(context, manager)
    }

    /** What `@capacitor/local-notifications` left, which reminders went through before: its alarms,
     * the reminders it kept, the ones it showed and its channel. Once, after the upgrade. */
    private fun dropPlugin(context: Context, manager: NotificationManager) {
        if (!File(context.applicationInfo.dataDir, "shared_prefs/$PLUGIN_STORE.xml").exists()) return
        val alarms = context.getSystemService(AlarmManager::class.java)
        val publisher = Intent().setComponent(ComponentName(context, "com.capacitorjs.plugins.localnotifications.TimedNotificationPublisher"))
        for (id in context.getSharedPreferences(PLUGIN_STORE, Context.MODE_PRIVATE).all.keys) {
            PendingIntent.getBroadcast(context, id.toInt(), publisher, PendingIntent.FLAG_NO_CREATE or PendingIntent.FLAG_MUTABLE)?.let {
                alarms.cancel(it)
                it.cancel()
            }
        }
        manager.cancelAll()
        manager.deleteNotificationChannel("default")
        context.deleteSharedPreferences(PLUGIN_STORE)
    }

    private const val PLUGIN_STORE = "NOTIFICATION_STORE"
}
