package com.navree.client.ui

import androidx.compose.runtime.Composable
import com.navree.client.BuildConfig

/**
 * A tap on what composing a card costs, for a test to write down: it is told the name of a part of a card and the
 * nanoseconds the part took to compose each time one is composed. Nothing in the app sets it. A part is a trace
 * section as well (`android.os.Trace`), so a system trace of the app shows the same parts by the same names.
 * A part's time holds the parts inside it.
 *
 * All of it is there only in a build that can be debugged: every use is behind `BuildConfig.DEBUG`, which is a
 * constant, so a release build of the library holds neither the trace sections nor a read of the tap or of a clock.
 */
internal object CardCost {
    @Volatile
    var tap: ((String, Long) -> Unit)? = null
}

/** The time to measure a part from: nothing is read where no test listens, and nothing at all in a release build. */
internal fun cardClock(): Long = if (BuildConfig.DEBUG && CardCost.tap != null) System.nanoTime() else 0L

/** Tells the tap how long a part of a card that is not a block of its own took, from `began` (a `cardClock()`). */
internal fun cardPartSince(name: String, began: Long) {
    if (BuildConfig.DEBUG) CardCost.tap?.invoke(name, System.nanoTime() - began)
}

/**
 * Composes `content` as a named part of a card: in a build that can be debugged a trace section, and a line to the
 * tap when there is one; in a release build `content` and nothing else.
 */
@Composable
internal inline fun CardPart(name: String, content: @Composable () -> Unit) {
    if (BuildConfig.DEBUG) android.os.Trace.beginSection(name)
    val tap = if (BuildConfig.DEBUG) CardCost.tap else null
    val began = if (tap != null) System.nanoTime() else 0L
    content()
    if (tap != null) tap(name, System.nanoTime() - began)
    if (BuildConfig.DEBUG) android.os.Trace.endSection()
}
