// SPDX-License-Identifier: GPL-3.0-or-later
package com.davefx.clipboardwire.service

import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test

class ProtocolTest {

    @Test
    fun `parse welcome frame without last_clip`() {
        val json = """
            {"type":"welcome","server":"clipboardwire/0.3.0",
             "client_id":"aaa-bbb","last_clip":null}
        """.trimIndent()
        val frame = Protocol.parseFrame(json)
        assertTrue(frame is Protocol.Frame.Welcome)
        val w = frame as Protocol.Frame.Welcome
        assertEquals("clipboardwire/0.3.0", w.server)
        assertEquals("aaa-bbb", w.clientId)
        assertNull(w.lastClip)
    }

    @Test
    fun `parse welcome frame with cached text clip`() {
        val json = """
            {"type":"welcome","server":"clipboardwire/0.3.0",
             "client_id":"aaa-bbb",
             "last_clip":{"type":"clip","id":"ccc","ts":1000,
                          "content_type":"text/plain; charset=utf-8",
                          "content":"hello","from":"ddd"}}
        """.trimIndent()
        val frame = Protocol.parseFrame(json)
        assertTrue(frame is Protocol.Frame.Welcome)
        val w = frame as Protocol.Frame.Welcome
        assertNotNull(w.lastClip)
        assertEquals("hello", w.lastClip!!.content)
        assertEquals("ddd", w.lastClip!!.from)
    }

    @Test
    fun `parse text clip frame`() {
        val json = """
            {"type":"clip","id":"id1","ts":12345,
             "content_type":"text/plain; charset=utf-8",
             "content":"clipboard text","from":"peer1"}
        """.trimIndent()
        val frame = Protocol.parseFrame(json)
        assertTrue(frame is Protocol.Frame.Clip)
        val c = frame as Protocol.Frame.Clip
        assertEquals("id1", c.id)
        assertEquals(12345L, c.ts)
        assertEquals(Protocol.TEXT_CONTENT_TYPE, c.contentType)
        assertEquals("clipboard text", c.content)
        assertNull(c.contentB64)
        assertEquals("peer1", c.from)
    }

    @Test
    fun `parse image clip frame`() {
        val json = """
            {"type":"clip","id":"id2","ts":99999,
             "content_type":"image/png",
             "content_b64":"aWNvbg==","from":"peer2"}
        """.trimIndent()
        val frame = Protocol.parseFrame(json)
        assertTrue(frame is Protocol.Frame.Clip)
        val c = frame as Protocol.Frame.Clip
        assertEquals(Protocol.IMAGE_CONTENT_TYPE, c.contentType)
        assertNull(c.content)
        assertEquals("aWNvbg==", c.contentB64)
    }

    @Test
    fun `parse clip frame without from field`() {
        val json = """
            {"type":"clip","id":"id3","ts":0,
             "content_type":"text/plain; charset=utf-8",
             "content":"no from"}
        """.trimIndent()
        val frame = Protocol.parseFrame(json) as Protocol.Frame.Clip
        assertNull(frame.from)
    }

    @Test
    fun `parse error frame`() {
        val json = """
            {"type":"error","code":"bad_frame","message":"malformed frame"}
        """.trimIndent()
        val frame = Protocol.parseFrame(json)
        assertTrue(frame is Protocol.Frame.Error)
        val e = frame as Protocol.Frame.Error
        assertEquals("bad_frame", e.code)
        assertEquals("malformed frame", e.message)
    }

    @Test
    fun `unknown frame type returns Unknown`() {
        val json = """{"type":"future_type","data":123}"""
        val frame = Protocol.parseFrame(json)
        assertTrue(frame is Protocol.Frame.Unknown)
    }

    @Test
    fun `buildClipText produces valid frame`() {
        val json = Protocol.buildClipText("hello world")
        val obj = JSONObject(json)
        assertEquals("clip", obj.getString("type"))
        assertEquals(Protocol.TEXT_CONTENT_TYPE, obj.getString("content_type"))
        assertEquals("hello world", obj.getString("content"))
        assertFalse(obj.has("content_b64"))
        assertTrue(obj.getString("id").isNotBlank())
        assertTrue(obj.getLong("ts") > 0)
    }

    @Test
    fun `buildClipImage produces valid frame`() {
        val json = Protocol.buildClipImage("iVBORw0KGgo=")
        val obj = JSONObject(json)
        assertEquals("clip", obj.getString("type"))
        assertEquals(Protocol.IMAGE_CONTENT_TYPE, obj.getString("content_type"))
        assertEquals("iVBORw0KGgo=", obj.getString("content_b64"))
        assertFalse(obj.has("content"))
    }

    @Test
    fun `buildClipText round-trips through parseFrame`() {
        val original = "round trip text"
        val json = Protocol.buildClipText(original)
        val frame = Protocol.parseFrame(json)
        assertTrue(frame is Protocol.Frame.Clip)
        assertEquals(original, (frame as Protocol.Frame.Clip).content)
    }

    @Test
    fun `buildClipImage round-trips through parseFrame`() {
        val b64 = "dGVzdCBpbWFnZSBkYXRh"
        val json = Protocol.buildClipImage(b64)
        val frame = Protocol.parseFrame(json)
        assertTrue(frame is Protocol.Frame.Clip)
        assertEquals(b64, (frame as Protocol.Frame.Clip).contentB64)
    }

    @Test
    fun `each buildClipText call generates a unique id`() {
        val json1 = Protocol.buildClipText("a")
        val json2 = Protocol.buildClipText("a")
        val id1 = JSONObject(json1).getString("id")
        val id2 = JSONObject(json2).getString("id")
        assertNotEquals(id1, id2)
    }
}
