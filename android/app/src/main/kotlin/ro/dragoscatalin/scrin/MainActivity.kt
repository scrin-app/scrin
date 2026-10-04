package ro.dragoscatalin.scrin

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.core.splashscreen.SplashScreen.Companion.installSplashScreen
import ro.dragoscatalin.scrin.core.AdvancedProtection
import ro.dragoscatalin.scrin.data.Settings
import ro.dragoscatalin.scrin.ui.ScrinRoot
import ro.dragoscatalin.scrin.ui.theme.ScrinTheme

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        installSplashScreen()
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        // Debug builds: `am start ... --ez scrin.simulate_aapm true` previews the A-009 fallback.
        if (BuildConfig.DEBUG && intent?.hasExtra(AdvancedProtection.EXTRA_SIMULATE) == true) {
            AdvancedProtection.simulated = intent.getBooleanExtra(AdvancedProtection.EXTRA_SIMULATE, false)
        }
        val app = application as ScrinApp
        setContent {
            val settings by app.settings.settings.collectAsState(initial = Settings())
            ScrinTheme(settings.theme, settings.accent, settings.dynamicColor) {
                ScrinRoot(app, settings)
            }
        }
    }
}
