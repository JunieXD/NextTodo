package cn.juniexd.nexttodo

import android.Manifest
import android.content.Intent
import android.net.Uri
import android.provider.OpenableColumns
import android.provider.Settings
import androidx.activity.result.ActivityResult
import androidx.core.content.FileProvider
import cn.juniexd.nexttodo.widget.Widget
import com.getcapacitor.JSObject
import com.getcapacitor.Plugin
import com.getcapacitor.PluginCall
import com.getcapacitor.PluginMethod
import com.getcapacitor.annotation.ActivityCallback
import com.getcapacitor.annotation.CapacitorPlugin
import com.getcapacitor.annotation.Permission
import java.io.File
import java.util.concurrent.atomic.AtomicBoolean
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch

/** One APK download at a time, whichever activity asked for it: each one has a plugin of its own. */
private val installing = AtomicBoolean(false)

/** Whether the APKs earlier runs of the app downloaded have gone. This run's stay: an installer may
 * not have read them yet. */
private var cleared = false

/** What the web app asks of Android beyond the stock plugins: see `apps/web/src/android.ts`. Its
 * permission, `calendar`, is checked and asked for with the plugin's own `checkPermissions` and
 * `requestPermissions`. */
@CapacitorPlugin(
    name = "NextTodo",
    permissions = [Permission(alias = "calendar", strings = [Manifest.permission.READ_CALENDAR, Manifest.permission.WRITE_CALENDAR])],
)
class NextTodoPlugin : Plugin() {
    /** Does `work`, which is to `what`, off the bridge's thread: resolves `call` with what it gives, or
     * rejects it with what to tell the user, and the kind of failure as its code (`forUser`, `kind`). */
    private fun work(call: PluginCall, what: String, work: suspend () -> JSObject) {
        CoroutineScope(Dispatchers.IO).launch {
            try {
                call.resolve(work())
            } catch (e: Exception) {
                call.reject(forUser(what, e), kind(e), e)
            }
        }
    }

    @PluginMethod
    fun updateWidget(call: PluginCall) {
        val tasks = call.getString("tasks")!!
        // Not `getLong`: that gives nothing for a number small enough to arrive as an Int.
        val seq = call.data.takeIf { it.has("seq") }?.getLong("seq")
        work(call, "更新小组件") {
            Widget.show(context, tasks, seq)
            JSObject()
        }
    }

    /** The widget's text size: `small`, `standard` or `large`. */
    @PluginMethod
    fun widgetTextSize(call: PluginCall) {
        call.resolve(JSObject().put("size", Widget.textSize(context)))
    }

    @PluginMethod
    fun setWidgetTextSize(call: PluginCall) {
        val size = call.getString("size")!!
        work(call, "设置小组件的文字大小") {
            Widget.setTextSize(context, size)
            JSObject()
        }
    }

    /** Where the tasks with a deadline are written in the system calendar: `target`, `own` or the
     * id of one of the device's calendars, or nothing. See `DeviceCalendar`. */
    @PluginMethod
    fun calendar(call: PluginCall) = work(call, "读取日历设置") {
        JSObject().put(
            "target",
            when (val t = DeviceCalendar.target(context)) {
                null -> null
                DeviceCalendar.Target.Own -> "own"
                is DeviceCalendar.Target.Device -> t.id
            },
        )
    }

    @PluginMethod
    fun calendars(call: PluginCall) = work(call, "读取手机上的日历") { JSObject().put("calendars", DeviceCalendar.calendars(context)) }

    @PluginMethod
    fun setCalendar(call: PluginCall) {
        val target = when (val t = call.data.opt("target")) {
            null -> null
            "own" -> DeviceCalendar.Target.Own
            else -> DeviceCalendar.Target.Device((t as Number).toLong())
        }
        work(call, "设置写进的日历") {
            DeviceCalendar.setTarget(context, target)
            JSObject()
        }
    }

    @PluginMethod
    fun updateCalendar(call: PluginCall) {
        val events = call.getString("events")!!
        work(call, "更新系统日历") {
            DeviceCalendar.update(context, events)
            JSObject()
        }
    }

    @PluginMethod
    fun clearCalendar(call: PluginCall) = work(call, "删除写进日历的日程") {
        DeviceCalendar.clear(context)
        JSObject()
    }

    @PluginMethod
    fun openAppSettings(call: PluginCall) {
        val intent = Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS, Uri.fromParts("package", context.packageName, null))
        context.startActivity(intent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
        call.resolve()
    }

    /** This APK's version, and its native version, which builds of the web app may need. */
    @PluginMethod
    fun appInfo(call: PluginCall) {
        call.resolve(JSObject().put("version", BuildConfig.VERSION_NAME).put("native", BuildConfig.NATIVE))
    }

    /** The web app has started: the build being tried works. */
    @PluginMethod
    fun webReady(call: PluginCall) {
        WebUpdate.ready(context)
        call.resolve()
    }

    /** Downloads the build of the web app on `server`, the one signed in to, for the next start: see
     * `WebUpdate`. */
    @PluginMethod
    fun updateWeb(call: PluginCall) {
        val server = call.getString("server")!!
        work(call, "下载新版本") { JSObject().put("staged", WebUpdate.update(context, server)) }
    }

