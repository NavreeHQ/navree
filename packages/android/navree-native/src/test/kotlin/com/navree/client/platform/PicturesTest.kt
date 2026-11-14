package com.navree.client.platform

import org.junit.Assert.assertEquals
import org.junit.Test

class PicturesTest {
    @Test
    fun a_picture_is_sampled_down_until_no_side_is_longer_than_the_limit() {
        assertEquals(1, sampleSize(1200, 800))
        assertEquals(1, sampleSize(2048, 2048))
        assertEquals(2, sampleSize(4000, 3000))
        assertEquals(32, sampleSize(65_000, 10))
        assertEquals(64, sampleSize(100_000, 100_000))
        assertEquals(2, sampleSize(10, 4097))
    }

    @Test
    fun a_picture_whose_size_does_not_read_is_not_decoded() {
        assertEquals(0, sampleSize(-1, -1))
        assertEquals(0, sampleSize(0, 600))
    }
}
