package ro.dragoscatalin.scrin.ui.theme

import androidx.annotation.StringRes
import androidx.compose.ui.graphics.toArgb
import app.scrin.brand.BrandDark
import app.scrin.brand.BrandLight
import app.scrin.brand.BrandTokens
import ro.dragoscatalin.scrin.R

/** One accent swatch; mirrors ACCENT_PRESETS in packages/ui/src/theme/presets.ts. */
data class AccentPreset(val id: String, val hue: Double, val chroma: Double, @param:StringRes val label: Int)

object Tokens {
    val ACCENTS: List<AccentPreset> = listOf(
        AccentPreset(LAGOON, BrandTokens.ACCENT_HUE.toDouble(), BrandTokens.ACCENT_CHROMA.toDouble(), R.string.accent_lagoon),
        AccentPreset("iris", 264.0, 0.17, R.string.accent_iris),
        AccentPreset("ocean", 245.0, 0.15, R.string.accent_ocean),
        AccentPreset("sky", 225.0, 0.13, R.string.accent_sky),
        AccentPreset("teal", 190.0, 0.12, R.string.accent_teal),
        AccentPreset("mint", 160.0, 0.13, R.string.accent_mint),
        AccentPreset("lime", 130.0, 0.15, R.string.accent_lime),
        AccentPreset("amber", 75.0, 0.15, R.string.accent_amber),
        AccentPreset("tangerine", 50.0, 0.17, R.string.accent_tangerine),
        AccentPreset("coral", 30.0, 0.17, R.string.accent_coral),
        AccentPreset("rose", 5.0, 0.18, R.string.accent_rose),
        AccentPreset("orchid", 330.0, 0.17, R.string.accent_orchid),
        AccentPreset("violet", 295.0, 0.18, R.string.accent_violet),
        AccentPreset("graphite", 260.0, 0.02, R.string.accent_graphite),
    )

    /** The brand accent (brand/brand.json); its palette is the brand pack's, not derived. */
    const val LAGOON = "lagoon"
    const val DEFAULT_ACCENT = LAGOON

    fun accent(id: String): AccentPreset = ACCENTS.firstOrNull { it.id == id } ?: ACCENTS.first()

    /** Palette for one mode, same L/C as packages/ui/src/theme.css; hue follows the accent. */
    data class Palette(
        val bg: Int,
        val surface: Int,
        val surface2: Int,
        val fg: Int,
        val muted: Int,
        val outline: Int,
        val accent: Int,
        val accentFg: Int,
        val accentContainer: Int,
        val success: Int,
        val warning: Int,
        val danger: Int,
    )

    fun palette(a: AccentPreset, dark: Boolean): Palette {
        if (a.id == LAGOON) return brandPalette(dark)
        val h = a.hue
        // Neutrals carry a whisper of the accent hue (0.007–0.014 chroma), like the web tokens.
        val n = if (a.chroma < 0.05) 0.006 else 0.014
        fun ok(l: Double, c: Double, hue: Double = h) = Oklch.toArgb(l, c, hue)
        return if (!dark) {
            Palette(
                bg = ok(0.985, n / 2), surface = ok(1.0, 0.0), surface2 = ok(0.955, n),
                fg = ok(0.2, n), muted = ok(0.47, n), outline = ok(0.86, n),
                accent = ok(0.52, a.chroma), accentFg = ok(0.99, 0.0),
                accentContainer = ok(0.92, a.chroma * 0.35),
                success = ok(0.5, 0.13, 150.0), warning = ok(0.55, 0.14, 70.0), danger = ok(0.53, 0.19, 27.0),
            )
        } else {
            Palette(
                bg = ok(0.17, n), surface = ok(0.215, n), surface2 = ok(0.26, n),
                fg = ok(0.96, n / 2), muted = ok(0.74, n), outline = ok(0.36, n),
                accent = ok(0.76, a.chroma), accentFg = ok(0.18, n),
                accentContainer = ok(0.32, a.chroma * 0.5),
                success = ok(0.74, 0.13, 150.0), warning = ok(0.8, 0.14, 70.0), danger = ok(0.7, 0.17, 27.0),
            )
        }
    }

    /** Lagoon = the exact brand tokens (brand/dist/kotlin/BrandTokens.kt); status colours stay derived. */
    private fun brandPalette(dark: Boolean): Palette {
        val hueOnly = AccentPreset("lagoon-derived", BrandTokens.ACCENT_HUE.toDouble(), BrandTokens.ACCENT_CHROMA.toDouble(), R.string.accent_lagoon)
        val derived = palette(hueOnly, dark)
        return if (!dark) {
            Palette(
                bg = BrandLight.bgCanvas.toArgb(), surface = BrandLight.bgSurface.toArgb(), surface2 = BrandLight.bgSubtle.toArgb(),
                fg = BrandLight.fgDefault.toArgb(), muted = BrandLight.fgMuted.toArgb(), outline = BrandLight.borderDefault.toArgb(),
                accent = BrandLight.accentSolid.toArgb(), accentFg = BrandLight.fgOnAccent.toArgb(),
                accentContainer = BrandLight.accentSubtle.toArgb(),
                success = derived.success, warning = derived.warning, danger = derived.danger,
            )
        } else {
            Palette(
                bg = BrandDark.bgCanvas.toArgb(), surface = BrandDark.bgSurface.toArgb(), surface2 = BrandDark.bgSubtle.toArgb(),
                fg = BrandDark.fgDefault.toArgb(), muted = BrandDark.fgMuted.toArgb(), outline = BrandDark.borderDefault.toArgb(),
                accent = BrandDark.accentSolid.toArgb(), accentFg = BrandDark.fgOnAccent.toArgb(),
                accentContainer = BrandDark.accentSubtle.toArgb(),
                success = derived.success, warning = derived.warning, danger = derived.danger,
            )
        }
    }
}
