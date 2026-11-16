package com.navree.client

import android.os.ParcelFileDescriptor
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.platform.LocalLayoutDirection
import androidx.compose.ui.test.SemanticsNodeInteractionsProvider
import androidx.compose.ui.test.isRoot
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.unit.LayoutDirection
import androidx.test.platform.app.InstrumentationRegistry

/**
 * Screenshots of the whole display, system bars included, written where the
 * cloud job pulls them from. The look (light, dark, large, rtl) and the device
 * come from the job as instrumentation arguments, so each file says what it shows.
 */
object Shots {
    const val FOLDER = "/data/local/tmp/navree-screens"

    private val arguments get() = InstrumentationRegistry.getArguments()

    /** The look the job set on the device before this run. */
    val look: String get() = arguments.getString("look") ?: "light"

    /** The device the job runs on: `phone` or `tablet`. */
    val device: String get() = arguments.getString("device") ?: "device"

    /**
     * The look's layout direction. Setting right to left as the developer
     * option does (`debug.force_rtl`) did not reach the test's process on the
     * emulator image, so in the rtl look the composition is laid out right to
     * left here; the system bars stay as the system draws them.
     */
    @Composable
    fun Looked(content: @Composable () -> Unit) {
        if (look == "rtl") {
            CompositionLocalProvider(LocalLayoutDirection provides LayoutDirection.Rtl) { content() }
        } else {
            content()
        }
    }

    private var demo = false

    /**
     * Puts the system bar in the system's own demo mode, as pictures that are
     * shown to people want it: a fixed time, full battery, full signal and
     * wifi, no notification icons. Once for the process; a device that does
     * not allow it keeps the bar it has.
     */
    fun demoStatusBar() {
        if (demo) return
        demo = true
        try {
            shell("settings put global sysui_demo_allowed 1")
            // The first look of a run is taken soon after the device came up, when the bar has not yet drawn the wifi icon it is told to
            // show: the commands are given twice, a moment apart.
            val commands = listOf(
                "enter",
                "clock -e hhmm 1000",
                "battery -e level 100 -e plugged false",
                "network -e wifi show -e level 4 -e fully true",
                // The emulator's cellular icon reads "3G" whatever it is told; a bar with no cell icon is clean.
                "network -e mobile hide",
                "network -e nosim hide",
                "notifications -e visible false",
            )
            for (pass in 1..2) {
                for (command in commands) shell("am broadcast -a com.android.systemui.demo -e command $command")
                if (pass == 1) Thread.sleep(1200)
            }
            // The bar fades its icons in and out; a picture waits for it to settle.
            Thread.sleep(1500)
        } catch (_: Exception) {
            // The pictures are taken with the bar as it is.
        }
    }

    /** The test's compose rule, so each picture also writes what a screen reader is told by the screen. */
    var rule: SemanticsNodeInteractionsProvider? = null

    /** The tree of the screen in front, as the server sent it, written beside each picture. */
    var tree: (() -> String?)? = null

    /** A window opened over the screen laid itself out by another direction than the screen's. */
    class WindowDirectionError(message: String) : AssertionError(message)

    /**
     * The layout direction each window the test can see has, as the system
     * resolved it, with whether it is the activity's own window (the screen,
     * which the right to left look lays out right to left by its composition)
     * or one opened over it: a dialog, a menu, a picker, a sheet.
     */
    fun windows(): List<Pair<Boolean, Int>> {
        val provider = rule ?: return emptyList()
        val decor = ((provider as? androidx.compose.ui.test.junit4.AndroidComposeTestRule<*, *>)?.activity as? android.app.Activity)?.window?.decorView
        return provider.onAllNodes(isRoot()).fetchSemanticsNodes().map { node ->
            val view = node.root as? android.view.View
            (view != null && view.rootView === decor) to (view?.layoutDirection ?: -1)
        }
    }

