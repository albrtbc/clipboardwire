// SPDX-License-Identifier: GPL-3.0-or-later
package com.davefx.clipboardwire.send

import org.junit.Assert.*
import org.junit.Test

class SendDecisionTest {

    private val text = ClipPayload.Text("hello")

    private fun decide(
        payload: ClipPayload? = text,
        trigger: Trigger = Trigger.MANUAL,
        fromOwnProvider: Boolean = false,
        sensitive: Boolean = false,
        serviceRunning: Boolean = true,
        connected: Boolean = true,
        lastSent: String? = null,
        lastReceived: String? = null
    ) = SendDecision.decide(
        payload, trigger, fromOwnProvider, sensitive,
        serviceRunning, connected, lastSent, lastReceived
    )

    private fun oversizeImage() =
        ClipPayload.Image("A".repeat(SendDecision.MAX_FRAME_CHARS + 1))

    @Test
    fun `fingerprint is the text or the base64`() {
        assertEquals("hello", text.fingerprint)
        assertEquals("QUJD", ClipPayload.Image("QUJD").fingerprint)
    }

    @Test
    fun `max frame chars is 15 MiB`() {
        assertEquals(15 * 1024 * 1024, SendDecision.MAX_FRAME_CHARS)
    }

    @Test
    fun `plain text is sent`() {
        assertEquals(SendOutcome.SENT, decide())
        assertEquals(SendOutcome.SENT, decide(trigger = Trigger.AUTO))
    }

    @Test
    fun `null payload is nothing to send`() {
        assertEquals(SendOutcome.NOTHING_TO_SEND, decide(payload = null))
    }

    @Test
    fun `blank text is nothing to send`() {
        assertEquals(SendOutcome.NOTHING_TO_SEND, decide(payload = ClipPayload.Text("")))
        assertEquals(SendOutcome.NOTHING_TO_SEND, decide(payload = ClipPayload.Text(" \n\t ")))
    }

    @Test
    fun `nothing to send wins over every other rule`() {
        val outcome = decide(
            payload = null,
            trigger = Trigger.AUTO,
            fromOwnProvider = true,
            sensitive = true,
            serviceRunning = false,
            connected = false
        )
        assertEquals(SendOutcome.NOTHING_TO_SEND, outcome)
    }

    @Test
    fun `own provider is already on desktop even for manual`() {
        val image = ClipPayload.Image("QUJD")
        assertEquals(
            SendOutcome.ALREADY_ON_DESKTOP,
            decide(payload = image, trigger = Trigger.MANUAL, fromOwnProvider = true)
        )
        assertEquals(
            SendOutcome.ALREADY_ON_DESKTOP,
            decide(payload = image, trigger = Trigger.AUTO, fromOwnProvider = true)
        )
    }

    @Test
    fun `own provider wins over sensitive and connection state`() {
        val outcome = decide(
            trigger = Trigger.AUTO,
            fromOwnProvider = true,
            sensitive = true,
            serviceRunning = false
        )
        assertEquals(SendOutcome.ALREADY_ON_DESKTOP, outcome)
    }

    @Test
    fun `auto skips sensitive clips`() {
        assertEquals(
            SendOutcome.SKIPPED_SENSITIVE,
            decide(trigger = Trigger.AUTO, sensitive = true)
        )
    }

    @Test
    fun `manual sends sensitive clips`() {
        assertEquals(SendOutcome.SENT, decide(trigger = Trigger.MANUAL, sensitive = true))
    }

    @Test
    fun `sensitive wins over dedupe and connection state for auto`() {
        val outcome = decide(
            trigger = Trigger.AUTO,
            sensitive = true,
            connected = false,
            lastSent = "hello"
        )
        assertEquals(SendOutcome.SKIPPED_SENSITIVE, outcome)
    }

    @Test
    fun `auto equal to last received is already on desktop`() {
        assertEquals(
            SendOutcome.ALREADY_ON_DESKTOP,
            decide(trigger = Trigger.AUTO, lastReceived = "hello")
        )
    }

