package cn.juniexd.nexttodo

import android.content.Intent
import android.net.Uri
import android.provider.Settings
import androidx.core.content.FileProvider
import cn.juniexd.nexttodo.widget.Widget
import com.getcapacitor.JSObject
import com.getcapacitor.Plugin
import com.getcapacitor.PluginCall
import com.getcapacitor.PluginMethod
import com.getcapacitor.annotation.CapacitorPlugin
import java.io.File
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch

/** What the web app asks of Android beyond the stock plugins: see `apps/web/src/android.ts`. */
@CapacitorPlugin(name = "NextTodo")
class NextTodoPlugin : Plugin() {
    @PluginMethod
    fun updateWidget(call: PluginCall) {
        val tasks = call.getString("tasks")!!
        CoroutineScope(Dispatchers.Default).launch {
            try {
                Widget.show(context, tasks)
                call.resolve()
            } catch (e: Exception) {
                call.reject(e.message, e)
            }
        }
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

    /** Downloads the server's build of the web app, for the next start: see `WebUpdate`. */
    @PluginMethod
    fun updateWeb(call: PluginCall) {
        CoroutineScope(Dispatchers.Default).launch {
            try {
                call.resolve(JSObject().put("staged", WebUpdate.update(context)))
            } catch (e: Exception) {
                call.reject(e.message, e)
            }
        }
    }

    /** Downloads the APK at `url` and hands it to the system installer. Without the permission to
     * install apps, opens the page that allows it instead and resolves `started: false`. */
    @PluginMethod
    fun installApp(call: PluginCall) {
        if (!context.packageManager.canRequestPackageInstalls()) {
            val intent = Intent(Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES, Uri.parse("package:${context.packageName}"))
            context.startActivity(intent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
            call.resolve(JSObject().put("started", false))
            return
        }
        val url = call.getString("url")!!
        CoroutineScope(Dispatchers.IO).launch {
            try {
                val apk = File(context.cacheDir, "update/NextTodo.apk").apply { parentFile!!.mkdirs() }
                apk.writeBytes(download(url))
                val uri = FileProvider.getUriForFile(context, "${context.packageName}.fileprovider", apk)
                val intent = Intent(Intent.ACTION_VIEW).setDataAndType(uri, "application/vnd.android.package-archive")
                context.startActivity(intent.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_ACTIVITY_NEW_TASK))
                call.resolve(JSObject().put("started", true))
            } catch (e: Exception) {
                call.reject(e.message, e)
            }
        }
    }
}
