// SPDX-License-Identifier: GPL-3.0-or-later
package com.davefx.clipboardwire

import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class MainActivityTest {

    @get:Rule
    val composeRule = createAndroidComposeRule<MainActivity>()

    @Test
    fun settings_screen_shows_all_fields() {
        composeRule.onNodeWithText("clipboardwire").assertIsDisplayed()
        composeRule.onNodeWithText("Server URL").assertIsDisplayed()
        composeRule.onNodeWithText("Username").assertIsDisplayed()
        composeRule.onNodeWithText("Password").assertIsDisplayed()
        composeRule.onNodeWithText("Skip TLS verification (LAN/VPN only)").assertIsDisplayed()
        composeRule.onNodeWithText("Save & Connect").assertIsDisplayed()
        composeRule.onNodeWithText("Stop service").assertIsDisplayed()
    }

    @Test
    fun can_type_into_server_url_field() {
        composeRule.onNodeWithText("Server URL").performTextInput("wss://test:8484/sync")
        composeRule.onNodeWithText("wss://test:8484/sync").assertIsDisplayed()
    }

    @Test
    fun can_type_into_username_field() {
        composeRule.onNodeWithText("Username").performTextInput("alice")
        composeRule.onNodeWithText("alice").assertIsDisplayed()
    }

    @Test
    fun can_toggle_tls_checkbox() {
        val checkbox = composeRule.onNodeWithText("Skip TLS verification (LAN/VPN only)")
        checkbox.assertIsDisplayed()
        // Toggle the checkbox by clicking it
        checkbox.performClick()
    }
}
