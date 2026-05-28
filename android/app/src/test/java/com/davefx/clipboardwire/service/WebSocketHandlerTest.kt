// SPDX-License-Identifier: GPL-3.0-or-later
package com.davefx.clipboardwire.service

import okhttp3.mockwebserver.MockResponse
import okhttp3.mockwebserver.MockWebServer
import org.junit.After
import org.junit.Assert.*
import org.junit.Before
import org.junit.Test
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

class WebSocketHandlerTest {

    private lateinit var server: MockWebServer
    private var handler: WebSocketHandler? = null

    @Before
    fun setUp() {
        server = MockWebServer()
    }

    @After
    fun tearDown() {
        handler?.close()
        handler = null
        try { server.shutdown() } catch (_: Exception) {}
    }

    private fun createHandler(listener: WebSocketHandler.Listener): WebSocketHandler {
        val h = WebSocketHandler(
            serverUrl = "ws://${server.hostName}:${server.port}/sync",
            user = "alice",
            password = "hunter2",
            tlsInsecure = false,
            listener = listener
        )
        handler = h
        return h
    }

    private val welcomeJson = """
        {"type":"welcome","server":"clipboardwire/0.3.0",
         "client_id":"test-id","last_clip":null}
    """.trimIndent()

    @Test
    fun `connects and receives welcome frame`() {
        server.enqueue(MockResponse().withWebSocketUpgrade(object : okhttp3.WebSocketListener() {
            override fun onOpen(webSocket: okhttp3.WebSocket, response: okhttp3.Response) {
                webSocket.send(welcomeJson)
            }
        }))
        server.start()

        val latch = CountDownLatch(1)
        var receivedWelcome: Protocol.Frame.Welcome? = null

        val h = createHandler(object : WebSocketHandler.Listener {
            override fun onConnected(welcome: Protocol.Frame.Welcome) {
                receivedWelcome = welcome
                latch.countDown()
            }
            override fun onClipReceived(clip: Protocol.Frame.Clip) {}
            override fun onDisconnected(reason: String) {}
            override fun onError(error: String) {}
        })
        h.connect()

        assertTrue("should receive welcome within 5s", latch.await(5, TimeUnit.SECONDS))
        assertEquals("test-id", receivedWelcome?.clientId)
    }

    @Test
    fun `sends basic auth header on upgrade`() {
        server.enqueue(MockResponse().withWebSocketUpgrade(object : okhttp3.WebSocketListener() {
            override fun onOpen(webSocket: okhttp3.WebSocket, response: okhttp3.Response) {
                webSocket.send(welcomeJson)
            }
        }))
        server.start()

        val latch = CountDownLatch(1)
        val h = createHandler(object : WebSocketHandler.Listener {
            override fun onConnected(welcome: Protocol.Frame.Welcome) { latch.countDown() }
            override fun onClipReceived(clip: Protocol.Frame.Clip) {}
            override fun onDisconnected(reason: String) {}
            override fun onError(error: String) {}
        })
        h.connect()
        assertTrue(latch.await(5, TimeUnit.SECONDS))

        val request = server.takeRequest(2, TimeUnit.SECONDS)
        assertNotNull("upgrade request should arrive", request)
        val authHeader = request!!.getHeader("Authorization")
        assertNotNull("Authorization header must be present", authHeader)
        assertTrue("must be Basic auth", authHeader!!.startsWith("Basic "))
    }

    @Test
    fun `receives clip frame and calls listener`() {
        val clipJson = """
            {"type":"clip","id":"c1","ts":1000,
             "content_type":"text/plain; charset=utf-8",
             "content":"hello from peer","from":"other"}
        """.trimIndent()

        server.enqueue(MockResponse().withWebSocketUpgrade(object : okhttp3.WebSocketListener() {
            override fun onOpen(webSocket: okhttp3.WebSocket, response: okhttp3.Response) {
                webSocket.send(welcomeJson)
                webSocket.send(clipJson)
            }
        }))
        server.start()

        val latch = CountDownLatch(1)
        var receivedClip: Protocol.Frame.Clip? = null

        val h = createHandler(object : WebSocketHandler.Listener {
            override fun onConnected(welcome: Protocol.Frame.Welcome) {}
            override fun onClipReceived(clip: Protocol.Frame.Clip) {
                receivedClip = clip
                latch.countDown()
            }
            override fun onDisconnected(reason: String) {}
            override fun onError(error: String) {}
        })
        h.connect()

        assertTrue("should receive clip within 5s", latch.await(5, TimeUnit.SECONDS))
        assertEquals("hello from peer", receivedClip?.content)
        assertEquals("other", receivedClip?.from)
    }

    @Test
    fun `sendText delivers message to server`() {
        val messageLatch = CountDownLatch(1)
        var receivedMessage: String? = null

        server.enqueue(MockResponse().withWebSocketUpgrade(object : okhttp3.WebSocketListener() {
            override fun onOpen(webSocket: okhttp3.WebSocket, response: okhttp3.Response) {
                webSocket.send(welcomeJson)
            }
            override fun onMessage(webSocket: okhttp3.WebSocket, text: String) {
                receivedMessage = text
                messageLatch.countDown()
            }
        }))
        server.start()

        val connectedLatch = CountDownLatch(1)
        val h = createHandler(object : WebSocketHandler.Listener {
            override fun onConnected(welcome: Protocol.Frame.Welcome) {
                connectedLatch.countDown()
            }
            override fun onClipReceived(clip: Protocol.Frame.Clip) {}
            override fun onDisconnected(reason: String) {}
            override fun onError(error: String) {}
        })
        h.connect()
        assertTrue(connectedLatch.await(5, TimeUnit.SECONDS))

        h.sendText(Protocol.buildClipText("outbound text"))

        assertTrue("server should receive the message", messageLatch.await(5, TimeUnit.SECONDS))
        assertNotNull(receivedMessage)
        assertTrue(receivedMessage!!.contains("outbound text"))
    }

    @Test
    fun `calls onDisconnected when connection fails`() {
        // Point at a port with nothing listening.
        server.start()
        val port = server.port
        server.shutdown()

        val latch = CountDownLatch(1)
        var disconnectReason: String? = null

        val h = WebSocketHandler(
            serverUrl = "ws://127.0.0.1:$port/sync",
            user = "alice",
            password = "hunter2",
            tlsInsecure = false,
            listener = object : WebSocketHandler.Listener {
                override fun onConnected(welcome: Protocol.Frame.Welcome) {}
                override fun onClipReceived(clip: Protocol.Frame.Clip) {}
                override fun onDisconnected(reason: String) {
                    disconnectReason = reason
                    latch.countDown()
                }
                override fun onError(error: String) {}
            }
        )
        handler = h
        h.connect()

        assertTrue("should get disconnect callback", latch.await(5, TimeUnit.SECONDS))
        assertNotNull(disconnectReason)
    }
}
