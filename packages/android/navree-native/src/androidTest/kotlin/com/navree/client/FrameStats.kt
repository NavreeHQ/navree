package com.navree.client

import android.os.Build
import android.os.Handler
import android.os.HandlerThread
import android.view.FrameMetrics
import android.view.Window

/**
 * The frames a window draws while something is done to it, from the platform's own account of each
 * (`Window.OnFrameMetricsAvailableListener` and `FrameMetrics`): how many, how many took longer than the time
 * the system gave them (`DEADLINE`, from Android 12; a 60 Hz frame before it), and the 50th, 90th and 99th
 * percentile of the time a frame took (`TOTAL_DURATION`).
 */
internal class FrameStats(private val window: Window) {
    private val totals = ArrayList<Long>()
    private var late = 0
    private var dropped = 0
    private val thread = HandlerThread("frame-stats").also { it.start() }

    private val listener = Window.OnFrameMetricsAvailableListener { _, metrics, dropCount ->
        // The object is reused after this returns: what is wanted is read now.
        val total = metrics.getMetric(FrameMetrics.TOTAL_DURATION)
        val deadline = if (Build.VERSION.SDK_INT >= 31) metrics.getMetric(FrameMetrics.DEADLINE) else NOMINAL_NANOS
        synchronized(this) {
            totals += total
            if (total > deadline) late++
            dropped += dropCount
        }
    }

    fun start() {
        window.addOnFrameMetricsAvailableListener(listener, Handler(thread.looper))
    }

    /** Stops, and says what was seen in a line. */
    fun stop(): String {
        window.removeOnFrameMetricsAvailableListener(listener)
        thread.quitSafely()
        thread.join(2_000)
        val seen = synchronized(this) { totals.sorted() }
        if (seen.isEmpty()) return "no frame was reported"
        fun at(share: Double) = seen[((seen.size - 1) * share).toInt()] / 1_000_000.0
        val percent = 100.0 * late / seen.size
        return "frames: ${seen.size}, over their deadline: $late (${"%.0f".format(percent)} %), reports the system dropped: $dropped, " +
            "frame time p50 ${"%.1f".format(at(0.5))} ms, p90 ${"%.1f".format(at(0.9))} ms, p99 ${"%.1f".format(at(0.99))} ms, slowest ${"%.1f".format(seen.last() / 1_000_000.0)} ms"
    }

    private companion object {
        const val NOMINAL_NANOS = 16_666_667L
    }
}
