// SPDX-License-Identifier: GPL-3.0-or-later
package com.davefx.clipboardwire.send

import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.lifecycle.lifecycleScope
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

/**
 * Transparent activity that takes focus, reads the clipboard and hands it to the sync service.
 * Since Android 10 only the focused app can read the clipboard.
 */
class SendClipboardActivity : ComponentActivity() {
    private var handled = false

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        lifecycleScope.launch {
            delay(FOCUS_TIMEOUT_MS)
            if (!handled) {
                handled = true
                SendBridge.feedback(
                    this@SendClipboardActivity, SendOutcome.READ_FAILED, Trigger.MANUAL, false
                )
                SendBridge.finishQuietly(this@SendClipboardActivity)
            }
        }
    }

    override fun onWindowFocusChanged(hasFocus: Boolean) {
        super.onWindowFocusChanged(hasFocus)
        if (!hasFocus || handled) return
        handled = true
        val read = ClipReader.fromClipboard(this)
        lifecycleScope.launch {
            val resolved = ClipReader.resolveImage(this@SendClipboardActivity, read)
            SendBridge.sendAndReport(this@SendClipboardActivity, resolved, Trigger.MANUAL)
            SendBridge.finishQuietly(this@SendClipboardActivity)
        }
    }

    companion object {
        private const val FOCUS_TIMEOUT_MS = 1500L

        fun pendingIntent(context: Context): PendingIntent =
            PendingIntent.getActivity(
                context, 2,
                Intent(context, SendClipboardActivity::class.java)
                    .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK),
                PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
            )
    }
}
