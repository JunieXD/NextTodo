package cn.juniexd.nexttodo

import android.content.Intent
import android.net.Uri
import android.provider.Settings
import cn.juniexd.nexttodo.widget.Widget
import com.getcapacitor.Plugin
import com.getcapacitor.PluginCall
import com.getcapacitor.PluginMethod
import com.getcapacitor.annotation.CapacitorPlugin
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
}
