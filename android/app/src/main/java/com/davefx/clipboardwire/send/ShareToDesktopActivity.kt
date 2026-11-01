// SPDX-License-Identifier: GPL-3.0-or-later
package com.davefx.clipboardwire.send

import android.content.Intent
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.lifecycle.lifecycleScope
import kotlinx.coroutines.launch

/** Share target for text and images. It needs no focus and never reads the clipboard. */
class ShareToDesktopActivity : ComponentActivity() {

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        if (intent?.action != Intent.ACTION_SEND) {
            SendBridge.finishQuietly(this)
            return
        }
        val read = ClipReader.fromShareIntent(this, intent)
        // The grant on a shared stream lasts only while this activity lives
        lifecycleScope.launch {
            val resolved = ClipReader.resolveImage(this@ShareToDesktopActivity, read)
            SendBridge.sendAndReport(
                this@ShareToDesktopActivity, resolved, Trigger.MANUAL, fromShare = true
            )
            SendBridge.finishQuietly(this@ShareToDesktopActivity)
        }
    }
}
