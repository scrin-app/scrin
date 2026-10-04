package ro.dragoscatalin.scrin.service

import android.accessibilityservice.AccessibilityService
import android.accessibilityservice.GestureDescription
import android.content.ComponentName
import android.content.Context
import android.graphics.Path
import android.hardware.display.DisplayManager
import android.os.Bundle
import android.os.SystemClock
import android.provider.Settings
import android.util.DisplayMetrics
import android.util.Log
import android.view.Display
import android.view.accessibility.AccessibilityEvent
import android.view.accessibility.AccessibilityNodeInfo
import ro.dragoscatalin.scrin.ffi.MouseButtonKind
import ro.dragoscatalin.scrin.ffi.RemoteInput
import ro.dragoscatalin.scrin.ffi.TouchPhase

/**
 * Remote input on an attended Android host: taps, swipes and long-presses through
 * `dispatchGesture`, Back/Home/Recents through global actions, text through
 * `ACTION_SET_TEXT` on the focused field.
 *
 * Accepts both controller styles: touch points (an Android controller in touch mode) and
 * mouse events (a Windows controller or trackpad mode) — left button down → move → up is
 * a drag, a short press is a tap, the right button is a long-press.
 *
 * Declared WITHOUT `isAccessibilityTool` (scrin is not an assistive tool; INV-14).
 * Enabled by the user only after the prominent disclosure in the Host screen.
 */
class RemoteInputService : AccessibilityService() {
    companion object {
        private const val TAG = "scrin.input"
        @Volatile private var instance: RemoteInputService? = null

        /** HID usages with an Android meaning. */
        const val HID_ENTER = 0x28u
        const val HID_ESCAPE = 0x29u
        const val HID_BACKSPACE = 0x2Au
        const val HID_HOME = 0x4Au
        const val HID_APP_SWITCH = 0x65u
        const val HID_WIN_LEFT = 0xE3u

        private const val TAP_SLOP_PX = 12f
        private const val TAP_MS = 40L
        private const val LONG_PRESS_MS = 700L
        private const val MAX_GESTURE_MS = 10_000L

        fun isEnabled(ctx: Context): Boolean {
            val enabled = Settings.Secure.getString(ctx.contentResolver, Settings.Secure.ENABLED_ACCESSIBILITY_SERVICES) ?: return false
            val me = ComponentName(ctx, RemoteInputService::class.java).flattenToString()
            return enabled.split(':').any { it.equals(me, ignoreCase = true) }
        }

        /** `true` when the event was handed to the running service. */
        fun dispatch(event: RemoteInput): Boolean {
            val s = instance ?: return false
            s.handle(event)
            return true
        }
    }

    /** One finger/button stroke being recorded until its release. */
    private class Stroke(val x: Float, val y: Float, val at: Long) {
        val path = Path().apply { moveTo(x, y) }
        var lastX = x
        var lastY = y
        var moved = false
    }

    private var stroke: Stroke? = null
    private var cursorX = 0.5f
    private var cursorY = 0.5f

    override fun onServiceConnected() {
        instance = this
        Log.i(TAG, "remote input service connected")
    }

    override fun onUnbind(intent: android.content.Intent?): Boolean {
        instance = null
        return super.onUnbind(intent)
    }

    override fun onAccessibilityEvent(event: AccessibilityEvent?) = Unit
    override fun onInterrupt() = Unit

    /** Full panel size (the projection the controller sees includes the system bars). */
    @Suppress("DEPRECATION") // getRealMetrics; WindowMetrics needs a visual context.
    private fun screen(): DisplayMetrics {
        val m = DisplayMetrics()
        getSystemService(DisplayManager::class.java).getDisplay(Display.DEFAULT_DISPLAY)?.getRealMetrics(m)
        return if (m.widthPixels > 0) m else resources.displayMetrics
    }

    private fun px(nx: Float, ny: Float): Pair<Float, Float> {
        val m = screen()
        return nx.coerceIn(0f, 1f) * (m.widthPixels - 1) to ny.coerceIn(0f, 1f) * (m.heightPixels - 1)
    }

    private fun handle(e: RemoteInput) {
        when (e) {
            is RemoteInput.Touch -> touch(e)
            is RemoteInput.MouseMove -> {
                cursorX = e.x
                cursorY = e.y
                stroke?.let { s -> px(e.x, e.y).let { (x, y) -> extend(s, x, y) } }
            }
            is RemoteInput.MouseButton -> button(e.button, e.down)
            is RemoteInput.Wheel -> scroll(e.dy)
            is RemoteInput.Key -> if (e.down) key(e.hidUsage)
            is RemoteInput.Text -> setText(e.text)
        }
    }

    private fun extend(s: Stroke, x: Float, y: Float) {
        if (!s.moved && kotlin.math.hypot(x - s.x, y - s.y) < TAP_SLOP_PX) return
        s.moved = true
        s.path.lineTo(x, y)
        s.lastX = x
        s.lastY = y
    }

