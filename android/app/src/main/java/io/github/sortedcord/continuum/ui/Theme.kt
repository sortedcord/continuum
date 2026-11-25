package io.github.sortedcord.continuum.ui

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.dynamicDarkColorScheme
import androidx.compose.material3.dynamicLightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.platform.LocalContext

/** Material You colours; available on every supported version (API 31+). */
@Composable
fun ContinuumTheme(content: @Composable () -> Unit) {
    val context = LocalContext.current
    val scheme = if (isSystemInDarkTheme()) dynamicDarkColorScheme(context) else dynamicLightColorScheme(context)
    MaterialTheme(colorScheme = scheme, content = content)
}
