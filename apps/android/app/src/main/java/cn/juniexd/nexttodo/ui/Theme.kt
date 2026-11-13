package cn.juniexd.nexttodo.ui

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.staticCompositionLocalOf
import cn.juniexd.nexttodo.Palette

/** The palette in use, for colours Material 3 has no role for. */
val LocalPalette = staticCompositionLocalOf { Palette.light }

/** The app's colours in Material 3's roles, light or dark as the system is: the native parts can't
 * read the theme chosen in the app. */
@Composable
fun NextTodoTheme(content: @Composable () -> Unit) {
    val dark = isSystemInDarkTheme()
    val p = if (dark) Palette.dark else Palette.light
    val colors = if (dark) {
        darkColorScheme(
            primary = p.primary, onPrimary = p.onPrimary, secondaryContainer = p.secondaryContainer, onSecondaryContainer = p.onSecondaryContainer,
            surface = p.surface, onSurface = p.onSurface, onSurfaceVariant = p.onSurfaceVariant, surfaceContainer = p.surfaceContainer,
            surfaceContainerHigh = p.surfaceHigh, outline = p.outline, error = p.error,
        )
    } else {
        lightColorScheme(
            primary = p.primary, onPrimary = p.onPrimary, secondaryContainer = p.secondaryContainer, onSecondaryContainer = p.onSecondaryContainer,
            surface = p.surface, onSurface = p.onSurface, onSurfaceVariant = p.onSurfaceVariant, surfaceContainer = p.surfaceContainer,
            surfaceContainerHigh = p.surfaceHigh, outline = p.outline, error = p.error,
        )
    }
    CompositionLocalProvider(LocalPalette provides p) { MaterialTheme(colorScheme = colors, content = content) }
}
