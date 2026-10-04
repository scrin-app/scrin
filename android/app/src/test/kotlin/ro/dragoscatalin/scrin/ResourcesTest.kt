package ro.dragoscatalin.scrin

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import ro.dragoscatalin.scrin.ui.theme.Oklch
import ro.dragoscatalin.scrin.ui.theme.Tokens
import java.io.File
import javax.xml.parsers.DocumentBuilderFactory
import kotlin.math.abs
import kotlin.math.pow

/** Drift and policy checks on resources and tokens (no Android runtime needed). */
class ResourcesTest {
    private val res = File("src/main/res")

    private fun strings(dir: String): Map<String, String> {
        val doc = DocumentBuilderFactory.newInstance().newDocumentBuilder().parse(File(res, "$dir/strings.xml"))
        val nodes = doc.getElementsByTagName("string")
        return (0 until nodes.length).map { nodes.item(it) }
            .filter { it.attributes.getNamedItem("translatable")?.nodeValue != "false" }
            .associate { it.attributes.getNamedItem("name").nodeValue to it.textContent }
    }

    @Test fun everyEnglishStringExistsInRomanianAndViceVersa() {
        val en = strings("values")
        val ro = strings("values-ro")
        assertEquals("missing in values-ro", emptySet<String>(), en.keys - ro.keys)
        assertEquals("only in values-ro", emptySet<String>(), ro.keys - en.keys)
    }

    @Test fun placeholdersMatchBetweenLocales() {
        val en = strings("values")
        val ro = strings("values-ro")
        val ph = Regex("%\\d+\\$[sd]")
        en.forEach { (k, v) ->
            assertEquals("placeholders of $k", ph.findAll(v).map { it.value }.toSet(), ph.findAll(ro.getValue(k)).map { it.value }.toSet())
        }
    }

    @Test fun romanianUsesCommaBelowDiacritics() {
        // ş/ţ (cedilla) are wrong in Romanian; ș/ț (comma below) are right.
        strings("values-ro").forEach { (k, v) -> assertFalse("cedilla in $k", v.contains('ş') || v.contains('ţ') || v.contains('Ş') || v.contains('Ţ')) }
    }

    @Test fun accessibilityServiceIsNotDeclaredAnAccessibilityTool() {
        val xml = File(res, "xml/accessibility_service_config.xml").readText()
        assertFalse(Regex("isAccessibilityTool\\s*=\\s*\"true\"").containsMatchIn(xml))
        assertTrue(xml.contains("canPerformGestures=\"true\""))
        assertTrue(xml.contains("@string/a11y_service_description"))
    }

    @Test fun oklchMatchesKnownSrgbValues() {
        assertEquals(0xFFFFFFFF.toInt(), Oklch.toArgb(1.0, 0.0, 0.0))
        assertEquals(0xFF000000.toInt(), Oklch.toArgb(0.0, 0.0, 0.0))
        // oklch(0.628 0.2577 29.23) is CSS red #ff0000 (±1 per channel).
        val red = Oklch.toArgb(0.62796, 0.25768, 29.2339)
        assertTrue(abs(((red shr 16) and 0xFF) - 255) <= 1)
        assertTrue(((red shr 8) and 0xFF) <= 1)
        assertTrue((red and 0xFF) <= 1)
    }

    @Test fun defaultAccentIsTheBrandLagoon() {
        assertEquals("lagoon", Tokens.DEFAULT_ACCENT)
        assertEquals("lagoon", Tokens.ACCENTS.first().id)
        assertEquals(0xFF007B69.toInt(), Tokens.palette(Tokens.accent("lagoon"), dark = false).accent)
        assertEquals(0xFF00CDB1.toInt(), Tokens.palette(Tokens.accent("lagoon"), dark = true).accent)
    }

    @Test fun brandAssetsAreVerbatimCopiesOfTheBrandPack() {
        val brand = File("../../brand/android")
        val pairs = listOf(
            "mipmap-anydpi-v26/ic_launcher.xml" to "mipmap-anydpi/ic_launcher.xml",
            "mipmap-anydpi-v26/ic_launcher_round.xml" to "mipmap-anydpi/ic_launcher_round.xml",
            "drawable/ic_launcher_foreground.xml" to "drawable/ic_launcher_foreground.xml",
            "drawable/ic_launcher_background.xml" to "drawable/ic_launcher_background.xml",
            "drawable/ic_launcher_monochrome.xml" to "drawable/ic_launcher_monochrome.xml",
            "drawable/avd_scrin_splash.xml" to "drawable/avd_scrin_splash.xml",
            "drawable-nodpi/splash_icon_288dp_xxxhdpi.png" to "drawable-nodpi/splash_icon_288dp_xxxhdpi.png",
            "values/brand_colors.xml" to "values/brand_colors.xml",
        )
        // The brand pack's mipmap-*/ic_launcher*.webp fallbacks are deliberately NOT shipped:
        // minSdk 26 always resolves the adaptive mipmap-anydpi icon, so they would be dead weight.
        assertFalse(File(res, "mipmap-xxxhdpi/ic_launcher.webp").exists())
        pairs.forEach { (src, dst) ->
            assertTrue("brand drift: $dst", File(brand, src).readBytes().contentEquals(File(res, dst).readBytes()))
        }
        val tokens = File("../../brand/dist/kotlin/BrandTokens.kt").readBytes()
        assertTrue("BrandTokens.kt drift", tokens.contentEquals(File("src/main/kotlin/app/scrin/brand/BrandTokens.kt").readBytes()))
    }

    @Test fun accentOnSurfaceMeetsWcagAaInBothModes() {
        Tokens.ACCENTS.forEach { a ->
            listOf(false, true).forEach { dark ->
                val p = Tokens.palette(a, dark)
                assertTrue("${a.id} dark=$dark fg/bg", contrast(p.fg, p.bg) >= 4.5)
                assertTrue("${a.id} dark=$dark accentFg/accent", contrast(p.accentFg, p.accent) >= 4.5)
                assertTrue("${a.id} dark=$dark muted/bg", contrast(p.muted, p.bg) >= 4.5)
            }
        }
    }

    private fun lum(c: Int): Double {
        fun ch(v: Int): Double = (v / 255.0).let { if (it <= 0.03928) it / 12.92 else ((it + 0.055) / 1.055).pow(2.4) }
        return 0.2126 * ch((c shr 16) and 0xFF) + 0.7152 * ch((c shr 8) and 0xFF) + 0.0722 * ch(c and 0xFF)
    }

    private fun contrast(a: Int, b: Int): Double {
        val (x, y) = lum(a) to lum(b)
        return (maxOf(x, y) + 0.05) / (minOf(x, y) + 0.05)
    }
}
