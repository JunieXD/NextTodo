package cn.juniexd.nexttodo

import android.os.Bundle
import android.os.Handler
import android.os.Looper
import com.getcapacitor.Bridge
import com.getcapacitor.BridgeActivity

class MainActivity : BridgeActivity() {
    private val handler = Handler(Looper.getMainLooper())

    /** The build tried has not reported ready: back to the one before, now. Reopening the app may
     * only bring this activity back, so the next start may be far off. */
    private val giveUp = Runnable {
        WebUpdate.giveUp(this) { dir ->
            if (dir != null) bridge.setServerBasePath(dir.path) else bridge.setServerAssetPath(Bridge.DEFAULT_WEB_ASSET_DIR)
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        registerPlugin(NextTodoPlugin::class.java)
        WebUpdate.start(this)
        super.onCreate(savedInstanceState)
        // Without a WebView, Capacitor shows its error page and makes no bridge: nothing to give up.
        if (bridge != null && WebUpdate.trying(this)) handler.postDelayed(giveUp, READY_MS)
    }

    override fun onDestroy() {
        handler.removeCallbacks(giveUp)
        super.onDestroy()
    }

    private companion object {
        /** How long a build tried has to show its first page. */
        const val READY_MS = 30_000L
    }
}
