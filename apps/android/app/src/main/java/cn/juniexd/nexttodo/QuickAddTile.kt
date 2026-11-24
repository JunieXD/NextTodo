package cn.juniexd.nexttodo

import android.app.PendingIntent
import android.content.Intent
import android.os.Build
import android.service.quicksettings.Tile
import android.service.quicksettings.TileService

/** The Quick Settings tile “快速添加”, which the user adds when editing the panel: opens the quick add
 * card, after unlocking (docs/android.md#快速添加小窗). */
class QuickAddTile : TileService() {
    override fun onStartListening() {
        // An action, not a switch that is on or off.
        qsTile.state = Tile.STATE_INACTIVE
        qsTile.updateTile()
    }

    override fun onClick() {
        val card = Intent(this, QuickAddActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        val open = {
            // Android 14 takes a pending intent only; the one before, an intent only.
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
                startActivityAndCollapse(PendingIntent.getActivity(this, 0, card, PendingIntent.FLAG_IMMUTABLE))
            } else {
                @Suppress("DEPRECATION")
                startActivityAndCollapse(card)
            }
        }
        if (isLocked) unlockAndRun(open) else open()
    }
}