    private fun begin(x: Float, y: Float) {
        stroke = Stroke(x, y, SystemClock.uptimeMillis())
    }

    /** Ends the stroke: a tap (or long-press when held) if it never moved, else a drag. */
    private fun end(x: Float, y: Float, longPress: Boolean = false) {
        val s = stroke ?: return
        stroke = null
        extend(s, x, y)
        val held = (SystemClock.uptimeMillis() - s.at).coerceIn(1, MAX_GESTURE_MS)
        if (s.moved) {
            gesture(s.path, held.coerceAtLeast(50), "drag (${s.x.toInt()},${s.y.toInt()})->(${s.lastX.toInt()},${s.lastY.toInt()})")
        } else {
            val dur = if (longPress) LONG_PRESS_MS else held.coerceAtMost(LONG_PRESS_MS - 100).coerceAtLeast(TAP_MS)
            gesture(Path().apply { moveTo(s.x, s.y) }, dur, "${if (longPress) "long-press" else "tap"} (${s.x.toInt()},${s.y.toInt()})")
        }
    }

    private fun touch(e: RemoteInput.Touch) {
        val (x, y) = px(e.x, e.y)
        when (e.phase) {
            TouchPhase.DOWN -> begin(x, y)
            TouchPhase.MOVE -> stroke?.let { extend(it, x, y) }
            TouchPhase.UP -> end(x, y)
            TouchPhase.CANCEL -> stroke = null
        }
    }

    private fun button(b: MouseButtonKind, down: Boolean) {
        val (x, y) = px(cursorX, cursorY)
        when (b) {
            MouseButtonKind.LEFT -> if (down) begin(x, y) else end(x, y)
            // Right click = long-press, the closest Android equivalent.
            MouseButtonKind.RIGHT -> if (down) begin(x, y) else end(x, y, longPress = true)
            MouseButtonKind.BACK -> if (down) performGlobalAction(GLOBAL_ACTION_BACK)
            MouseButtonKind.MIDDLE, MouseButtonKind.FORWARD -> Unit
        }
    }

    /** Wheel notches → a swipe: wheel up (positive) scrolls content up = finger moves down. */
    private fun scroll(dy: Int) {
        if (dy == 0) return
        val m = screen()
        val (x, y) = px(cursorX, cursorY)
        val dist = (dy / 120f) * m.heightPixels * 0.15f
        val p = Path().apply { moveTo(x, y); lineTo(x, (y + dist).coerceIn(0f, m.heightPixels - 1f)) }
        gesture(p, 200, "scroll $dy")
    }

    private fun key(hid: UInt) {
        val action = when (hid) {
            HID_ESCAPE -> GLOBAL_ACTION_BACK
            HID_HOME, HID_WIN_LEFT -> GLOBAL_ACTION_HOME
            HID_APP_SWITCH -> GLOBAL_ACTION_RECENTS
            HID_BACKSPACE -> return editText { it.dropLast(1) }
            HID_ENTER -> return imeEnter()
            else -> return
        }
        Log.i(TAG, "global action $action for HID 0x${hid.toString(16)}")
        performGlobalAction(action)
    }

    private fun focused(): AccessibilityNodeInfo? = rootInActiveWindow?.findFocus(AccessibilityNodeInfo.FOCUS_INPUT)

    private fun setText(text: String) = editText { it + text }

    private fun editText(change: (String) -> String) {
        val node = focused() ?: return
        val args = Bundle().apply {
            putCharSequence(AccessibilityNodeInfo.ACTION_ARGUMENT_SET_TEXT_CHARSEQUENCE, change(node.text?.toString().orEmpty()))
        }
        node.performAction(AccessibilityNodeInfo.ACTION_SET_TEXT, args)
    }

    private fun imeEnter() {
        if (android.os.Build.VERSION.SDK_INT < android.os.Build.VERSION_CODES.R) return
        val node = focused() ?: return
        node.performAction(AccessibilityNodeInfo.AccessibilityAction.ACTION_IME_ENTER.id)
    }

    /** `what` may carry coordinates (a PIN pad position): logged only in debug builds. */
    private fun gesture(p: Path, durationMs: Long, detail: String) {
        val what = if (ro.dragoscatalin.scrin.BuildConfig.DEBUG) detail else detail.substringBefore(' ')
        val stroke = GestureDescription.StrokeDescription(p, 0, durationMs)
        val ok = dispatchGesture(
            GestureDescription.Builder().addStroke(stroke).build(),
            object : GestureResultCallback() {
                override fun onCompleted(gestureDescription: GestureDescription?) {
                    Log.i(TAG, "gesture completed: $what")
                }
                override fun onCancelled(gestureDescription: GestureDescription?) {
                    Log.i(TAG, "gesture cancelled: $what")
                }
            },
            null,
        )
        if (!ok) Log.w(TAG, "gesture rejected: $what")
    }
}
