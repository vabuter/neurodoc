package today.vab.neurodoceditor.ui.theme

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Typography
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.graphics.Color
import today.vab.neurodoceditor.data.ThemeMode

// Палитра повторяет десктопную версию: тёмный графит + teal-акцент из иконки.
private val Teal = Color(0xFF12A094)
private val TealLight = Color(0xFF4DD6C7)
private val DarkBackground = Color(0xFF111111)
private val DarkEditor = Color(0xFF1A1A1A)
private val DarkSurface = Color(0xFF1E1E1E)
private val LightEditor = Color(0xFFFFFFFF)
private val LightBackground = Color(0xFFF3F3F3)

/** Дополнительные цвета приложения, которых нет в Material-схеме. */
data class NeurodocColors(
    val editorBackground: Color,
    val paneBackground: Color,
    val cardBorder: Color,
    val dimText: Color,
    val linkChip: Color,
    val linkChipHover: Color,
    val linkBorder: Color,
    val webLink: Color,
    /** Цвет уже прочитанной ссылки — намеренно нейтральный, а не акцентный. */
    val visitedLink: Color,
    val findHighlight: Color,
    val findCurrent: Color,
)

val LocalNeurodocColors = staticCompositionLocalOf {
    NeurodocColors(
        editorBackground = DarkEditor,
        paneBackground = Color(0x0BFFFFFF),
        cardBorder = Color(0x14FFFFFF),
        dimText = Color(0x8CFFFFFF),
        linkChip = Color(0x2212A094),
        linkChipHover = Color(0x4412A094),
        linkBorder = Color(0x8812A094),
        webLink = Color(0xFF4DA3FF),
        visitedLink = Color(0xFF86868E),
        findHighlight = Color(0x66FFC107),
        findCurrent = Color(0xE6FF9800),
    )
}

private val DarkColors = darkColorScheme(
    primary = TealLight,
    onPrimary = Color(0xFF00201C),
    primaryContainer = Color(0xFF00504A),
    onPrimaryContainer = Color(0xFF9FF2E6),
    secondary = TealLight,
    background = DarkBackground,
    onBackground = Color(0xFFE8E8E8),
    surface = DarkSurface,
    onSurface = Color(0xFFE8E8E8),
    surfaceVariant = Color(0xFF262626),
    onSurfaceVariant = Color(0xFFBDBDBD),
    outline = Color(0xFF4A4A4A),
    error = Color(0xFFFF6B6B),
)

private val LightColors = lightColorScheme(
    primary = Teal,
    onPrimary = Color.White,
    primaryContainer = Color(0xFFA7F3E9),
    onPrimaryContainer = Color(0xFF00201C),
    secondary = Teal,
    background = LightBackground,
    onBackground = Color(0xFF1B1B1B),
    surface = LightEditor,
    onSurface = Color(0xFF1B1B1B),
    surfaceVariant = Color(0xFFE7E7E7),
    onSurfaceVariant = Color(0xFF4A4A4A),
    outline = Color(0xFFBDBDBD),
)

private val DarkExtras = NeurodocColors(
    editorBackground = DarkEditor,
    paneBackground = Color(0x0BFFFFFF),
    cardBorder = Color(0x14FFFFFF),
    dimText = Color(0x8CFFFFFF),
    linkChip = Color(0x2212A094),
    linkChipHover = Color(0x4412A094),
    linkBorder = Color(0x8812A094),
    webLink = Color(0xFF4DA3FF),
    visitedLink = Color(0xFF86868E),
    findHighlight = Color(0x66FFC107),
    findCurrent = Color(0xE6FF9800),
)

private val LightExtras = NeurodocColors(
    editorBackground = LightEditor,
    paneBackground = Color(0x99FFFFFF),
    cardBorder = Color(0x17000000),
    dimText = Color(0x8C000000),
    linkChip = Color(0x1A12A094),
    linkChipHover = Color(0x3312A094),
    linkBorder = Color(0x9912A094),
    webLink = Color(0xFF0B62C4),
    visitedLink = Color(0xFF767680),
    findHighlight = Color(0x88FFC107),
    findCurrent = Color(0xE6FF9800),
)

@Composable
fun NeurodocTheme(themeMode: ThemeMode, content: @Composable () -> Unit) {
    val dark = when (themeMode) {
        ThemeMode.SYSTEM -> isSystemInDarkTheme()
        ThemeMode.LIGHT -> false
        ThemeMode.DARK -> true
    }
    CompositionLocalProvider(LocalNeurodocColors provides if (dark) DarkExtras else LightExtras) {
        MaterialTheme(
            colorScheme = if (dark) DarkColors else LightColors,
            typography = Typography(),
            content = content,
        )
    }
}
