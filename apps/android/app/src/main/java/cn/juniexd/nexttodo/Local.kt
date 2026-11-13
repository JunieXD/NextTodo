package cn.juniexd.nexttodo

import android.content.Context
import cn.juniexd.nexttodo.reminders.Reminders
import cn.juniexd.nexttodo.widget.Widget
import com.getcapacitor.JSObject
import java.io.File
import java.io.IOException
import kotlinx.coroutines.runBlocking
import org.json.JSONObject

/**
 * Local mode (docs/local.md): the server runs in the app's process, started in `NextTodoApp.onCreate`
 * before anything that needs it, the widget and the reminders too. They and the page reach it over
 * HTTP, with the address and the session where a server's are, Capacitor's `server` and `token`.
 */
object Local {
    /** Capacitor's `mode`: `local` while the app is in local mode. Writing or removing it is the switch. */
    private const val MODE = "mode"
    private const val LOCAL = "local"

    /** The epoch of the database the widget's list and the reminders are of (docs/local.md#数据代号). */
    private const val EPOCH = "localEpoch"

    fun on(context: Context) = Server.prefs(context).getString(MODE, null) == LOCAL

    private fun dir(context: Context) = File(context.filesDir, "local").path

    /** The server did not start; `locked` while an earlier process of the app still has its data. */
    class StartFailed(message: String, val locked: Boolean) : Exception(message)

    /** What the page needs, once the server has started and the app signed in to it. */
    private var opened: JSObject? = null

    /** Starts the server and signs the app in (docs/local.md#android): its address, the session's
     * token, the local user and the epoch. Once the server has started, the same each time; until
     * then, it tries again. */
    @Synchronized
    fun open(context: Context): JSObject {
        opened?.let { return it }
        val prefs = Server.prefs(context)
        // The last run's, at a port no longer this app's: should the server not start, nothing goes there.
        prefs.edit().remove("server").remove("token").commit()
        val started = start(context)
        val epoch = started.getString("epoch")
        if (prefs.getString(EPOCH, null) != epoch) {
            // The widget's list, the reminders and the changes not sent are of data that is not there any more.
            runBlocking {
                Widget.forget(context)
                Changes.forget(context)
            }
            Reminders.clear(context)
            prefs.edit().putString(EPOCH, epoch).commit()
        }
        val token = LocalServer.session()
        if (!prefs.edit().putString("server", started.getString("url")).putString("token", token).commit()) throw IOException("打开本机模式失败：无法保存会话")
        return JSObject(started.toString()).put("token", token).also { opened = it }
    }

    /** Starts the server, the first time: its address, the local user and the epoch. */
    fun start(context: Context): JSONObject {
        val reply = JSONObject(LocalServer.start(dir(context)))
        if (reply.has("error")) throw StartFailed(reply.getString("error"), reply.getBoolean("locked"))
        return reply
    }

    /** Enters local mode, from the next start; or leaves it, and the session with it, in one go. */
    fun set(context: Context, on: Boolean) {
        val edit = Server.prefs(context).edit()
        if (on) edit.putString(MODE, LOCAL) else edit.remove(MODE).remove("server").remove("token").remove(EPOCH)
        if (!edit.commit()) throw IOException("切换本机模式失败，请稍后重试")
    }

    fun backup(to: File) = LocalServer.backup(to.path)

    fun prepare(context: Context, from: File) = LocalServer.prepare(dir(context), from.path)

    fun commit(context: Context) = LocalServer.commit(dir(context))
}

/** The server's library (`crates/android`), loaded the first time it is used: in local mode only.
 * Its methods throw with a message for the user. */
private object LocalServer {
    init {
        System.loadLibrary("nexttodo_android")
    }

    /** `{url, user, epoch}`, or `{error, locked}`. */
    @JvmStatic external fun start(dir: String): String

    @JvmStatic external fun session(): String

    @JvmStatic external fun backup(to: String)

    @JvmStatic external fun prepare(dir: String, from: String)

    @JvmStatic external fun commit(dir: String)
}
