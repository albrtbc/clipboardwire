// SPDX-License-Identifier: GPL-3.0-or-later
package com.davefx.clipboardwire.send

import android.app.Activity
import android.content.Context
import android.os.Build
import android.util.Log
import android.widget.Toast
import com.davefx.clipboardwire.service.ClipboardSyncService
import com.davefx.clipboardwire.service.Settings
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

object SendBridge {
    private const val TAG = "clipboardwire.send"

    @Volatile var service: ClipboardSyncService? = null

    /**
     * Sends [read] and shows the feedback for [trigger]. Call on the main thread, with a [read]
     * whose image (if any) is already encoded. [fromShare] only changes the read-failure text.
     * Returns the outcome.
     */
    suspend fun sendAndReport(
        context: Context,
        read: ClipRead?,
        trigger: Trigger,
        fromShare: Boolean = false
    ): SendOutcome {
        val (outcome, configured) = attempt(context, read, trigger)
        feedback(
            context, outcome, trigger, read?.payload is ClipPayload.Image, configured, fromShare
        )
        return outcome
    }

    private suspend fun attempt(
        context: Context,
        read: ClipRead?,
        trigger: Trigger
    ): Pair<SendOutcome, Boolean?> {
        if (read == null) return log(SendOutcome.READ_FAILED, trigger) to null
        val current = service
        val payload = read.payload
        if (current == null) {
            val outcome = SendDecision.decide(
                payload, trigger, read.fromOwnProvider, read.sensitive,
                serviceRunning = false, connected = false, lastSent = null, lastReceived = null
            )
            if (outcome != SendOutcome.NOT_RUNNING || trigger != Trigger.MANUAL) {
                return log(outcome, trigger) to null
            }
            val configured = Settings.load(context).isConfigured
            if (configured) ClipboardSyncService.start(context)
            return log(outcome, trigger) to configured
        }
        // decide() maps a null payload to NOTHING_TO_SEND before the service is asked
        if (payload == null) return log(SendOutcome.NOTHING_TO_SEND, trigger) to null
        // Building and sending a JSON of up to 15 MiB must not block the main thread
        val outcome = withContext(Dispatchers.Default) {
            current.sendFromForeground(payload, trigger, read.fromOwnProvider, read.sensitive)
        }
        return log(outcome, trigger) to null
    }

    private fun log(outcome: SendOutcome, trigger: Trigger): SendOutcome {
        Log.d(TAG, "send outcome $outcome (trigger $trigger)")
        return outcome
    }

    fun feedback(
        context: Context,
        outcome: SendOutcome,
        trigger: Trigger,
        isImage: Boolean,
        configured: Boolean? = null,
        fromShare: Boolean = false
    ) {
        if (trigger == Trigger.AUTO && outcome != SendOutcome.SENT) return
        val text = when (outcome) {
            SendOutcome.SENT ->
                if (isImage) "Image sent to desktop" else "Clipboard sent to desktop"
            SendOutcome.NOTHING_TO_SEND -> "Nothing to send"
            SendOutcome.ALREADY_ON_DESKTOP -> "Already on the desktop"
            SendOutcome.SKIPPED_SENSITIVE -> "Skipped a sensitive clip"
            SendOutcome.TOO_LARGE -> "Image too large to send"
            SendOutcome.NOT_CONNECTED -> "Not connected to the hub"
            SendOutcome.NOT_RUNNING ->
                if (configured == false) "Set up ClipboardWire first"
                else "Sync was off, starting it. Try again in a moment"
            SendOutcome.READ_FAILED ->
                if (fromShare) "Couldn't read the shared item" else "Couldn't read the clipboard"
        }
        Toast.makeText(context.applicationContext, text, Toast.LENGTH_SHORT).show()
    }

    /** Finishes a transparent activity without a window animation. */
    fun finishQuietly(activity: Activity) {
        activity.finish()
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
            activity.overrideActivityTransition(Activity.OVERRIDE_TRANSITION_CLOSE, 0, 0)
        } else {
            @Suppress("DEPRECATION")
            activity.overridePendingTransition(0, 0)
        }
    }
}
