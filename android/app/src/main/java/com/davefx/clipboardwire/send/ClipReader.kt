// SPDX-License-Identifier: GPL-3.0-or-later
package com.davefx.clipboardwire.send

import android.content.ClipboardManager
import android.content.ContentResolver
import android.content.Context
import android.content.Intent
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.net.Uri
import android.util.Base64
import android.util.Log
import androidx.core.content.IntentCompat
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import java.io.ByteArrayOutputStream
import kotlin.math.roundToInt

/**
 * What was read from the clipboard or a share intent. [payload] is null when there is nothing
 * to send yet: either the clip is empty or unsupported, or it is an image still to be encoded
 * from [imageUri].
 */
data class ClipRead(
    val payload: ClipPayload?,
    val fromOwnProvider: Boolean,
    val sensitive: Boolean,
    val imageUri: Uri? = null,
    val timestamp: Long = 0
)

object ClipReader {
    private const val TAG = "clipboardwire.send"
    private const val EXTRA_IS_SENSITIVE = "android.content.extra.IS_SENSITIVE"
    private const val MAX_SHRINK_RETRIES = 3

    private val EMPTY = ClipRead(null, fromOwnProvider = false, sensitive = false)

    /** Must run on the main thread while the app has focus. Returns null when the read fails. */
    fun fromClipboard(context: Context): ClipRead? {
        return try {
            val manager = context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
            val clip = manager.primaryClip ?: return EMPTY
            if (clip.itemCount == 0) return EMPTY
            val item = clip.getItemAt(0)
            val description = clip.description ?: return EMPTY
            val mime = description.getMimeType(0) ?: return EMPTY
            val sensitive = description.extras?.getBoolean(EXTRA_IS_SENSITIVE) == true

            val read = when {
                mime.startsWith("text/") -> {
                    val text = (item.text ?: item.coerceToText(context))?.toString()
                    if (text == null) EMPTY
                    else ClipRead(
                        ClipPayload.Text(text), fromOwnProvider = false, sensitive = sensitive
                    )
                }
                mime.startsWith("image/") -> {
                    val uri = item.uri ?: return EMPTY
                    imageRead(context, uri, sensitive)
                }
                else -> EMPTY
            }
            read.copy(timestamp = description.timestamp)
        } catch (e: RuntimeException) {
            Log.w(TAG, "clipboard read failed: ${e.javaClass.simpleName}")
            null
        }
    }

    fun fromShareIntent(context: Context, intent: Intent): ClipRead? {
        return try {
            if (intent.action != Intent.ACTION_SEND) return EMPTY
            val type = intent.type ?: return EMPTY
            when {
                type.startsWith("text/") -> {
                    val text = intent.getCharSequenceExtra(Intent.EXTRA_TEXT)?.toString()
                    if (text == null) EMPTY
                    else ClipRead(
                        ClipPayload.Text(text), fromOwnProvider = false, sensitive = false
                    )
                }
                type.startsWith("image/") -> {
                    val uri = IntentCompat.getParcelableExtra(
                        intent, Intent.EXTRA_STREAM, Uri::class.java
                    ) ?: return EMPTY
                    imageRead(context, uri, sensitive = false)
                }
                else -> EMPTY
            }
        } catch (e: RuntimeException) {
            Log.w(TAG, "share read failed: ${e.javaClass.simpleName}")
            null
        }
    }

    private fun imageRead(context: Context, uri: Uri, sensitive: Boolean): ClipRead {
        // A file:// URI would let another app make us read our own private files
        if (uri.scheme != ContentResolver.SCHEME_CONTENT) return EMPTY
        // An image we received from the desktop is never sent back, so skip decoding it
        return if (uri.authority == "${context.packageName}.fileprovider") {
            ClipRead(ClipPayload.Image(""), fromOwnProvider = true, sensitive = sensitive)
        } else {
            ClipRead(null, fromOwnProvider = false, sensitive = sensitive, imageUri = uri)
        }
    }

    /** Encodes a pending image. Returns null when the image can't be read or decoded. */
    suspend fun resolveImage(context: Context, read: ClipRead?): ClipRead? {
        if (read == null) return null
        val uri = read.imageUri ?: return read
        val base64 = encodeImage(context, uri) ?: return null
        return read.copy(payload = ClipPayload.Image(base64), imageUri = null)
    }

    @Suppress("UNUSED_VALUE")
    suspend fun encodeImage(context: Context, uri: Uri): String? = withContext(Dispatchers.IO) {
        var bitmap: Bitmap? = null
        try {
            val bytes = context.contentResolver.openInputStream(uri)?.use { it.readBytes() }
                ?: return@withContext null
            val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
            BitmapFactory.decodeByteArray(bytes, 0, bytes.size, bounds)
            if (bounds.outWidth <= 0 || bounds.outHeight <= 0) return@withContext null
            var sample = ImageScaling.sampleSizeFor(bounds.outWidth, bounds.outHeight)

            var retries = 0
            while (bitmap == null) {
                try {
                    val options = BitmapFactory.Options().apply { inSampleSize = sample }
                    bitmap = BitmapFactory.decodeByteArray(bytes, 0, bytes.size, options)
                        ?: return@withContext null
                } catch (e: OutOfMemoryError) {
                    if (retries++ >= MAX_SHRINK_RETRIES) throw e
                    sample *= 2
                }
            }

            retries = 0
            var encoded: String?
            while (true) {
                val current = bitmap!!
                encoded = try {
                    encodePng(current)
                } catch (e: OutOfMemoryError) {
                    null
                }
                if (encoded != null && encoded.length <= SendDecision.MAX_FRAME_CHARS) break
                if (retries++ >= MAX_SHRINK_RETRIES) break
                // After an OOM the size is unknown, so just halve the bitmap
                val factor = if (encoded == null) 0.5f
                else ImageScaling.shrinkFactor(encoded.length, SendDecision.MAX_FRAME_CHARS)
                // Drop the oversized string before the next encode, so both aren't held at once
                encoded = null
                val width = (current.width * factor).roundToInt().coerceAtLeast(1)
                val height = (current.height * factor).roundToInt().coerceAtLeast(1)
                val scaled = Bitmap.createScaledBitmap(current, width, height, true)
                if (scaled !== current) current.recycle()
                bitmap = scaled
            }
            encoded
        } catch (e: OutOfMemoryError) {
            Log.w(TAG, "image read failed: ${e.javaClass.simpleName}")
            null
        } catch (e: Exception) {
            Log.w(TAG, "image read failed: ${e.javaClass.simpleName}")
            null
        } finally {
            bitmap?.recycle()
        }
    }

    private fun encodePng(bitmap: Bitmap): String {
        val out = ByteArrayOutputStream()
        bitmap.compress(Bitmap.CompressFormat.PNG, 100, out)
        return Base64.encodeToString(out.toByteArray(), Base64.NO_WRAP)
    }
}
