package com.navree.client

import android.os.Build
import android.os.Handler
import android.os.HandlerThread
import android.view.FrameMetrics
import android.view.Window

/**
 * The frames a window draws while something is done to it, from the platform's own account of each
 * (`Window.OnFrameMetricsAvailableListener` and `FrameMetrics`): how many, how many took longer than the time
 * the system gave them (`DEADLINE`, from Android 12; a 60 Hz frame before it), the 50th, 90th and 99th
 * percentile of the time a frame took (`TOTAL_DURATION`), and the median of each stage of a frame, so a frame
 * that is slow says in which stage: the UI thread's input, animation (where Compose runs its frames), measure
 * and layout (where a lazy list composes the items it lays), drawing, or the render thread's own work.
 */
internal class FrameStats(private val window: Window) {
    private val totals = ArrayList<Long>()
    private val stages = STAGES.associate { it.first to ArrayList<Long>() }
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
            for ((id, _) in STAGES) {
                if (id != FrameMetrics.GPU_DURATION || Build.VERSION.SDK_INT >= 31) stages.getValue(id) += metrics.getMetric(id)
            }
        }
    }

    fun start() {
        window.addOnFrameMetricsAvailableListener(listener, Handler(thread.looper))
    }

    /** Stops, and says what was seen in two lines. */
    fun stop(): String {
        window.removeOnFrameMetricsAvailableListener(listener)
        thread.quitSafely()
        thread.join(2_000)
        val seen = synchronized(this) { totals.sorted() }
        if (seen.isEmpty()) return "no frame was reported"
        fun at(values: List<Long>, share: Double) = values[((values.size - 1) * share).toInt()] / 1_000_000.0
        val percent = 100.0 * late / seen.size
        val medians = synchronized(this) {
            STAGES.filter { stages.getValue(it.first).isNotEmpty() }.joinToString(", ") { (id, name) -> "$name ${"%.1f".format(at(stages.getValue(id).sorted(), 0.5))}" }
        }
        return "frames: ${seen.size}, over their deadline: $late (${"%.0f".format(percent)} %), reports the system dropped: $dropped, " +
            "frame time p50 ${"%.1f".format(at(seen, 0.5))} ms, p90 ${"%.1f".format(at(seen, 0.9))} ms, p99 ${"%.1f".format(at(seen, 0.99))} ms, slowest ${"%.1f".format(seen.last() / 1_000_000.0)} ms\n" +
            "median of each stage of a frame, in ms: $medians"
    }

    private companion object {
        const val NOMINAL_NANOS = 16_666_667L

        val STAGES = listOf(
            FrameMetrics.UNKNOWN_DELAY_DURATION to "waiting for the UI thread",
            FrameMetrics.INPUT_HANDLING_DURATION to "input",
            FrameMetrics.ANIMATION_DURATION to "animation callbacks",
            FrameMetrics.LAYOUT_MEASURE_DURATION to "measure and layout",
            FrameMetrics.DRAW_DURATION to "draw",
            FrameMetrics.SYNC_DURATION to "sync",
            FrameMetrics.COMMAND_ISSUE_DURATION to "commands to the GPU",
            FrameMetrics.SWAP_BUFFERS_DURATION to "swap buffers",
            FrameMetrics.GPU_DURATION to "GPU",
        )
    }
}
