package com.navree.client

import androidx.activity.ComponentActivity
import androidx.compose.foundation.layout.Column
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.getValue
import androidx.compose.runtime.setValue
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

/**
 * The direction every window of the app has under the look the job runs: a
 * dialog and a menu opened over a screen are laid out the way the screen is,
 * left to right in every look but the right to left one, which lays out the
 * app's own composition right to left (the system's language is not changed,
 * so this is what that look is).
 */
@RunWith(AndroidJUnit4::class)
class LanguageTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    @Before
    fun readingListsFollowThePictures() {
        Shots.rule = compose
    }

    @OptIn(androidx.compose.material3.ExperimentalMaterial3Api::class)
    @Test
    fun aDialogAMenuAndASheetHaveTheLooksDirection() {
        var menu by androidx.compose.runtime.mutableStateOf(false)
        var sheet by androidx.compose.runtime.mutableStateOf(false)
        compose.setContent {
            MaterialTheme {
                Shots.Looked {
                    Column {
                        Text("Screen")
                        AlertDialog(onDismissRequest = {}, confirmButton = { TextButton({}) { Text("Yes") } }, dismissButton = { TextButton({}) { Text("No") } }, title = { Text("Question") })
                        DropdownMenu(expanded = menu, onDismissRequest = {}) { DropdownMenuItem(text = { Text("Item") }, onClick = {}) }
                        if (sheet) androidx.compose.material3.ModalBottomSheet(onDismissRequest = {}) { Text("Sheet") }
                    }
                }
            }
        }
        compose.waitForIdle()
        Shots.requireWindowsFollowTheScreen("a dialog")
        compose.runOnUiThread { menu = true }
        compose.waitForIdle()
        Shots.requireWindowsFollowTheScreen("a dialog and a menu")
        compose.runOnUiThread { sheet = true }
        compose.waitForIdle()
        Thread.sleep(800)
        Shots.requireWindowsFollowTheScreen("a dialog, a menu and a sheet")
        Shots.take("language-windows")
    }
}
