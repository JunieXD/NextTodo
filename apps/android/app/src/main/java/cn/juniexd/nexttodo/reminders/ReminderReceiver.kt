package cn.juniexd.nexttodo.reminders

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.widget.Toast
import cn.juniexd.nexttodo.Changes
import cn.juniexd.nexttodo.Occurrence
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.json.JSONObject

/** The reminders' alarm, the phone starting or the app updated, which may have missed some; the phone
 * unlocked, for the card to show what came due while it was locked; and “完成” on a reminder. */
class ReminderReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        when (intent.action) {
            ALARM, Intent.ACTION_BOOT_COMPLETED, Intent.ACTION_MY_PACKAGE_REPLACED -> Card.show(context, Reminders.show(context))
            Intent.ACTION_USER_PRESENT -> Card.show(context, Reminders.unlocked(context))
            DONE -> {
                val task = intent.data!!.lastPathSegment!!
                // Gone at once; should the server turn it down, a toast says why.
                Reminders.withdraw(context, task)
                val pending = goAsync()
                CoroutineScope(Dispatchers.IO).launch {
                    try {
                        val occurrence = intent.getStringExtra(OCCURRENCE)?.let { Occurrence.from(JSONObject(it)) }
                        val outcome = Changes.complete(context, task, intent.getStringExtra(TITLE)!!, occurrence)
                        if (outcome is Changes.Outcome.Refused) withContext(Dispatchers.Main) { Toast.makeText(context, outcome.message, Toast.LENGTH_LONG).show() }
                    } finally {
                        pending.finish()
                    }
                }
            }
        }
    }

    companion object {
        const val ALARM = "cn.juniexd.nexttodo.REMINDER"
        const val DONE = "cn.juniexd.nexttodo.DONE"
        const val TITLE = "title"

        /** The time the reminder is of, should the task repeat; none from an earlier version. */
        const val OCCURRENCE = "occurrence"
    }
}
