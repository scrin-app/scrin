package ro.dragoscatalin.scrin.ui

import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.navigation3.runtime.NavEntry
import androidx.navigation3.runtime.rememberNavBackStack
import androidx.navigation3.ui.NavDisplay
import ro.dragoscatalin.scrin.ScrinApp
import ro.dragoscatalin.scrin.core.Role
import ro.dragoscatalin.scrin.data.Settings
import ro.dragoscatalin.scrin.ffi.SessionState
import ro.dragoscatalin.scrin.ui.screens.AboutScreen
import ro.dragoscatalin.scrin.ui.screens.ConnectingScreen
import ro.dragoscatalin.scrin.ui.screens.HomeScreen
import ro.dragoscatalin.scrin.ui.screens.HostScreen
import ro.dragoscatalin.scrin.ui.screens.IncomingRequestDialog
import ro.dragoscatalin.scrin.ui.screens.RestrictedGuideScreen
import ro.dragoscatalin.scrin.ui.screens.SettingsScreen
import ro.dragoscatalin.scrin.ui.screens.TrustedDevicesScreen
import ro.dragoscatalin.scrin.ui.screens.ViewerScreen
import ro.dragoscatalin.scrin.ui.Settings as SettingsRoute

@Composable
fun ScrinRoot(app: ScrinApp, settings: Settings) {
    val hub = app.hub
    val ui by hub.ui.collectAsState()
    val backStack = rememberNavBackStack(Home)
    fun go(r: Route) { backStack.add(r) }
    fun back() { if (backStack.size > 1) backStack.removeAt(backStack.lastIndex) }

    // Controller: move to the viewer once the host accepts.
    LaunchedEffect(ui.role, ui.state) {
        if (ui.role == Role.CONTROLLER && ui.state == SessionState.ACTIVE && backStack.lastOrNull() == Connecting) {
            backStack.removeAt(backStack.lastIndex)
            backStack.add(Viewer)
        }
        if (ui.role == Role.HOST && ui.state == SessionState.PAIRING && backStack.lastOrNull() != Host) {
            backStack.add(Host)
        }
    }

    Surface(Modifier.fillMaxSize(), color = MaterialTheme.colorScheme.background) {
        NavDisplay(
            backStack = backStack,
            onBack = { back() },
            entryProvider = { key ->
                when (key) {
                    Home -> NavEntry(key) {
                        HomeScreen(hub, onConnect = { go(Connecting) }, onHost = { go(Host) }, onSettings = { go(SettingsRoute) })
                    }
                    Connecting -> NavEntry(key) { ConnectingScreen(hub, onBack = { hub.end(); hub.dismissEnded(); back() }) }
                    Viewer -> NavEntry(key) { ViewerScreen(hub, onClose = { hub.end(); hub.dismissEnded(); back() }) }
                    Host -> NavEntry(key) {
                        HostScreen(app, settings, onBack = { back() }, onRestrictedGuide = { go(RestrictedGuide) })
                    }
                    SettingsRoute -> NavEntry(key) {
                        SettingsScreen(
                            app, settings, onBack = { back() },
                            onTrusted = { go(TrustedDevices) }, onAbout = { go(About) }, onRestrictedGuide = { go(RestrictedGuide) },
                        )
                    }
                    TrustedDevices -> NavEntry(key) { TrustedDevicesScreen(hub, onBack = { back() }) }
                    About -> NavEntry(key) { AboutScreen(onBack = { back() }) }
                    RestrictedGuide -> NavEntry(key) { RestrictedGuideScreen(onBack = { back() }) }
                    else -> NavEntry(key) { HomeScreen(hub, onConnect = { go(Connecting) }, onHost = { go(Host) }, onSettings = { go(SettingsRoute) }) }
                }
            },
        )
        // The pre-accept interstitial shows over any screen (ADR-0009).
        ui.request?.let { IncomingRequestDialog(hub, it) }
    }
}
