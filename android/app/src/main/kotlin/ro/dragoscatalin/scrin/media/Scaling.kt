package ro.dragoscatalin.scrin.media

import kotlin.math.max
import kotlin.math.min
import kotlin.math.roundToInt

/**
 * Encoder output size for a `w`×`h` screen: at most 1080p (long edge ≤ [maxLong], short
 * edge ≤ [maxShort]), aspect preserved, both edges multiples of 16 (what every hardware
 * AVC encoder accepts). Steps down 10 % at a time until [supported] accepts the size.
 */
fun fitSize(w: Int, h: Int, maxLong: Int = 1920, maxShort: Int = 1080, supported: (Int, Int) -> Boolean = { _, _ -> true }): Pair<Int, Int> {
    require(w > 0 && h > 0)
    var scale = min(1.0, min(maxLong.toDouble() / max(w, h), maxShort.toDouble() / min(w, h)))
    while (true) {
        val cw = align16(w * scale)
        val ch = align16(h * scale)
        if (supported(cw, ch) || cw <= 16 || ch <= 16) return cw to ch
        scale *= 0.9
    }
}

private fun align16(v: Double): Int = max(16, (v / 16).roundToInt() * 16)