    /** Downloads the APK at `url` and hands it to the system installer. Without the permission to
     * install apps, opens the page that allows it instead and resolves `started: false`. One download
     * at a time: asked again meanwhile, it resolves at once. */
    @PluginMethod
    fun installApp(call: PluginCall) {
        if (!context.packageManager.canRequestPackageInstalls()) {
            val intent = Intent(Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES, Uri.parse("package:${context.packageName}"))
            context.startActivity(intent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
            call.resolve(JSObject().put("started", false))
            return
        }
        val url = call.getString("url")!!
        if (!installing.compareAndSet(false, true)) return call.resolve(JSObject().put("started", true))
        work(call, "下载新版本") {
            try {
                val dir = File(context.cacheDir, "update")
                if (!cleared) {
                    dir.deleteRecursively()
                    cleared = true
                }
                dir.mkdirs()
                // A file of its own, which nothing writes while the installer reads it.
                val apk = File.createTempFile("NextTodo-", ".apk", dir)
                apk.writeBytes(download(url))
                val uri = FileProvider.getUriForFile(context, "${context.packageName}.fileprovider", apk)
                val intent = Intent(Intent.ACTION_VIEW).setDataAndType(uri, "application/vnd.android.package-archive")
                context.startActivity(intent.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_ACTIVITY_NEW_TASK))
                JSObject().put("started", true)
            } finally {
                installing.set(false)
            }
        }
    }

    /** Local mode (`Local`): the server's address, the session's token, the local user and the epoch,
     * as the app's start left them. Should the server not have started, it tries again, and rejects
     * with code `locked` while an earlier process of the app still has the data. */
    @PluginMethod
    fun openLocal(call: PluginCall) {
        CoroutineScope(Dispatchers.IO).launch {
            try {
                call.resolve(Local.open(context))
            } catch (e: Local.StartFailed) {
                call.reject(e.message, if (e.locked) "locked" else null)
            } catch (e: Exception) {
                call.reject(forUser("打开本机模式", e), kind(e), e)
            }
        }
    }

    /** Starts local mode's server here, to enter it: the local user, whose data it has. */
    @PluginMethod
    fun startLocal(call: PluginCall) = work(call, "启动本机模式") { JSObject().put("user", Local.start(context).getString("user")) }

    /** Enters local mode, or leaves it, from the next start: see `restart`. */
    @PluginMethod
    fun setLocalMode(call: PluginCall) = work(call, "切换本机模式") {
        Local.set(context, call.getBoolean("on")!!)
        JSObject()
    }

    /** Ends the app's process and opens the app again. */
    @PluginMethod
    fun restart(call: PluginCall) {
        Restart.start(activity)
        call.resolve()
    }

    /** Asks where to save a backup, offering `name`, and writes it there: `saved`, the file's name, or
     * nothing when the user cancelled. Through a file in the cache, which the server writes. */
    @PluginMethod
    fun backupLocal(call: PluginCall) {
        val intent = Intent(Intent.ACTION_CREATE_DOCUMENT).addCategory(Intent.CATEGORY_OPENABLE)
            .setType("application/octet-stream")
            .putExtra(Intent.EXTRA_TITLE, call.getString("name"))
        startActivityForResult(call, intent, "backupTo")
    }

    /** Without `call`, lost should the app have been ended meanwhile, no one waits for the backup. */
    @ActivityCallback
    private fun backupTo(call: PluginCall?, result: ActivityResult) {
        if (call == null) return
        val uri = result.data?.data ?: return call.resolve()
        work(call, "备份") {
            val file = File.createTempFile("backup-", ".db", context.cacheDir)
            try {
                Local.backup(file)
                context.contentResolver.openOutputStream(uri, "wt")!!.use { out -> file.inputStream().use { it.copyTo(out) } }
            } finally {
                file.delete()
            }
            JSObject().put("saved", name(uri))
        }
    }

    /** Asks for a backup, and prepares it to replace the data: `picked`, whether the user picked one.
     * Through a file in the cache, which the server reads. Works without the server, which may have
     * failed to start. */
    @PluginMethod
    fun prepareRestore(call: PluginCall) {
        startActivityForResult(call, Intent(Intent.ACTION_OPEN_DOCUMENT).addCategory(Intent.CATEGORY_OPENABLE).setType("*/*"), "restoreFrom")
    }

    @ActivityCallback
    private fun restoreFrom(call: PluginCall?, result: ActivityResult) {
        if (call == null) return
        val uri = result.data?.data ?: return call.resolve(JSObject().put("picked", false))
        work(call, "读取备份") {
            val file = File.createTempFile("restore-", ".db", context.cacheDir)
            try {
                context.contentResolver.openInputStream(uri)!!.use { input -> file.outputStream().use { input.copyTo(it) } }
                Local.prepare(context, file)
            } finally {
                file.delete()
            }
            JSObject().put("picked", true)
        }
    }

    /** The user's go-ahead: the backup prepared replaces the data when the app starts next. */
    @PluginMethod
    fun commitRestore(call: PluginCall) = work(call, "恢复") {
        Local.commit(context)
        JSObject()
    }

    /** What the user sees a document called. */
    private fun name(uri: Uri): String =
        context.contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)!!.use {
            it.moveToFirst()
            it.getString(0)
        }
}
