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

    @Before
    fun setUp() {
        server = MockWebServer()
    }

    @After
    fun tearDown() {
        server.shutdown()
    }

    @Test
    fun `connects and receives welcome frame`() {
        val welcomeJson = """
            {"type":"welcome","server":"clipboardwire/0.3.0",
             "client_id":"test-id","last_clip":null}
        """.trimIndent()

        server.enqueue(MockResponse().withWebSocketUpgrade(object : okhttp3.WebSocketListener() {
            override fun onOpen(webSocket: okhttp3.WebSocket, response: okhttp3.Response) {
                webSocket.send(welcomeJson)
            }
        }))
        server.start()

        val latch = CountDownLatch(1)
        var receivedWelcome: Protocol.Frame.Welcome? = null

        val handler = WebSocketHandler(
            serverUrl = "ws://${server.hostName}:${server.port}/sync",
            user = "alice",
            password = "hunter2",
            tlsInsecure = false,
            listener = object : WebSocketHandler.Listener {
                override fun onConnected(welcome: Protocol.Frame.Welcome) {
                    receivedWelcome = welcome
                    latch.countDown()
                }
                override fun onClipReceived(clip: Protocol.Frame.Clip) {}
                override fun onDisconnected(reason: String) {}
                override fun onError(error: String) {}
            }
        )
        handler.connect()

        assertTrue("should receive welcome within 5s", latch.await(5, TimeUnit.SECONDS))
        assertEquals("test-id", receivedWelcome?.clientId)
        handler.close()
    }

    @Test
    fun `sends basic auth header on upgrade`() {
        server.enqueue(MockResponse().withWebSocketUpgrade(object : okhttp3.WebSocketListener() {}))
        server.start()

        val latch = CountDownLatch(1)
        val handler = WebSocketHandler(
            serverUrl = "ws://${server.hostName}:${server.port}/sync",
            user = "alice",
            password = "hunter2",
            tlsInsecure = false,
            listener = object : WebSocketHandler.Listener {
                override fun onConnected(welcome: Protocol.Frame.Welcome) {}
                override fun onClipReceived(clip: Protocol.Frame.Clip) {}
                override fun onDisconnected(reason: String) { latch.countDown() }
                override fun onError(error: String) { latch.countDown() }
            }
        )
        handler.connect()
        Thread.sleep(500)

        val request = server.takeRequest(2, TimeUnit.SECONDS)
        assertNotNull("upgrade request should arrive", request)
        val authHeader = request!!.getHeader("Authorization")
        assertNotNull("Authorization header must be present", authHeader)
        assertTrue("must be Basic auth", authHeader!!.startsWith("Basic "))
        handler.close()
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
                val welcome = """
                    {"type":"welcome","server":"clipboardwire/0.3.0",
                     "client_id":"me","last_clip":null}
                """.trimIndent()
                webSocket.send(welcome)
                webSocket.send(clipJson)
            }
        }))
        server.start()

        val latch = CountDownLatch(1)
        var receivedClip: Protocol.Frame.Clip? = null

        val handler = WebSocketHandler(
            serverUrl = "ws://${server.hostName}:${server.port}/sync",
            user = "alice",
            password = "hunter2",
            tlsInsecure = false,
            listener = object : WebSocketHandler.Listener {
                override fun onConnected(welcome: Protocol.Frame.Welcome) {}
                override fun onClipReceived(clip: Protocol.Frame.Clip) {
                    receivedClip = clip
                    latch.countDown()
                }
                override fun onDisconnected(reason: String) {}
                override fun onError(error: String) {}
            }
        )
        handler.connect()

        assertTrue("should receive clip within 5s", latch.await(5, TimeUnit.SECONDS))
        assertEquals("hello from peer", receivedClip?.content)
        assertEquals("other", receivedClip?.from)
        handler.close()
    }

    @Test
    fun `sendText delivers message to server`() {
        val latch = CountDownLatch(1)
        var receivedMessage: String? = null

        server.enqueue(MockResponse().withWebSocketUpgrade(object : okhttp3.WebSocketListener() {
            override fun onOpen(webSocket: okhttp3.WebSocket, response: okhttp3.Response) {
                val welcome = """
                    {"type":"welcome","server":"clipboardwire/0.3.0",
                     "client_id":"me","last_clip":null}
                """.trimIndent()
                webSocket.send(welcome)
            }
            override fun onMessage(webSocket: okhttp3.WebSocket, text: String) {
                receivedMessage = text
                latch.countDown()
            }
        }))
        server.start()

        val connectedLatch = CountDownLatch(1)
        val handler = WebSocketHandler(
            serverUrl = "ws://${server.hostName}:${server.port}/sync",
            user = "alice",
            password = "hunter2",
            tlsInsecure = false,
            listener = object : WebSocketHandler.Listener {
                override fun onConnected(welcome: Protocol.Frame.Welcome) {
                    connectedLatch.countDown()
                }
                override fun onClipReceived(clip: Protocol.Frame.Clip) {}
                override fun onDisconnected(reason: String) {}
                override fun onError(error: String) {}
            }
        )
        handler.connect()
        assertTrue(connectedLatch.await(5, TimeUnit.SECONDS))

        val clipJson = Protocol.buildClipText("outbound text")
        handler.sendText(clipJson)

        assertTrue("server should receive the message", latch.await(5, TimeUnit.SECONDS))
        assertNotNull(receivedMessage)
        assertTrue(receivedMessage!!.contains("outbound text"))
        handler.close()
    }

    @Test
    fun `calls onDisconnected when server closes`() {
        server.enqueue(MockResponse().withWebSocketUpgrade(object : okhttp3.WebSocketListener() {
            override fun onOpen(webSocket: okhttp3.WebSocket, response: okhttp3.Response) {
                val welcome = """
                    {"type":"welcome","server":"clipboardwire/0.3.0",
                     "client_id":"me","last_clip":null}
                """.trimIndent()
                webSocket.send(welcome)
                webSocket.close(1000, "bye")
            }
        }))
        server.start()

        val latch = CountDownLatch(1)
        var disconnectReason: String? = null

        val handler = WebSocketHandler(
            serverUrl = "ws://${server.hostName}:${server.port}/sync",
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
        handler.connect()

        assertTrue("should get disconnect callback", latch.await(5, TimeUnit.SECONDS))
        assertNotNull(disconnectReason)
        handler.close()
    }
}