    @Test
    fun `manual equal to last received but not last sent is sent`() {
        assertEquals(
            SendOutcome.SENT,
            decide(trigger = Trigger.MANUAL, lastSent = "other", lastReceived = "hello")
        )
    }

    @Test
    fun `equal to last sent is already on desktop for both triggers`() {
        assertEquals(
            SendOutcome.ALREADY_ON_DESKTOP,
            decide(trigger = Trigger.MANUAL, lastSent = "hello")
        )
        assertEquals(
            SendOutcome.ALREADY_ON_DESKTOP,
            decide(trigger = Trigger.AUTO, lastSent = "hello")
        )
    }

    @Test
    fun `different content from last sent and last received is sent`() {
        assertEquals(
            SendOutcome.SENT,
            decide(trigger = Trigger.AUTO, lastSent = "a", lastReceived = "b")
        )
    }

    @Test
    fun `image equal to last sent is already on desktop`() {
        assertEquals(
            SendOutcome.ALREADY_ON_DESKTOP,
            decide(payload = ClipPayload.Image("QUJD"), lastSent = "QUJD")
        )
    }

    @Test
    fun `oversize image is too large`() {
        assertEquals(SendOutcome.TOO_LARGE, decide(payload = oversizeImage()))
        assertEquals(SendOutcome.TOO_LARGE, decide(payload = oversizeImage(), trigger = Trigger.AUTO))
    }

    @Test
    fun `image at the limit is sent`() {
        val image = ClipPayload.Image("A".repeat(SendDecision.MAX_FRAME_CHARS))
        assertEquals(SendOutcome.SENT, decide(payload = image))
    }

    @Test
    fun `text is never too large`() {
        val big = ClipPayload.Text("A".repeat(SendDecision.MAX_FRAME_CHARS + 1))
        assertEquals(SendOutcome.SENT, decide(payload = big))
    }

    @Test
    fun `too large wins over connection state`() {
        assertEquals(
            SendOutcome.TOO_LARGE,
            decide(payload = oversizeImage(), serviceRunning = false, connected = false)
        )
    }

    @Test
    fun `oversize image loses to own provider, dedupe and sensitive`() {
        val big = oversizeImage()
        assertEquals(
            SendOutcome.ALREADY_ON_DESKTOP,
            decide(payload = big, fromOwnProvider = true)
        )
        assertEquals(
            SendOutcome.ALREADY_ON_DESKTOP,
            decide(payload = big, lastSent = big.base64Png)
        )
        assertEquals(
            SendOutcome.SKIPPED_SENSITIVE,
            decide(payload = big, trigger = Trigger.AUTO, sensitive = true)
        )
    }

    @Test
    fun `toString does not expose clip content`() {
        assertEquals("Text(12 chars)", ClipPayload.Text("secret-12345").toString())
        assertEquals("Image(4 base64 chars)", ClipPayload.Image("QUJD").toString())
    }

    @Test
    fun `not running is reported`() {
        assertEquals(SendOutcome.NOT_RUNNING, decide(serviceRunning = false))
    }

    @Test
    fun `not running wins over not connected`() {
        assertEquals(
            SendOutcome.NOT_RUNNING,
            decide(serviceRunning = false, connected = false)
        )
    }

    @Test
    fun `not connected is reported`() {
        assertEquals(SendOutcome.NOT_CONNECTED, decide(connected = false))
    }

    @Test
    fun `dedupe wins over connection state`() {
        assertEquals(
            SendOutcome.ALREADY_ON_DESKTOP,
            decide(serviceRunning = false, connected = false, lastSent = "hello")
        )
        assertEquals(
            SendOutcome.ALREADY_ON_DESKTOP,
            decide(
                trigger = Trigger.AUTO,
                serviceRunning = true,
                connected = false,
                lastReceived = "hello"
            )
        )
    }
}
