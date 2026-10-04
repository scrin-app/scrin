package ro.dragoscatalin.scrin.debug

import android.accessibilityservice.GestureDescription
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.graphics.Path
import android.util.Log
import ro.dragoscatalin.scrin.service.RemoteInputService

/**
 * Debug builds only (src/debug): lets a device test run a two-finger pinch through the app's
 * own accessibility service, since `adb shell input` cannot inject multi-touch without root.
 *
 * `adb shell am broadcast -n ro.dragoscatalin.scrin/.debug.DebugGestureReceiver
 *   --ei cx 540 --ei cy 1250 --ei from 80 --ei to 400 --ei ms 500`
 * (`from` > `to` pinches in).
 */
class DebugGestureReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        val cx = intent.getIntExtra("cx", 540).toFloat()
        val cy = intent.getIntExtra("cy", 1200).toFloat()
        val from = intent.getIntExtra("from", 80).toFloat()
        val to = intent.getIntExtra("to", 400).toFloat()
        val ms = intent.getIntExtra("ms", 500).toLong()
        val left = Path().apply { moveTo(cx - from, cy); lineTo(cx - to, cy) }
        val right = Path().apply { moveTo(cx + from, cy); lineTo(cx + to, cy) }
        val g = GestureDescription.Builder()
            .addStroke(GestureDescription.StrokeDescription(left, 0, ms))
            .addStroke(GestureDescription.StrokeDescription(right, 0, ms))
            .build()
        Log.i("scrin.debug", "pinch $from->$to at ($cx,$cy): dispatched=${RemoteInputService.dispatchDebugGesture(g)}")
    }
}
