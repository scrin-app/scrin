package ro.dragoscatalin.scrin.ui

import ro.dragoscatalin.scrin.ffi.MouseButtonKind
import ro.dragoscatalin.scrin.ffi.RemoteInput

enum class TouchMode { TRACKPAD, DIRECT }

/**
 * Turns local gestures on the video rectangle into [RemoteInput] mouse events, which both
 * a Windows host (SendInput) and an Android host (gestures) understand.
 *
 * - DIRECT: the finger is the pointer. Tap = left click at that point, drag = left-button
 *   drag, long-press without moving = right click.
 * - TRACKPAD: the finger moves a remote cursor relatively. Tap = left click, long-press =
 *   right click, two-finger tap = right click, two-finger vertical drag = wheel.
 *
 * Pure: the UI feeds positions in pixels of the video rect and calls [longPress] from a timer.
 */
class TouchMapper(var mode: TouchMode = TouchMode.TRACKPAD, private val sensitivity: Float = 1.6f, private val slopPx: Float = 12f) {
    /** Remote cursor in normalised [0,1] coordinates. */
    var cursorX = 0.5f
        private set
    var cursorY = 0.5f
        private set

    private enum class Phase { IDLE, PENDING, DRAGGING, CONSUMED }

    private var phase = Phase.IDLE
    private var downX = 0f
    private var downY = 0f
    private var travelled = 0f

    /** Whether a long-press now would still be a right click (UI timer check). */
    val longPressPending: Boolean get() = phase == Phase.PENDING

    fun down(x: Float, y: Float, w: Float, h: Float): List<RemoteInput> {
        phase = Phase.PENDING
        downX = x
        downY = y
        travelled = 0f
        return when (mode) {
            TouchMode.DIRECT -> moveTo(norm(x, w), norm(y, h))
            TouchMode.TRACKPAD -> emptyList()
        }
    }

    fun move(x: Float, y: Float, dx: Float, dy: Float, w: Float, h: Float): List<RemoteInput> {
        if (phase == Phase.IDLE || w <= 0f || h <= 0f) return emptyList()
        travelled += kotlin.math.abs(dx) + kotlin.math.abs(dy)
        val out = ArrayList<RemoteInput>(3)
        when (mode) {
            TouchMode.DIRECT -> {
                if (phase == Phase.CONSUMED) return emptyList()
                if (phase == Phase.PENDING && kotlin.math.hypot(x - downX, y - downY) > slopPx) {
                    phase = Phase.DRAGGING
                    out += RemoteInput.MouseButton(MouseButtonKind.LEFT, true)
                }
                if (phase == Phase.DRAGGING) out += moveTo(norm(x, w), norm(y, h))
            }
            TouchMode.TRACKPAD -> {
                if (phase == Phase.PENDING && travelled > slopPx) phase = Phase.DRAGGING
                cursorX = (cursorX + dx / w * sensitivity).coerceIn(0f, 1f)
                cursorY = (cursorY + dy / h * sensitivity).coerceIn(0f, 1f)
                out += RemoteInput.MouseMove(cursorX, cursorY)
            }
        }
        return out
    }

    fun up(x: Float, y: Float, w: Float, h: Float): List<RemoteInput> {
        val was = phase
        phase = Phase.IDLE
        return when (was) {
            Phase.PENDING -> when (mode) {
                TouchMode.DIRECT -> moveTo(norm(x, w), norm(y, h)) + click(MouseButtonKind.LEFT)
                TouchMode.TRACKPAD -> click(MouseButtonKind.LEFT)
            }
            Phase.DRAGGING -> if (mode == TouchMode.DIRECT) {
                moveTo(norm(x, w), norm(y, h)) + RemoteInput.MouseButton(MouseButtonKind.LEFT, false)
            } else {
                emptyList()
            }
            Phase.IDLE, Phase.CONSUMED -> emptyList()
        }
    }

    /** The finger rested without moving: right click (both modes). */
    fun longPress(): List<RemoteInput> {
        if (phase != Phase.PENDING) return emptyList()
        phase = Phase.CONSUMED
        return click(MouseButtonKind.RIGHT)
    }

    /** A second finger landed: the gesture becomes a scroll / two-finger tap. */
    fun cancel(): List<RemoteInput> {
        val wasDragging = phase == Phase.DRAGGING && mode == TouchMode.DIRECT
        phase = Phase.CONSUMED
        return if (wasDragging) listOf(RemoteInput.MouseButton(MouseButtonKind.LEFT, false)) else emptyList()
    }

    fun twoFingerTap(): List<RemoteInput> = click(MouseButtonKind.RIGHT)

    /** Two-finger vertical drag in pixels → wheel units (120 = one notch per ~40 px). */
    fun scroll(dyPx: Float): List<RemoteInput> {
        val units = (dyPx * 3f).toInt()
        return if (units == 0) emptyList() else listOf(RemoteInput.Wheel(0, units))
    }

    private fun moveTo(nx: Float, ny: Float): List<RemoteInput> {
        cursorX = nx
        cursorY = ny
        return listOf(RemoteInput.MouseMove(nx, ny))
    }

    private fun click(b: MouseButtonKind) = listOf(RemoteInput.MouseButton(b, true), RemoteInput.MouseButton(b, false))

    private fun norm(v: Float, size: Float) = if (size <= 0f) 0f else (v / size).coerceIn(0f, 1f)
}

/** USB HID usages and modifier combos for the special-keys bar. */
object Keys {
    const val ENTER = 0x28u
    const val ESC = 0x29u
    const val BACKSPACE = 0x2Au
    const val TAB = 0x2Bu
    const val DELETE = 0x4Cu
    const val RIGHT = 0x4Fu
    const val LEFT = 0x50u
    const val DOWN = 0x51u
    const val UP = 0x52u
    const val CTRL = 0xE0u
    const val SHIFT = 0xE1u
    const val ALT = 0xE2u
    const val WIN = 0xE3u

    /** `scrin.v1.KeyEvent.modifiers` bits. */
    const val MOD_SHIFT = 1u
    const val MOD_CTRL = 2u
    const val MOD_ALT = 4u
    const val MOD_META = 8u

    private val MOD_KEYS = listOf(MOD_CTRL to CTRL, MOD_ALT to ALT, MOD_META to WIN, MOD_SHIFT to SHIFT)

    /** Press [usage] with the held [mods]: modifiers down, key down/up, modifiers up (reverse). */
    fun press(usage: UInt, mods: UInt = 0u): List<RemoteInput> {
        val held = MOD_KEYS.filter { (bit, _) -> mods and bit != 0u }.map { it.second }
        val out = ArrayList<RemoteInput>(held.size * 2 + 2)
        var m = 0u
        held.forEach { k ->
            m = m or modBit(k)
            out += RemoteInput.Key(k, true, m)
        }
        out += RemoteInput.Key(usage, true, mods)
        out += RemoteInput.Key(usage, false, mods)
        held.asReversed().forEach { k ->
            m = m and modBit(k).inv()
            out += RemoteInput.Key(k, false, m)
        }
        return out
    }

    /** A modifier tapped alone (e.g. Win opens the Start menu). */
    fun tapModifier(usage: UInt): List<RemoteInput> = listOf(RemoteInput.Key(usage, true, modBit(usage)), RemoteInput.Key(usage, false, 0u))

    fun ctrlAltDel(): List<RemoteInput> = press(DELETE, MOD_CTRL or MOD_ALT)

    private fun modBit(k: UInt): UInt = MOD_KEYS.firstOrNull { it.second == k }?.first ?: 0u
}
