package ro.dragoscatalin.scrin

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import ro.dragoscatalin.scrin.service.HidText
import ro.dragoscatalin.scrin.ui.VRect
import ro.dragoscatalin.scrin.ui.Zoom
import ro.dragoscatalin.scrin.ui.letterbox

class ViewportTest {
    private val eps = 0.01f

    @Test fun letterboxCentresAWidePictureInATallViewport() {
        val r = letterbox(1000f, 2000f, 2f)
        assertEquals(VRect(0f, 750f, 1000f, 500f), r)
    }

    @Test fun letterboxPillarboxesATallPictureInAWideViewport() {
        val r = letterbox(2000f, 1000f, 0.5f)
        assertEquals(VRect(750f, 0f, 500f, 1000f), r)
    }

    @Test fun pinchZoomKeepsThePointUnderTheFingersFixed() {
        val base = VRect(0f, 0f, 1000f, 500f)
        val z = Zoom().transformed(base, 250f, 125f, 2f, 0f, 0f)
        assertEquals(2f, z.scale, eps)
        val r = z.rect(base)
        // The picture point under (250,125) before is the same fraction after.
        assertEquals(0.25f, (250f - r.left) / r.width, eps)
        assertEquals(0.25f, (125f - r.top) / r.height, eps)
        assertTrue(z.zoomed)
    }

    @Test fun zoomIsClampedAndNeverLeavesGapsAtTheEdges() {
        val base = VRect(0f, 100f, 1000f, 500f)
        var z = Zoom().transformed(base, 500f, 350f, 100f, 0f, 0f)
        assertEquals(Zoom.MAX, z.scale, eps)
        z = z.transformed(base, 500f, 350f, 1f, 1e6f, 1e6f)
        val r = z.rect(base)
        assertEquals(base.left, r.left, eps)
        assertEquals(base.top, r.top, eps)
        z = z.transformed(base, 500f, 350f, 1f, -1e6f, -1e6f)
        val r2 = z.rect(base)
        assertEquals(base.right, r2.right, eps)
        assertEquals(base.bottom, r2.bottom, eps)
    }

    @Test fun zoomingOutBelowOneSnapsBackToTheBase() {
        val base = VRect(10f, 20f, 300f, 200f)
        val z = Zoom().transformed(base, 100f, 100f, 0.2f, 50f, 50f)
        assertEquals(1f, z.scale, eps)
        assertEquals(base, z.rect(base))
        assertFalse(z.zoomed)
    }

    @Test fun hidLettersDigitsAndPunctuationBecomeText() {
        assertEquals('a', HidText.char(0x04u, 0u))
        assertEquals('Z', HidText.char(0x1Du, 1u))
        assertEquals('1', HidText.char(0x1Eu, 0u))
        assertEquals('0', HidText.char(0x27u, 0u))
        assertEquals(')', HidText.char(0x27u, 1u))
        assertEquals(' ', HidText.char(0x2Cu, 0u))
        assertEquals('?', HidText.char(0x38u, 1u))
    }

    @Test fun hidShortcutsAndControlKeysAreNotText() {
        assertNull(HidText.char(0x06u, 2u)) // Ctrl+C
        assertNull(HidText.char(0x04u, 8u)) // Win+A
        assertNull(HidText.char(0x28u, 0u)) // Enter
        assertNull(HidText.char(0x29u, 0u)) // Esc
    }
}
