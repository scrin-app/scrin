package ro.dragoscatalin.scrin.service

import android.accessibilityservice.AccessibilityService
import android.accessibilityservice.GestureDescription
import android.content.ComponentName
import android.content.Context
import android.graphics.Path
import android.os.Bundle
import android.os.SystemClock
import android.provider.Settings
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
 * Declared WITHOUT `isAccessibilityTool` (scrin is not an assistive tool; INV-14).
 * Enabled by the user only after the prominent disclosure in the Host screen.
 */
class RemoteInputService : AccessibilityService() {
    companion object {
        @Volatile private var instance: RemoteInputService? = null

        /** HID usages of the keys mapped to global actions. */
        const val HID_ESCAPE = 0x29u
        const val HID_HOME = 0x4Au
        const val HID_APP_SWITCH = 0x65u

        fun isEnabled(ctx: Context): Boolean {
            val enabled = Settings.Secure.getString(ctx.contentResolver, Settings.Secure.ENABLED_ACCESSIBILITY_SERVICES) ?: return false
            val me = ComponentName(ctx, RemoteInputService::class.java).flattenToString()
            return enabled.split(':').any { it.equals(me, ignoreCase = true) }
        }

        fun dispatch(event: RemoteInput) {
            instance?.handle(event)
        }
    }

    private var downAt = 0L
    private var downX = 0f
    private var downY = 0f
    private val path = Path()
    private var cursorX = 0.5f
    private var cursorY = 0.5f

    override fun onServiceConnected() {
        instance = this
    }

    override fun onUnbind(intent: android.content.Intent?): Boolean {
        instance = null
        return super.onUnbind(intent)
    }

    override fun onAccessibilityEvent(event: AccessibilityEvent?) = Unit
    override fun onInterrupt() = Unit

    private fun px(nx: Float, ny: Float): Pair<Float, Float> {
        val m = resources.displayMetrics
        return nx.coerceIn(0f, 1f) * (m.widthPixels - 1) to ny.coerceIn(0f, 1f) * (m.heightPixels - 1)
    }

    private fun handle(e: RemoteInput) {
        when (e) {
            is RemoteInput.Touch -> touch(e)
            is RemoteInput.MouseMove -> { cursorX = e.x; cursorY = e.y }
            is RemoteInput.MouseButton -> if (!e.down) click(e.button)
            is RemoteInput.Wheel -> scroll(e.dy)
            is RemoteInput.Key -> if (e.down) key(e.hidUsage)
            is RemoteInput.Text -> setText(e.text)
        }
    }

    private fun touch(e: RemoteInput.Touch) {
        val (x, y) = px(e.x, e.y)
        when (e.phase) {
            TouchPhase.DOWN -> {
                downAt = SystemClock.uptimeMillis()
                downX = x
                downY = y
                path.reset()
                path.moveTo(x, y)
            }
            TouchPhase.MOVE -> path.lineTo(x, y)
            TouchPhase.UP -> {
                val dur = (SystemClock.uptimeMillis() - downAt).coerceIn(1, 10_000)
                val stroke = if (kotlin.math.hypot(x - downX, y - downY) < 12f) {
                    Path().apply { moveTo(downX, downY) }
                } else {
                    path.lineTo(x, y)
                    Path(path)
                }
                gesture(stroke, dur)
            }
            TouchPhase.CANCEL -> path.reset()
        }
    }

    private fun click(b: MouseButtonKind) {
        val (x, y) = px(cursorX, cursorY)
        when (b) {
            MouseButtonKind.LEFT -> gesture(Path().apply { moveTo(x, y) }, 40)
            // Right click = long-press, the closest Android equivalent.
            MouseButtonKind.RIGHT -> gesture(Path().apply { moveTo(x, y) }, 700)
            MouseButtonKind.BACK -> performGlobalAction(GLOBAL_ACTION_BACK)
            MouseButtonKind.MIDDLE, MouseButtonKind.FORWARD -> Unit
        }
    }

    private fun scroll(dy: Int) {
        if (dy == 0) return
        val (x, y) = px(cursorX, cursorY)
        val dist = (dy / 120f) * resources.displayMetrics.heightPixels * 0.15f
        val p = Path().apply { moveTo(x, y); lineTo(x, (y + dist).coerceIn(0f, resources.displayMetrics.heightPixels - 1f)) }
        gesture(p, 200)
    }

    private fun key(hid: UInt) {
        when (hid) {
            HID_ESCAPE -> performGlobalAction(GLOBAL_ACTION_BACK)
            HID_HOME -> performGlobalAction(GLOBAL_ACTION_HOME)
            HID_APP_SWITCH -> performGlobalAction(GLOBAL_ACTION_RECENTS)
        }
    }

    private fun setText(text: String) {
        val node = rootInActiveWindow?.findFocus(AccessibilityNodeInfo.FOCUS_INPUT) ?: return
        val current = node.text?.toString().orEmpty()
        val args = Bundle().apply {
            putCharSequence(AccessibilityNodeInfo.ACTION_ARGUMENT_SET_TEXT_CHARSEQUENCE, current + text)
        }
        node.performAction(AccessibilityNodeInfo.ACTION_SET_TEXT, args)
    }

    private fun gesture(p: Path, durationMs: Long) {
        val stroke = GestureDescription.StrokeDescription(p, 0, durationMs)
        dispatchGesture(GestureDescription.Builder().addStroke(stroke).build(), null, null)
    }
}
