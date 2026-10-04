package ro.dragoscatalin.scrin.ui

import ro.dragoscatalin.scrin.ffi.MouseButtonKind
import ro.dragoscatalin.scrin.ffi.RemoteInput
import ro.dragoscatalin.scrin.ffi.TouchPhase

enum class TouchMode { TRACKPAD, DIRECT }

/**
 * Turns local gestures on the viewer into [RemoteInput].
 *
 * - DIRECT: the finger is the remote finger (absolute, normalised to the video rect).
 * - TRACKPAD: the finger moves a remote cursor relatively; tap = left click,
 *   two-finger tap = right click, two-finger drag = wheel.
 */
class TouchMapper(var mode: TouchMode = TouchMode.TRACKPAD, private val sensitivity: Float = 1.6f) {
    /** Remote cursor in normalised [0,1] coordinates (trackpad mode). */
    var cursorX = 0.5f
        private set
    var cursorY = 0.5f
        private set

    fun down(id: Int, x: Float, y: Float, w: Float, h: Float): List<RemoteInput> = when (mode) {
        TouchMode.DIRECT -> listOf(RemoteInput.Touch(id.toUInt(), TouchPhase.DOWN, norm(x, w), norm(y, h)))
        TouchMode.TRACKPAD -> emptyList()
    }

    fun move(id: Int, x: Float, y: Float, dx: Float, dy: Float, w: Float, h: Float): List<RemoteInput> = when (mode) {
        TouchMode.DIRECT -> listOf(RemoteInput.Touch(id.toUInt(), TouchPhase.MOVE, norm(x, w), norm(y, h)))
        TouchMode.TRACKPAD -> {
            if (w <= 0f || h <= 0f) {
                emptyList()
            } else {
                cursorX = (cursorX + dx / w * sensitivity).coerceIn(0f, 1f)
                cursorY = (cursorY + dy / h * sensitivity).coerceIn(0f, 1f)
                listOf(RemoteInput.MouseMove(cursorX, cursorY))
            }
        }
    }

    fun up(id: Int, x: Float, y: Float, w: Float, h: Float): List<RemoteInput> = when (mode) {
        TouchMode.DIRECT -> listOf(RemoteInput.Touch(id.toUInt(), TouchPhase.UP, norm(x, w), norm(y, h)))
        TouchMode.TRACKPAD -> emptyList()
    }

    fun tap(): List<RemoteInput> = when (mode) {
        TouchMode.TRACKPAD -> click(MouseButtonKind.LEFT)
        TouchMode.DIRECT -> emptyList()
    }

    fun twoFingerTap(): List<RemoteInput> = click(MouseButtonKind.RIGHT)

    /** Two-finger vertical drag in pixels → wheel notches (120 per ~40 px). */
    fun scroll(dyPx: Float): List<RemoteInput> {
        val units = (-dyPx * 3f).toInt()
        return if (units == 0) emptyList() else listOf(RemoteInput.Wheel(0, units))
    }

    private fun click(b: MouseButtonKind) = listOf(RemoteInput.MouseButton(b, true), RemoteInput.MouseButton(b, false))

    private fun norm(v: Float, size: Float) = if (size <= 0f) 0f else (v / size).coerceIn(0f, 1f)
}
