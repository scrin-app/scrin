package ro.dragoscatalin.scrin.core

import android.content.Context
import android.os.Build
import android.security.advancedprotection.AdvancedProtectionManager
import ro.dragoscatalin.scrin.BuildConfig

/**
 * Android 16+ Advanced Protection Mode (AAPM) turns off accessibility services that are not
 * declared assistive tools — scrin's remote-control service included (it never claims
 * `isAccessibilityTool`, INV-14). With AAPM on, the host falls back to view-only sharing.
 */
object AdvancedProtection {
    /** Intent extra (debug builds only) that simulates AAPM on devices older than Android 16. */
    const val EXTRA_SIMULATE = "scrin.simulate_aapm"

    @Volatile var simulated: Boolean = false

    fun enabled(ctx: Context): Boolean {
        if (BuildConfig.DEBUG && simulated) return true
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.BAKLAVA) return false
        return runCatching {
            ctx.getSystemService(AdvancedProtectionManager::class.java)?.isAdvancedProtectionEnabled == true
        }.getOrDefault(false)
    }
}
