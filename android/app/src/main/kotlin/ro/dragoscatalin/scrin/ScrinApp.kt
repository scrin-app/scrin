package ro.dragoscatalin.scrin

import android.app.Application
import android.os.Build
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.runBlocking
import ro.dragoscatalin.scrin.core.NativeCore
import ro.dragoscatalin.scrin.core.SeedVault
import ro.dragoscatalin.scrin.core.SessionHub
import ro.dragoscatalin.scrin.data.RelayUrls
import ro.dragoscatalin.scrin.data.ServerUrl
import ro.dragoscatalin.scrin.data.SettingsRepository
import ro.dragoscatalin.scrin.ffi.CoreConfig
import ro.dragoscatalin.scrin.ffi.ScrinCore
import java.io.File

class ScrinApp : Application() {
    val appScope = CoroutineScope(SupervisorJob())
    lateinit var settings: SettingsRepository
        private set
    lateinit var hub: SessionHub
        private set

    override fun onCreate() {
        super.onCreate()
        settings = SettingsRepository(this)
        val vault = SeedVault(File(noBackupFilesDir, "identity.sealed"))
        val seed = vault.load()
        // DataStore's first read is tiny and needed before the core binds its relays.
        val saved = runBlocking { settings.settings.first() }
        val relays = RelayUrls.parse(saved.relayUrls).orEmpty()
        val core = ScrinCore(
            File(noBackupFilesDir, "core").absolutePath,
            seed,
            CoreConfig(
                deviceName = Build.MODEL ?: "Android",
                relayUrls = relays,
                loopbackOnly = false,
                serverUrl = ServerUrl.parse(saved.serverUrl),
            ),
        )
        if (seed == null) vault.store(core.identitySeed())
        hub = SessionHub(NativeCore(core), appScope)
    }
}
