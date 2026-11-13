package com.navree.client.ui

/**
 * Tells apart two reasons the state's text differs from what a text control
 * shows: the control's own report coming back a moment late, and a change made
 * elsewhere (the server sent the form again, a search was cleared).
 *
 * A control shows what is typed at once and reports it to the state. The
 * state reaches the control again a frame later, by which time more may have
 * been typed. Taking that late value for the truth would eat the newer
 * letters, so the control remembers what it reported and lets those pass.
 */
internal class Echo {
    private val reported = ArrayDeque<String>()

    /** The control told the state its text is now `text`. */
    fun report(text: String) {
        reported.addLast(text)
    }

    /**
     * Whether `current`, the state's text, was set by something other than
     * this control, so the control should show it in place of `shown`.
     */
    fun changedOutside(current: String, shown: String): Boolean {
        if (current == shown) {
            reported.clear()
            return false
        }
        val at = reported.indexOf(current)
        if (at >= 0) {
            repeat(at + 1) { reported.removeFirst() }
            return false
        }
        reported.clear()
        return true
    }
}
