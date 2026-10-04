package ro.dragoscatalin.scrin

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import ro.dragoscatalin.scrin.ffi.MouseButtonKind
import ro.dragoscatalin.scrin.ffi.RemoteInput
import ro.dragoscatalin.scrin.ui.Keys
import ro.dragoscatalin.scrin.ui.TouchMapper
import ro.dragoscatalin.scrin.ui.TouchMode

class TouchMapperTest {
    private val leftClick = listOf(RemoteInput.MouseButton(MouseButtonKind.LEFT, true), RemoteInput.MouseButton(MouseButtonKind.LEFT, false))
    private val rightClick = listOf(RemoteInput.MouseButton(MouseButtonKind.RIGHT, true), RemoteInput.MouseButton(MouseButtonKind.RIGHT, false))

    @Test fun directTapClicksAtTheFinger() {
        val m = TouchMapper(TouchMode.DIRECT)
        assertEquals(listOf(RemoteInput.MouseMove(0.5f, 0.25f)), m.down(50f, 25f, 100f, 100f))
        assertEquals(listOf(RemoteInput.MouseMove(0.5f, 0.25f)) + leftClick, m.up(50f, 25f, 100f, 100f))
    }

    @Test fun directDragHoldsTheLeftButton() {
        val m = TouchMapper(TouchMode.DIRECT, slopPx = 5f)
        m.down(10f, 10f, 100f, 100f)
        assertTrue(m.move(12f, 10f, 2f, 0f, 100f, 100f).isEmpty())
        assertEquals(
            listOf(RemoteInput.MouseButton(MouseButtonKind.LEFT, true), RemoteInput.MouseMove(0.3f, 0.1f)),
            m.move(30f, 10f, 18f, 0f, 100f, 100f),
        )
        assertEquals(
            listOf(RemoteInput.MouseMove(1f, 0f), RemoteInput.MouseButton(MouseButtonKind.LEFT, false)),
            m.up(150f, -3f, 100f, 100f),
        )
    }

    @Test fun longPressIsARightClickAndSwallowsTheUp() {
        val m = TouchMapper(TouchMode.DIRECT)
        m.down(40f, 40f, 100f, 100f)
        assertTrue(m.longPressPending)
        assertEquals(rightClick, m.longPress())
        assertTrue(m.up(40f, 40f, 100f, 100f).isEmpty())
        assertTrue(m.longPress().isEmpty())
    }

    @Test fun trackpadMovesARelativeCursorAndTapClicks() {
        val m = TouchMapper(TouchMode.TRACKPAD, sensitivity = 1f)
        assertTrue(m.down(0f, 0f, 100f, 100f).isEmpty())
        assertEquals(listOf(RemoteInput.MouseMove(0.6f, 0.5f)), m.move(10f, 0f, 10f, 0f, 100f, 100f))
        m.move(0f, 0f, 1000f, 1000f, 100f, 100f)
        assertEquals(1f, m.cursorX, 0f)
        assertEquals(1f, m.cursorY, 0f)
        assertTrue("moved: no click", m.up(0f, 0f, 100f, 100f).isEmpty())
        m.down(0f, 0f, 100f, 100f)
        assertEquals(leftClick, m.up(0f, 0f, 100f, 100f))
        assertEquals(rightClick, m.twoFingerTap())
    }

    @Test fun secondFingerCancelsADragAndScrollMapsToWheel() {
        val m = TouchMapper(TouchMode.DIRECT, slopPx = 1f)
        m.down(0f, 0f, 100f, 100f)
        m.move(10f, 0f, 10f, 0f, 100f, 100f)
        assertEquals(listOf(RemoteInput.MouseButton(MouseButtonKind.LEFT, false)), m.cancel())
        assertTrue(m.up(10f, 0f, 100f, 100f).isEmpty())
        // Finger down → content follows (natural scrolling) → positive wheel = up.
        assertEquals(listOf(RemoteInput.Wheel(0, 120)), m.scroll(40f))
        assertTrue(m.scroll(0.1f).isEmpty())
    }

    @Test fun ctrlAltDelPressesAndReleasesInOrder() {
        val ev = Keys.ctrlAltDel().map { (it as RemoteInput.Key).let { k -> Triple(k.hidUsage, k.down, k.modifiers) } }
        assertEquals(
            listOf(
                Triple(Keys.CTRL, true, 2u),
                Triple(Keys.ALT, true, 6u),
                Triple(Keys.DELETE, true, 6u),
                Triple(Keys.DELETE, false, 6u),
                Triple(Keys.ALT, false, 2u),
                Triple(Keys.CTRL, false, 0u),
            ),
            ev,
        )
        assertEquals(listOf(RemoteInput.Key(Keys.ESC, true, 0u), RemoteInput.Key(Keys.ESC, false, 0u)), Keys.press(Keys.ESC))
        assertEquals(RemoteInput.Key(Keys.WIN, true, 8u), Keys.tapModifier(Keys.WIN).first())
    }
}
