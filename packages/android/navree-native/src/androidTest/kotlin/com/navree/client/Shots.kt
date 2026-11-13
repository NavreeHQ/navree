package com.navree.client

import android.os.ParcelFileDescriptor
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

    fun take(name: String) {
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
