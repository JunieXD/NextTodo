package cn.juniexd.nexttodo

import android.content.Context
import com.getcapacitor.plugin.WebView
import java.io.File
import java.io.IOException
import java.io.InputStream
import java.net.HttpURLConnection
import java.net.URL
import java.util.concurrent.atomic.AtomicBoolean
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.json.JSONObject

/**
 * Newer builds of the web app from the server, used from the next start (docs/android.md#更新).
 * A build is downloaded into `files/web/<version>/`, reusing the files the one running has, and is
 * tried at the next start; one that does not report ready in time is given up.
 */
object WebUpdate {
    /** The build served now: a downloaded one's version, or the APK's. */
    @Volatile
    var running: String? = null
        private set

    private val busy = AtomicBoolean(false)

    private fun prefs(context: Context) = context.getSharedPreferences("WebUpdate", Context.MODE_PRIVATE)
    private fun builds(context: Context) = File(context.filesDir, "web")

    /** Before the bridge starts: picks the build to serve, and has Capacitor serve it. */
    @Synchronized
    fun start(context: Context) {
        val prefs = prefs(context)
        // Builds downloaded for another APK may need what this one lacks, and Capacitor serves the
        // APK's after an update anyway.
        val sameApk = prefs.getInt("apk", -1) == BuildConfig.VERSION_CODE
        if (!sameApk) builds(context).deleteRecursively()
        fun built(version: String?) = version?.takeIf { sameApk && File(builds(context), it).isDirectory }
        // Tried at the last start without reporting ready: back to the one before, for good.
        val failed = built(prefs.getString("trying", null))
        failed?.let { File(builds(context), it).deleteRecursively() }
        val bad = if (sameApk) prefs.getStringSet("bad", null).orEmpty() else emptySet()
        val trying = built(prefs.getString("staged", null))
        val good = built(prefs.getString("good", null))
        prefs.edit().clear()
            .putInt("apk", BuildConfig.VERSION_CODE)
            .putString("good", good)
            .putString("trying", trying)
            .putStringSet("bad", failed?.let { bad + it } ?: bad)
            .commit()
        serve(context, trying ?: good)
    }

    /** Whether the build served is being tried, and has yet to report ready. */
    fun trying(context: Context) = prefs(context).contains("trying")

    /** The build tried has not reported ready in time: given up for good, back to the one it was to
     * replace, which `load` gets: its directory, or null for the APK's. A build downloaded meanwhile
     * waits for the next start. Nothing happens should the build have reported ready since. */
    @Synchronized
    fun giveUp(context: Context, load: (File?) -> Unit) {
        val prefs = prefs(context)
        val failed = prefs.getString("trying", null) ?: return
        File(builds(context), failed).deleteRecursively()
        prefs.edit().remove("trying").putStringSet("bad", prefs.getStringSet("bad", null).orEmpty() + failed).commit()
        load(serve(context, prefs.getString("good", null)))
    }

    /** Has Capacitor serve build `version`, or the APK's, from its next start; returns the directory. */
    private fun serve(context: Context, version: String?): File? {
        val dir = version?.let { File(builds(context), it) }
        context.getSharedPreferences(WebView.WEBVIEW_PREFS_NAME, Context.MODE_PRIVATE).edit()
            .putString(WebView.CAP_SERVER_PATH, dir?.path ?: "")
            .commit()
        running = version ?: bundled(context)
        return dir
    }

    /** The web app started: the build being tried works, and the ones before it can go. */
    @Synchronized
    fun ready(context: Context) {
        val prefs = prefs(context)
        val trying = prefs.getString("trying", null) ?: return
        prefs.edit().putString("good", trying).remove("trying").commit()
        val keep = setOf(trying, prefs.getString("staged", null))
        builds(context).listFiles()?.filter { it.name !in keep }?.forEach { it.deleteRecursively() }
    }

    /** Downloads `server`'s build when it is another one that this APK can run, and returns its
     * version; nothing when there is none to download. Fails while another download is under way,
     * one the page before a recreated activity started say, so that this check is not taken as done. */
    suspend fun update(context: Context, server: String): String? = withContext(Dispatchers.IO) {
        if (!busy.compareAndSet(false, true)) throw IOException("正在下载另一个版本")
        try {
            val info = JSONObject(String(download("$server/app/web.json")))
            // A commit, which names a directory here.
            val version = info.getString("version").takeIf { it.matches(Regex("[0-9a-f]{7,40}")) } ?: throw IOException("版本号无效")
            val prefs = prefs(context)
            val known = setOf(running, prefs.getString("staged", null)) + prefs.getStringSet("bad", null).orEmpty()
            if (version in known || info.getInt("native") > BuildConfig.NATIVE) return@withContext null
            val part = File(context.filesDir, "web-part").apply { deleteRecursively() }
            val files = info.getJSONArray("files")
            for (i in 0 until files.length()) {
                val path = files.getString(i)
                if (path.split('/').any { it.isEmpty() || it == "." || it == ".." }) throw IOException("文件路径无效：$path")
                val to = File(part, path).apply { parentFile!!.mkdirs() }
                // Vite names these after their content: the same name is the same file.
                val have = if (path.startsWith("assets/")) current(context, path) else null
                if (have != null) {
                    have.use { input -> to.outputStream().use { input.copyTo(it) } }
                } else {
                    to.writeBytes(download("$server/$path"))
                }
            }
            // Released again meanwhile, the server may have given files of the next build.
            if (JSONObject(String(download("$server/app/web.json"))).getString("version") != version) throw IOException("服务器上的版本已更新，下次再下载")
            File(part, "app").mkdirs()
            File(part, "app/web.json").writeText(info.toString())
            stage(context, part, version)
            version
        } finally {
            busy.set(false)
        }
    }

    /** Puts a downloaded build in place to be tried at the next start, instead of one staged before. */
    @Synchronized
    private fun stage(context: Context, part: File, version: String) {
        val prefs = prefs(context)
        val into = File(builds(context), version)
        into.deleteRecursively()
        into.parentFile!!.mkdirs()
        if (!part.renameTo(into)) throw IOException("无法保存新版本")
        prefs.getString("staged", null)?.let { File(builds(context), it).deleteRecursively() }
        prefs.edit().putString("staged", version).commit()
    }

    /** A file of the build running, if it has one at `path`. */
    private fun current(context: Context, path: String): InputStream? {
        val dir = running?.let { File(builds(context), it) }?.takeIf { it.isDirectory }
        if (dir != null) return File(dir, path).takeIf { it.isFile }?.inputStream()
        return runCatching { context.assets.open("public/$path") }.getOrNull()
    }

    /** The version of the build in the APK. */
    private fun bundled(context: Context): String? =
        runCatching { JSONObject(context.assets.open("public/app/web.json").bufferedReader().use { it.readText() }).getString("version") }.getOrNull()
}

/** The file at `url`; throws when the server does not give it. */
fun download(url: String): ByteArray {
    val c = URL(url).openConnection() as HttpURLConnection
    try {
        c.connectTimeout = 10_000
        c.readTimeout = 30_000
        if (c.responseCode != 200) throw IOException("下载失败（${c.responseCode}）：$url")
        return c.inputStream.use { it.readBytes() }
    } finally {
        c.disconnect()
    }
}
