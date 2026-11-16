package com.navree.client.ui

import androidx.compose.runtime.Composable

/**
 * A tap on what composing a card costs, for a test to write down: it is told the name of a part of a card and the
 * nanoseconds the part took to compose each time one is composed. Nothing in the app sets it. A part is a trace
 * section as well (`android.os.Trace`), so a system trace of the app shows the same parts by the same names.
 * A part's time holds the parts inside it.
 */
internal object CardCost {
    @Volatile
    var tap: ((String, Long) -> Unit)? = null
}

/** Tells the tap how long a part of a card that is not a block of its own took, from `began` (a `System.nanoTime()`). */
internal fun cardPartSince(name: String, began: Long) {
    CardCost.tap?.invoke(name, System.nanoTime() - began)
}

/** Composes `content` as a named part of a card: a trace section, and a line to the tap when there is one. */
@Composable
internal inline fun CardPart(name: String, content: @Composable () -> Unit) {
    android.os.Trace.beginSection(name)
    val tap = CardCost.tap
    val began = if (tap != null) System.nanoTime() else 0L
    content()
    if (tap != null) tap(name, System.nanoTime() - began)
    android.os.Trace.endSection()
}
