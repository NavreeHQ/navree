package com.navree.client

import android.util.Log
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.junit.Test
import org.junit.runner.RunWith

/** Sets the system's language for the look the job runs next, and says what the system then holds. */
@RunWith(AndroidJUnit4::class)
class LanguageTest {
    @Test
    fun theSystemSpeaksTheLooksLanguage() {
        val said = Shots.language()
        Log.i("NavreeShots", said)
        println("language: $said")
    }
}
