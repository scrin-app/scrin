package ro.dragoscatalin.scrin.ui

/** An axis-aligned rectangle in pixels. */
data class VRect(val left: Float, val top: Float, val width: Float, val height: Float) {
    val right: Float get() = left + width
    val bottom: Float get() = top + height
}

/** The largest rectangle of `ratio` (w/h) centred in a `vw`×`vh` viewport. */
fun letterbox(vw: Float, vh: Float, ratio: Float): VRect {
    if (vw <= 0f || vh <= 0f || ratio <= 0f) return VRect(0f, 0f, vw.coerceAtLeast(0f), vh.coerceAtLeast(0f))
    val w = minOf(vw, vh * ratio)
    val h = w / ratio
    return VRect((vw - w) / 2f, (vh - h) / 2f, w, h)
}

/**
 * Pinch-zoom and pan of the remote picture, relative to its letterboxed [VRect] `base`.
 * Immutable: every gesture step returns a new value. The zoomed picture always covers the
 * whole base rectangle (no gaps at the edges); scale 1 means no pan.
 */
data class Zoom(val scale: Float = 1f, val panX: Float = 0f, val panY: Float = 0f) {
    companion object {
        const val MAX = 5f
    }

    val zoomed: Boolean get() = scale > 1.01f

    fun rect(base: VRect) = VRect(base.left + panX, base.top + panY, base.width * scale, base.height * scale)

    /** Zoom by `factor` around the point (`cx`, `cy`) and move by (`dx`, `dy`), all in viewport pixels. */
    fun transformed(base: VRect, cx: Float, cy: Float, factor: Float, dx: Float, dy: Float): Zoom {
        if (base.width <= 0f || base.height <= 0f) return this
        val old = rect(base)
        val s = (scale * factor).coerceIn(1f, MAX)
        val k = s / scale
        val w = base.width * s
        val h = base.height * s
        val left = (cx - (cx - old.left) * k + dx).coerceIn(base.right - w, base.left)
        val top = (cy - (cy - old.top) * k + dy).coerceIn(base.bottom - h, base.top)
        return Zoom(s, left - base.left, top - base.top)
    }
}
