package cn.juniexd.nexttodo

import androidx.compose.ui.graphics.Color

/** The colours of docs/ui.md that the native parts use, light and dark: the widget, the reminder card
 * and the add popup. */
class Palette(
    val primary: Color,
    val onPrimary: Color,
    val secondaryContainer: Color,
    val onSecondaryContainer: Color,
    val surface: Color,
    val group: Color,
    val surfaceContainer: Color,
    val surfaceHigh: Color,
    val onSurface: Color,
    val onSurfaceVariant: Color,
    val outline: Color,
    val rail: Color,
    /** The picked item of a segmented control. */
    val segOn: Color,
    val error: Color,
    val warn: Color,
    /** Fainter than outline-variant: the widget can't draw the app's half-pixel lines. */
    val divider: Color,
) {
    companion object {
        val light = Palette(
            primary = Color(0xFF1B6B51),
            onPrimary = Color(0xFFFFFFFF),
            secondaryContainer = Color(0xFFCEE9DB),
            onSecondaryContainer = Color(0xFF082016),
            surface = Color(0xFFF5FBF6),
            group = Color(0xFFFFFFFF),
            surfaceContainer = Color(0xFFE9EFEA),
            surfaceHigh = Color(0xFFE4EAE5),
            onSurface = Color(0xFF171D1A),
            onSurfaceVariant = Color(0xFF404944),
            outline = Color(0xFF707973),
            rail = Color(0xFFBFC9C2),
            segOn = Color(0xFFFFFFFF),
            error = Color(0xFFBA1A1A),
            warn = Color(0xFF8A5100),
            divider = Color(0xFFDDE4DF),
        )

        val dark = Palette(
            primary = Color(0xFF8BD6B6),
            onPrimary = Color(0xFF003827),
            secondaryContainer = Color(0xFF334B40),
            onSecondaryContainer = Color(0xFFCEE9DB),
            surface = Color(0xFF0F1512),
            group = Color(0xFF1B211E),
            surfaceContainer = Color(0xFF222825),
            surfaceHigh = Color(0xFF2A302D),
            onSurface = Color(0xFFDEE4DF),
            onSurfaceVariant = Color(0xFFBFC9C2),
            outline = Color(0xFF89938D),
            rail = Color(0xFF56605A),
            segOn = Color(0xFF3A413D),
            error = Color(0xFFFFB4AB),
            warn = Color(0xFFFFB870),
            divider = Color(0xFF2A302D),
        )
    }
}
