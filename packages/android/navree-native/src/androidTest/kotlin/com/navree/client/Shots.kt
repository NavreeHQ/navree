package com.navree.client

import android.os.ParcelFileDescriptor
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.platform.LocalLayoutDirection
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
            for (command in listOf(
                "enter",
                "clock -e hhmm 1000",
                "battery -e level 100 -e plugged false",
                "network -e wifi show -e level 4 -e fully true",
                "network -e mobile show -e datatype none -e level 4 -e fully true",
                "notifications -e visible false",
            )) {
                shell("am broadcast -a com.android.systemui.demo -e command $command")
            }
            Thread.sleep(500)
        } catch (_: Exception) {
            // The pictures are taken with the bar as it is.
        }
    }

    fun take(name: String) {
        demoStatusBar()
        InstrumentationRegistry.getInstrumentation().waitForIdleSync()
        shell("mkdir -p $FOLDER")
        shell("screencap -p $FOLDER/$device-$look-$name.png")
    }

    /** Runs a command as the shell user and waits for it to finish. */
    fun shell(command: String): String {
        val pipe = InstrumentationRegistry.getInstrumentation().uiAutomation.executeShellCommand(command)
        return ParcelFileDescriptor.AutoCloseInputStream(pipe).use { it.readBytes().decodeToString() }
    }
}
