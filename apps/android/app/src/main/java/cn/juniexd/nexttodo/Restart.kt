package cn.juniexd.nexttodo

import android.app.Activity
import android.content.Intent
import android.os.Bundle
import android.os.Process

/**
 * Ends the app's process and opens the app again, from a process of its own, as ProcessPhoenix does:
 * `NextTodoApp.onCreate` and the server start afresh (docs/local.md#选择和切换). An activity on screen
 * may open the app; an alarm, from the background, may not.
 */
class Restart : Activity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        Process.killProcess(intent.getIntExtra(PID, 0))
        startActivity(Intent(this, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TASK))
        finish()
        Runtime.getRuntime().exit(0)
    }

    companion object {
        private const val PID = "pid"

        fun start(from: Activity) = from.startActivity(Intent(from, Restart::class.java).putExtra(PID, Process.myPid()))
    }
}
