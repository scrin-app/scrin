package ro.dragoscatalin.scrin.ui.theme

import android.os.Build
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.ColorScheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Shapes
import androidx.compose.material3.Typography
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.dynamicDarkColorScheme
import androidx.compose.material3.dynamicLightColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import ro.dragoscatalin.scrin.data.ThemeMode

/** Status colours outside Material's scheme. */
@Immutable
data class ScrinColors(val success: Color, val warning: Color, val danger: Color)

val LocalScrinColors = staticCompositionLocalOf {
    ScrinColors(Color(0xFF2E7D32), Color(0xFFB26A00), Color(0xFFC62828))
}

fun schemeFor(p: Tokens.Palette, dark: Boolean): ColorScheme {
    val c = { v: Int -> Color(v) }
    return if (dark) {
        darkColorScheme(
            primary = c(p.accent), onPrimary = c(p.accentFg),
            primaryContainer = c(p.accentContainer), onPrimaryContainer = c(p.fg),
            secondary = c(p.accent), onSecondary = c(p.accentFg),
            secondaryContainer = c(p.surface2), onSecondaryContainer = c(p.fg),
            tertiary = c(p.success), onTertiary = c(p.bg),
            background = c(p.bg), onBackground = c(p.fg),
            surface = c(p.bg), onSurface = c(p.fg),
            surfaceVariant = c(p.surface2), onSurfaceVariant = c(p.muted),
            surfaceContainerLowest = c(p.bg), surfaceContainerLow = c(p.surface),
            surfaceContainer = c(p.surface), surfaceContainerHigh = c(p.surface2),
            surfaceContainerHighest = c(p.surface2),
            outline = c(p.outline), outlineVariant = c(p.outline),
            error = c(p.danger), onError = c(p.bg),
        )
    } else {
        lightColorScheme(
            primary = c(p.accent), onPrimary = c(p.accentFg),
            primaryContainer = c(p.accentContainer), onPrimaryContainer = c(p.fg),
            secondary = c(p.accent), onSecondary = c(p.accentFg),
            secondaryContainer = c(p.surface2), onSecondaryContainer = c(p.fg),
            tertiary = c(p.success), onTertiary = c(p.surface),
            background = c(p.bg), onBackground = c(p.fg),
            surface = c(p.bg), onSurface = c(p.fg),
            surfaceVariant = c(p.surface2), onSurfaceVariant = c(p.muted),
            surfaceContainerLowest = c(p.surface), surfaceContainerLow = c(p.surface),
            surfaceContainer = c(p.surface), surfaceContainerHigh = c(p.surface2),
            surfaceContainerHighest = c(p.surface2),
            outline = c(p.outline), outlineVariant = c(p.outline),
            error = c(p.danger), onError = c(p.surface),
        )
    }
}

private val ScrinShapes = Shapes(
    extraSmall = RoundedCornerShape(8.dp),
    small = RoundedCornerShape(12.dp),
    medium = RoundedCornerShape(16.dp),
    large = RoundedCornerShape(24.dp),
    extraLarge = RoundedCornerShape(32.dp),
)

private fun typography(): Typography {
    val base = Typography()
    return base.copy(
        displaySmall = base.displaySmall.copy(fontWeight = FontWeight.SemiBold, letterSpacing = (-0.5).sp),
        headlineMedium = base.headlineMedium.copy(fontWeight = FontWeight.SemiBold),
        headlineSmall = base.headlineSmall.copy(fontWeight = FontWeight.SemiBold),
        titleLarge = base.titleLarge.copy(fontWeight = FontWeight.SemiBold),
        titleMedium = base.titleMedium.copy(fontWeight = FontWeight.SemiBold),
        labelLarge = base.labelLarge.copy(fontWeight = FontWeight.SemiBold),
    )
}

/** Monospace style for IDs and codes: tabular, wide tracking. */
val CodeStyle = TextStyle(fontFamily = androidx.compose.ui.text.font.FontFamily.Monospace, fontWeight = FontWeight.SemiBold, letterSpacing = 2.sp)

@Composable
fun ScrinTheme(mode: ThemeMode, accentId: String, dynamic: Boolean, content: @Composable () -> Unit) {
    val dark = when (mode) {
        ThemeMode.SYSTEM -> isSystemInDarkTheme()
        ThemeMode.LIGHT -> false
        ThemeMode.DARK -> true
    }
    val palette = Tokens.palette(Tokens.accent(accentId), dark)
    val scheme = if (dynamic && Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
        val ctx = LocalContext.current
        if (dark) dynamicDarkColorScheme(ctx) else dynamicLightColorScheme(ctx)
    } else {
        schemeFor(palette, dark)
    }
    androidx.compose.runtime.CompositionLocalProvider(
        LocalScrinColors provides ScrinColors(Color(palette.success), Color(palette.warning), Color(palette.danger)),
    ) {
        MaterialTheme(colorScheme = scheme, shapes = ScrinShapes, typography = typography(), content = content)
    }
}
