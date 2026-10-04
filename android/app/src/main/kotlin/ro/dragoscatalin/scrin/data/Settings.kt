package ro.dragoscatalin.scrin.data

import android.content.Context
import androidx.datastore.core.DataStore
import androidx.datastore.preferences.core.Preferences
import androidx.datastore.preferences.core.booleanPreferencesKey
import androidx.datastore.preferences.core.edit
import androidx.datastore.preferences.core.stringPreferencesKey
import androidx.datastore.preferences.preferencesDataStore
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.map
import ro.dragoscatalin.scrin.ui.theme.Tokens

enum class ThemeMode { SYSTEM, LIGHT, DARK }

data class Settings(
    val theme: ThemeMode = ThemeMode.SYSTEM,
    val accent: String = Tokens.DEFAULT_ACCENT,
    val dynamicColor: Boolean = false,
    /** Empty = default relays. Comma-separated https URLs otherwise. */
    val relayUrls: String = "",
    val disclosureAccepted: Boolean = false,
)

private val Context.store: DataStore<Preferences> by preferencesDataStore(name = "settings")

class SettingsRepository(private val context: Context) {
    private object K {
        val THEME = stringPreferencesKey("theme")
        val ACCENT = stringPreferencesKey("accent")
        val DYNAMIC = booleanPreferencesKey("dynamic_color")
        val RELAYS = stringPreferencesKey("relay_urls")
        val DISCLOSURE = booleanPreferencesKey("a11y_disclosure_accepted")
    }

    val settings: Flow<Settings> = context.store.data.map { p ->
        Settings(
            theme = p[K.THEME]?.let { runCatching { ThemeMode.valueOf(it) }.getOrNull() } ?: ThemeMode.SYSTEM,
            accent = p[K.ACCENT] ?: Tokens.DEFAULT_ACCENT,
            dynamicColor = p[K.DYNAMIC] ?: false,
            relayUrls = p[K.RELAYS] ?: "",
            disclosureAccepted = p[K.DISCLOSURE] ?: false,
        )
    }

    suspend fun setTheme(m: ThemeMode) = context.store.edit { it[K.THEME] = m.name }
    suspend fun setAccent(id: String) = context.store.edit { it[K.ACCENT] = id }
    suspend fun setDynamic(on: Boolean) = context.store.edit { it[K.DYNAMIC] = on }
    suspend fun setRelays(v: String) = context.store.edit { it[K.RELAYS] = v.trim() }
    suspend fun setDisclosureAccepted(on: Boolean) = context.store.edit { it[K.DISCLOSURE] = on }
}

/** Validates the relay field: empty, or comma-separated https:// URLs. */
object RelayUrls {
    fun parse(input: String): List<String>? {
        val parts = input.split(',').map { it.trim() }.filter { it.isNotEmpty() }
        if (parts.isEmpty()) return emptyList()
        return parts.takeIf { list -> list.all { it.startsWith("https://") && it.length > "https://".length && ' ' !in it } }
    }
}
