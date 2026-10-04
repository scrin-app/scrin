package ro.dragoscatalin.scrin

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import ro.dragoscatalin.scrin.ffi.MouseButtonKind
import ro.dragoscatalin.scrin.ffi.RemoteInput
import ro.dragoscatalin.scrin.ffi.TouchPhase
import ro.dragoscatalin.scrin.ui.TouchMapper
import ro.dragoscatalin.scrin.ui.TouchMode

class TouchMapperTest {
    @Test fun directModeSendsNormalisedTouches() {
        val m = TouchMapper(TouchMode.DIRECT)
        assertEquals(listOf(RemoteInput.Touch(0u, TouchPhase.DOWN, 0.5f, 0.25f)), m.down(0, 50f, 25f, 100f, 100f))
        assertEquals(listOf(RemoteInput.Touch(0u, TouchPhase.MOVE, 1f, 0f)), m.move(0, 150f, -3f, 0f, 0f, 100f, 100f))
        assertEquals(listOf(RemoteInput.Touch(0u, TouchPhase.UP, 0.1f, 0.2f)), m.up(0, 10f, 20f, 100f, 100f))
        assertTrue(m.tap().isEmpty())
    }

    @Test fun trackpadMovesARelativeCursorAndClicks() {
        val m = TouchMapper(TouchMode.TRACKPAD, sensitivity = 1f)
        assertTrue(m.down(0, 0f, 0f, 100f, 100f).isEmpty())
        assertEquals(listOf(RemoteInput.MouseMove(0.6f, 0.5f)), m.move(0, 0f, 0f, 10f, 0f, 100f, 100f))
        m.move(0, 0f, 0f, 1000f, 1000f, 100f, 100f)
        assertEquals(1f, m.cursorX, 0f)
        assertEquals(1f, m.cursorY, 0f)
        assertEquals(
            listOf(RemoteInput.MouseButton(MouseButtonKind.LEFT, true), RemoteInput.MouseButton(MouseButtonKind.LEFT, false)),
            m.tap(),
        )
        assertEquals(MouseButtonKind.RIGHT, (m.twoFingerTap().first() as RemoteInput.MouseButton).button)
    }

    @Test fun scrollMapsPixelsToWheelUnits() {
        val m = TouchMapper()
        assertEquals(listOf(RemoteInput.Wheel(0, -120)), m.scroll(40f))
        assertTrue(m.scroll(0.1f).isEmpty())
    }
}
