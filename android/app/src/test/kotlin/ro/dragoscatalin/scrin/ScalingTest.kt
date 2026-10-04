package ro.dragoscatalin.scrin

import org.junit.Assert.assertEquals
import org.junit.Test
import ro.dragoscatalin.scrin.media.fitSize

class ScalingTest {
    @Test fun capsAtFullHdPreservingAspect() {
        // Galaxy A51 portrait 1080x2400 → short edge already 1080, long edge capped at 1920.
        assertEquals(864 to 1920, fitSize(1080, 2400))
        assertEquals(1920 to 864, fitSize(2400, 1080))
        assertEquals(1920 to 1088, fitSize(3840, 2160))
        assertEquals(1280 to 720, fitSize(1280, 720))
    }

    @Test fun stepsDownUntilTheEncoderAcceptsTheSize() {
        val (w, h) = fitSize(1080, 2400) { cw, ch -> cw * ch <= 1280 * 720 }
        assertEquals(0, w % 16)
        assertEquals(0, h % 16)
        assert(w * h <= 1280 * 720)
        assert(kotlin.math.abs(h.toDouble() / w - 2400.0 / 1080) < 0.05)
    }
}
