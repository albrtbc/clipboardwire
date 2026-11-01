// SPDX-License-Identifier: GPL-3.0-or-later
package com.davefx.clipboardwire.send

import kotlin.math.sqrt

object ImageScaling {
    private const val SAFETY = 0.9f
    private const val MIN_FACTOR = 0.01f

    /** Smallest power of two that brings both sides to [maxSide] or less. */
    fun sampleSizeFor(width: Int, height: Int, maxSide: Int = 4096): Int {
        require(maxSide > 0) { "maxSide must be positive" }
        val longest = maxOf(width, height).toLong()
        var sample = 1
        while (longest > maxSide.toLong() * sample) sample *= 2
        return sample
    }

    /** Scale factor in (0, 1] that should bring an encoded size of [encodedChars] under [limitChars]. */
    fun shrinkFactor(encodedChars: Int, limitChars: Int): Float {
        if (encodedChars <= 0) return 1f
        if (limitChars <= 0) return MIN_FACTOR
        val factor = sqrt(limitChars.toDouble() / encodedChars).toFloat() * SAFETY
        return factor.coerceIn(MIN_FACTOR, 1f)
    }
}
