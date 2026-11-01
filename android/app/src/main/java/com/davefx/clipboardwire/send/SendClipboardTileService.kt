// SPDX-License-Identifier: GPL-3.0-or-later
package com.davefx.clipboardwire.send

import android.annotation.SuppressLint
import android.content.Intent
import android.os.Build
import android.service.quicksettings.Tile
import android.service.quicksettings.TileService

class SendClipboardTileService : TileService() {

    override fun onStartListening() {
        super.onStartListening()
        qsTile?.apply {
            state = Tile.STATE_INACTIVE
            updateTile()
        }
    }

    override fun onClick() {
        super.onClick()
        if (isLocked) unlockAndRun { openSendActivity() } else openSendActivity()
    }

    // The Intent overload is only used below API 34, where it still works
    @SuppressLint("StartActivityAndCollapseDeprecated")
    private fun openSendActivity() {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
            @Suppress("DEPRECATION")
            startActivityAndCollapse(
                Intent(this, SendClipboardActivity::class.java)
                    .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            )
        } else {
            startActivityAndCollapse(SendClipboardActivity.pendingIntent(this))
        }
    }
}
