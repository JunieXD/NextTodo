package cn.juniexd.nexttodo

import android.content.ContentResolver
import android.content.Intent
import android.graphics.Color
import android.net.Uri
import android.os.Bundle
import android.provider.OpenableColumns
import androidx.core.content.IntentCompat
import com.getcapacitor.BridgeActivity
import com.getcapacitor.JSArray
import com.getcapacitor.JSObject
import java.io.File
import java.io.InputStream
import java.io.OutputStream

/** The quick add card over another app (docs/android.md#快速添加小窗): the web app's page `/quick-add`
 * in a window of its own, the app it was opened over showing dimmed behind it. Opened by the widget's
 * `+`, the app's shortcut, the Quick Settings tile, and what other apps share. */
class QuickAddActivity : BridgeActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        registerPlugin(NextTodoPlugin::class.java)
        super.onCreate(savedInstanceState)
        // Without a WebView, Capacitor shows its error page and makes no bridge.
        if (bridge == null) return
        bridge.webView.setBackgroundColor(Color.TRANSPARENT)
        open()
    }

    /** Opened again while still here, behind a window that did not cover it all: anew, with what is
     * shared now. */
    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        // Capacitor hands the intent it started with here too, as it starts.
        if (bridge == null || intent === getIntent()) return
        setIntent(intent)
        open()
    }

    /** The page, with `?add` for typing at once, the keyboard up. */
    private fun open() = bridge.webView.loadUrl(bridge.appUrl.trimEnd('/') + "/quick-add?add")

    /** Capacitor's theme, opaque, put in place as it starts: this one's instead. */
    override fun setTheme(resid: Int) = super.setTheme(R.style.QuickAdd)

    /** What another app shared, for the page to add, once: the text, with the subject first, a web
     * page's title say; the files, copied into the cache for the page to fetch, but for those larger
     * than `max` bytes and not of type `shrunk`, which is made smaller later; and, for one that could
     * not be read, why. Null when nothing was shared, or it was taken already. */
    @Synchronized
    fun takeShared(max: Long, shrunk: String?): JSObject? {
        val intent = intent.takeIf { it.action == Intent.ACTION_SEND || it.action == Intent.ACTION_SEND_MULTIPLE } ?: return null
        setIntent(Intent())
        val text = listOfNotNull(intent.getStringExtra(Intent.EXTRA_SUBJECT), intent.getCharSequenceExtra(Intent.EXTRA_TEXT)?.toString())
            .map { it.trim() }
            .filter { it.isNotEmpty() }
            .distinct()
            .joinToString("\n")
        val uris = if (intent.action == Intent.ACTION_SEND) {
            listOfNotNull(IntentCompat.getParcelableExtra(intent, Intent.EXTRA_STREAM, Uri::class.java))
        } else {
            IntentCompat.getParcelableArrayListExtra(intent, Intent.EXTRA_STREAM, Uri::class.java).orEmpty()
        }
        val dir = File(cacheDir, SHARED).apply { deleteRecursively() }
        val files = JSArray()
        uris.forEachIndexed { i, uri ->
            if (!lent(uri)) {
                files.put(JSObject().put("error", "不能添加分享的第 ${i + 1} 个文件"))
                return@forEachIndexed
            }
            // Another app's file, gone or not to be read: the rest, and the text, still come.
            try {
                val name = contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use {
                    if (it.moveToFirst()) it.getString(0) else null
                } ?: uri.lastPathSegment ?: "file"
                val type = contentResolver.getType(uri) ?: ""
                // A folder each: two shared files may have the same name.
                val file = File(dir, "$i/${File(name).name}").apply { parentFile!!.mkdirs() }
                val whole = contentResolver.openInputStream(uri)!!.use { input ->
                    file.outputStream().use { copy(input, it, if (type == shrunk) Long.MAX_VALUE else max) }
                }
                if (whole) {
                    files.put(JSObject().put("path", file.path).put("name", file.name).put("type", type))
                } else {
                    file.delete()
                    files.put(JSObject().put("name", file.name).put("tooLarge", true))
                }
            } catch (e: Exception) {
                files.put(JSObject().put("error", forUser("读取分享的第 ${i + 1} 个文件", e)))
            }
        }
        return JSObject().put("text", text).put("files", files)
    }

    /** Whether `uri` is what the sharing app lets this one read: not a file, nor content of this app's
     * own, which it could read anyway, whatever the sharing app may. */
    private fun lent(uri: Uri): Boolean {
        if (uri.scheme != ContentResolver.SCHEME_CONTENT) return false
        // Without the user it may start with, `0@`, which the content resolver takes off too.
        val authority = uri.authority?.substringAfterLast('@') ?: return false
        @Suppress("DEPRECATION") // From Android 13 the flags may also come as a long.
        return packageManager.resolveContentProvider(authority, 0)?.packageName != packageName
    }

    /** Copies `input` to `out`, up to `limit` bytes: false, having stopped there, when there is more. */
    private fun copy(input: InputStream, out: OutputStream, limit: Long): Boolean {
        val buffer = ByteArray(DEFAULT_BUFFER_SIZE)
        var left = limit
        while (true) {
            val n = input.read(buffer)
            if (n < 0) return true
            left -= n
            if (left < 0) return false
            out.write(buffer, 0, n)
        }
    }

    private companion object {
        /** Where shared files are copied, emptied at the next share. */
        const val SHARED = "shared"
    }
}
