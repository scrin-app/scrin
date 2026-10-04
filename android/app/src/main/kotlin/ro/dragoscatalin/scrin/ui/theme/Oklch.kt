package ro.dragoscatalin.scrin.ui.theme

import kotlin.math.PI
import kotlin.math.cos
import kotlin.math.pow
import kotlin.math.sin

/** Converts OKLCH (as written in packages/ui/src/theme.css) to an opaque sRGB ARGB int. */
object Oklch {
    fun toArgb(l: Double, c: Double, hDeg: Double): Int {
        val h = hDeg * PI / 180.0
        val a = c * cos(h)
        val b = c * sin(h)
        val l1 = (l + 0.3963377774 * a + 0.2158037573 * b).pow(3)
        val m1 = (l - 0.1055613458 * a - 0.0638541728 * b).pow(3)
        val s1 = (l - 0.0894841775 * a - 1.2914855480 * b).pow(3)
        val r = 4.0767416621 * l1 - 3.3077115913 * m1 + 0.2309699292 * s1
        val g = -1.2684380046 * l1 + 2.6097574011 * m1 - 0.3413193965 * s1
        val bl = -0.0041960863 * l1 - 0.7034186147 * m1 + 1.7076147010 * s1
        return (0xFF shl 24) or (channel(r) shl 16) or (channel(g) shl 8) or channel(bl)
    }

    private fun channel(linear: Double): Int {
        val x = linear.coerceIn(0.0, 1.0)
        val srgb = if (x <= 0.0031308) 12.92 * x else 1.055 * x.pow(1.0 / 2.4) - 0.055
        return (srgb * 255.0 + 0.5).toInt().coerceIn(0, 255)
    }
}
