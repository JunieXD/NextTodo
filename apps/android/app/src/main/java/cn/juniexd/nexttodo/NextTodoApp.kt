package cn.juniexd.nexttodo

import android.app.Application
import android.appwidget.AppWidgetManager
import android.content.ComponentName
import android.os.Build
import android.util.Log
import cn.juniexd.nexttodo.widget.NextWidgetReceiver
import cn.juniexd.nexttodo.widget.Widget
import java.io.File

/** Runs first whenever the app's process starts, for its page, the widget, a reminder or a refresh. */
class NextTodoApp : Application() {
    override fun onCreate() {
        super.onCreate()
        // `Restart`'s process, which ends this one, runs nothing of the app.
        if (processName() != packageName) return
        if (Local.on(this)) {
            try {
                Local.open(this)
            } catch (e: Exception) {
                // The page tries again, and says why it failed; the widget asks to open the app.
                Log.w(TAG, "local mode did not start", e)
            }
        }
        // To the mode now: in local mode its refreshes need no network (docs/local.md#android).
        if (AppWidgetManager.getInstance(this).getAppWidgetIds(ComponentName(this, NextWidgetReceiver::class.java)).isNotEmpty()) Widget.schedule(this)
    }

    private fun processName(): String =
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) Application.getProcessName() else File("/proc/self/cmdline").readText().substringBefore('\u0000')

    companion object {
        const val TAG = "NextTodo"
    }
}