    /** What the look wants every window opened over the screen to have: right to left for the right to left look, left to right for the others. */
    val wantedDirection: Int get() = if (look == "rtl") android.view.View.LAYOUT_DIRECTION_RTL else android.view.View.LAYOUT_DIRECTION_LTR

    /**
     * Fails when a window opened over the screen has another direction than the
     * look's, or when none is open: everything opened over a screen has the
     * direction of the screen under it.
     */
    fun requireWindowsFollowTheScreen(what: String) {
        val over = windows().filter { !it.first }.map { it.second }
        if (over.isEmpty() || over.any { it != wantedDirection }) {
            throw WindowDirectionError("$what: the windows opened over the screen have the directions $over, the look wants $wantedDirection")
        }
    }

    fun take(name: String) {
        demoStatusBar()
        InstrumentationRegistry.getInstrumentation().waitForIdleSync()
        shell("mkdir -p $FOLDER")
        shell("screencap -p $FOLDER/$device-$look-$name.png")
        try {
            // The trees are the same in every look: one look keeps them.
            if (look == "light") tree?.invoke()?.let { put("$device-$look-$name.tree.json", it) }
        } catch (_: Throwable) {
            // As above: an extra.
        }
        try {
            put("$device-$look-$name.windows.txt", "windows (is it the activity's own, its direction): ${windows()}; over the screen the look wants $wantedDirection\n")
        } catch (_: Throwable) {
            // An extra, as above.
        }
        rule?.let { provider ->
            val roots = provider.onAllNodes(isRoot()).fetchSemanticsNodes()
            try {
                put("$device-$look-$name.sem.txt", Audit.report(roots))
            } catch (_: Throwable) {
                // The picture is what was asked for; the reading list is an extra.
            }
            // A picture taken while a screen moves (a zoom caught between frames) has controls the size of the frame it is in.
            if (!name.contains("zoom")) Audit.require("$device-$look-$name", Audit.faults(roots))
        }
    }

    /**
     * A keyboard's Tab as the system delivers it to the window, through the
     * activity: that is what puts Compose into keyboard mode, where a button, a
     * switch or a picker can take the focus (in touch mode only a text field and
     * what is made `focusable` itself can). A key event given to the test's root
     * directly leaves Compose in touch mode, and a walk made with it visits only
     * the text fields.
     */
    fun pressTab() = pressKey(android.view.KeyEvent.KEYCODE_TAB)

    /** Any key, down and up, delivered the same way: Enter on what has the focus, an arrow, Escape. */
    fun pressKey(code: Int) {
        InstrumentationRegistry.getInstrumentation().sendKeyDownUpSync(code)
    }

    /** Shift+Tab, delivered the same way. */
    fun pressShiftTab() {
        val now = android.os.SystemClock.uptimeMillis()
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        instrumentation.sendKeySync(android.view.KeyEvent(now, now, android.view.KeyEvent.ACTION_DOWN, android.view.KeyEvent.KEYCODE_TAB, 0, android.view.KeyEvent.META_SHIFT_ON))
        instrumentation.sendKeySync(android.view.KeyEvent(now, now, android.view.KeyEvent.ACTION_UP, android.view.KeyEvent.KEYCODE_TAB, 0, android.view.KeyEvent.META_SHIFT_ON))
    }

    /** Writes a text file beside the pictures, by way of the app's own folder the shell can read. */
    fun put(file: String, text: String) {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val folder = context.getExternalFilesDir(null) ?: return
        val local = java.io.File(folder, file)
        local.writeText(text)
        shell("mkdir -p $FOLDER")
        val said = shell("cp ${local.absolutePath} $FOLDER/$file")
        if (said.isNotBlank()) android.util.Log.w("NavreeShots", "writing $file: $said")
        local.delete()
    }

    /** Runs a command as the shell user and waits for it to finish. */
    fun shell(command: String): String {
        val pipe = InstrumentationRegistry.getInstrumentation().uiAutomation.executeShellCommand(command)
        return ParcelFileDescriptor.AutoCloseInputStream(pipe).use { it.readBytes().decodeToString() }
    }
}
