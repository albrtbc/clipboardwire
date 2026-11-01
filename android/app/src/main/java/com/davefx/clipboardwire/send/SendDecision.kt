// SPDX-License-Identifier: GPL-3.0-or-later
package com.davefx.clipboardwire.send

sealed class ClipPayload {
    abstract val fingerprint: String

    data class Text(val text: String) : ClipPayload() {
        override val fingerprint: String get() = text

        override fun toString() = "Text(${text.length} chars)"
    }

    data class Image(val base64Png: String) : ClipPayload() {
        override val fingerprint: String get() = base64Png

        override fun toString() = "Image(${base64Png.length} base64 chars)"
    }
}

enum class Trigger { MANUAL, AUTO }

enum class SendOutcome {
    SENT,
    NOTHING_TO_SEND,
    ALREADY_ON_DESKTOP,
    SKIPPED_SENSITIVE,
    TOO_LARGE,
    NOT_RUNNING,
    NOT_CONNECTED,
    READ_FAILED
}

object SendDecision {
    // 15 MiB of base64, leaving room for the JSON envelope under OkHttp's 16 MiB frame limit
    const val MAX_FRAME_CHARS = 15 * 1024 * 1024

    /** Returns [SendOutcome.SENT] when the payload should be sent. */
    fun decide(
        payload: ClipPayload?,
        trigger: Trigger,
        fromOwnProvider: Boolean,
        sensitive: Boolean,
        serviceRunning: Boolean,
        connected: Boolean,
        lastSent: String?,
        lastReceived: String?
    ): SendOutcome {
        if (payload == null) return SendOutcome.NOTHING_TO_SEND
        if (payload is ClipPayload.Text && payload.text.isBlank()) return SendOutcome.NOTHING_TO_SEND
        if (fromOwnProvider) return SendOutcome.ALREADY_ON_DESKTOP
        if (trigger == Trigger.AUTO && sensitive) return SendOutcome.SKIPPED_SENSITIVE

        val fingerprint = payload.fingerprint
        if (fingerprint == lastSent) return SendOutcome.ALREADY_ON_DESKTOP
        if (trigger == Trigger.AUTO && fingerprint == lastReceived) return SendOutcome.ALREADY_ON_DESKTOP

        if (payload is ClipPayload.Image && payload.base64Png.length > MAX_FRAME_CHARS) {
            return SendOutcome.TOO_LARGE
        }
        if (!serviceRunning) return SendOutcome.NOT_RUNNING
        if (!connected) return SendOutcome.NOT_CONNECTED
        return SendOutcome.SENT
    }
}
