// SPDX-License-Identifier: GPL-3.0-or-later
package com.davefx.clipboardwire.send

import org.junit.Assert.*
import org.junit.Test
import kotlin.math.sqrt

class ImageScalingTest {

    @Test
    fun `image within the limit keeps sample size 1`() {
        assertEquals(1, ImageScaling.sampleSizeFor(4000, 3000))
        assertEquals(1, ImageScaling.sampleSizeFor(4096, 4096))
        assertEquals(1, ImageScaling.sampleSizeFor(100, 100))
    }

    @Test
    fun `larger image gets a power of two sample size`() {
        assertEquals(2, ImageScaling.sampleSizeFor(8192, 6144))
        assertEquals(2, ImageScaling.sampleSizeFor(4097, 10))
        assertEquals(4, ImageScaling.sampleSizeFor(16384, 100))
    }

    @Test
    fun `sample size follows the longest side`() {
        assertEquals(8, ImageScaling.sampleSizeFor(20000, 100))
        assertEquals(8, ImageScaling.sampleSizeFor(100, 20000))
    }

    @Test
    fun `custom max side is honoured`() {
        assertEquals(4, ImageScaling.sampleSizeFor(4000, 3000, maxSide = 1000))
    }

    @Test(expected = IllegalArgumentException::class)
    fun `non positive max side is rejected`() {
        ImageScaling.sampleSizeFor(100, 100, maxSide = 0)
    }

    @Test
    fun `degenerate dimensions give sample size 1`() {
        assertEquals(1, ImageScaling.sampleSizeFor(0, 0))
    }

    @Test
    fun `shrink factor is below 1 for an oversize image`() {
        val factor = ImageScaling.shrinkFactor(20_000_000, 15_000_000)
        assertTrue(factor > 0f)
        assertTrue(factor < 1f)
    }

    @Test
    fun `shrink factor brings the area estimate under the limit`() {
        val encoded = 40_000_000
        val limit = 15 * 1024 * 1024
        val factor = ImageScaling.shrinkFactor(encoded, limit)
        // encoded size grows roughly with the pixel area, which is factor squared
        assertTrue(encoded * factor.toDouble() * factor < limit)
        assertEquals(sqrt(limit.toDouble() / encoded) * 0.9, factor.toDouble(), 1e-4)
    }

    @Test
    fun `shrink factor is clamped to 1 when already under the limit`() {
        assertEquals(1f, ImageScaling.shrinkFactor(1000, 15_000_000), 0f)
    }

    @Test
    fun `shrink factor stays positive for extreme ratios`() {
        val factor = ImageScaling.shrinkFactor(Int.MAX_VALUE, 1)
        assertTrue(factor > 0f)
        assertTrue(factor < 1f)
    }
}
