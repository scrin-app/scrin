package ro.dragoscatalin.scrin.ui.screens

import android.app.LocaleManager
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.os.LocaleList
import android.provider.Settings
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.SegmentedButton
import androidx.compose.material3.SegmentedButtonDefaults
import androidx.compose.material3.SingleChoiceSegmentedButtonRow
import androidx.compose.material3.Surface
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import androidx.core.net.toUri
import kotlinx.coroutines.launch
import ro.dragoscatalin.scrin.BuildConfig
import ro.dragoscatalin.scrin.R
import ro.dragoscatalin.scrin.ScrinApp
import ro.dragoscatalin.scrin.core.SessionHub
import ro.dragoscatalin.scrin.data.RelayUrls
import ro.dragoscatalin.scrin.data.ThemeMode
import ro.dragoscatalin.scrin.ffi.TrustProfile
import ro.dragoscatalin.scrin.ffi.TrustedDevice
import ro.dragoscatalin.scrin.ui.components.ScrinIcons
import ro.dragoscatalin.scrin.ui.components.ScrinTopBar
import ro.dragoscatalin.scrin.ui.components.SectionCard
import ro.dragoscatalin.scrin.ui.components.SectionTitle
import ro.dragoscatalin.scrin.ui.theme.CodeStyle
import ro.dragoscatalin.scrin.ui.theme.Oklch
import ro.dragoscatalin.scrin.ui.theme.Tokens
import ro.dragoscatalin.scrin.data.Settings as AppSettings

@Composable
private fun ScreenColumn(title: String, onBack: () -> Unit, content: @Composable () -> Unit) {
    Column(Modifier.fillMaxSize().safeDrawingPadding()) {
        ScrinTopBar(title, onBack = onBack)
        Column(
            Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(horizontal = 20.dp, vertical = 4.dp),
            verticalArrangement = Arrangement.spacedBy(16.dp),
        ) { content() }
    }
}

@Composable
private fun NavRow(label: String, onClick: () -> Unit) {
    Row(
        Modifier.fillMaxWidth().heightIn(min = 48.dp).clickable(onClick = onClick, role = Role.Button),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(label, style = MaterialTheme.typography.bodyLarge, modifier = Modifier.weight(1f))
        Icon(ScrinIcons.ChevronRight, contentDescription = null)
    }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
fun SettingsScreen(
    app: ScrinApp,
    settings: AppSettings,
    onBack: () -> Unit,
    onTrusted: () -> Unit,
    onAbout: () -> Unit,
    onRestrictedGuide: () -> Unit,
) {
    val scope = rememberCoroutineScope()
    val repo = app.settings
    ScreenColumn(stringResource(R.string.settings_title), onBack) {
        SectionCard {
            SectionTitle(stringResource(R.string.settings_appearance))
            Text(stringResource(R.string.settings_theme), style = MaterialTheme.typography.labelLarge)
            val modes = listOf(ThemeMode.SYSTEM to R.string.theme_system, ThemeMode.LIGHT to R.string.theme_light, ThemeMode.DARK to R.string.theme_dark)
            SingleChoiceSegmentedButtonRow(Modifier.fillMaxWidth()) {
                modes.forEachIndexed { i, (m, label) ->
                    SegmentedButton(
                        selected = settings.theme == m,
                        onClick = { scope.launch { repo.setTheme(m) } },
                        shape = SegmentedButtonDefaults.itemShape(i, modes.size),
                    ) { Text(stringResource(label)) }
                }
            }
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
                Row(
                    Modifier.fillMaxWidth().heightIn(min = 48.dp).toggleable(settings.dynamicColor, role = Role.Switch) { v -> scope.launch { repo.setDynamic(v) } },
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Column(Modifier.weight(1f)) {
                        Text(stringResource(R.string.settings_dynamic_color), style = MaterialTheme.typography.bodyLarge)
                        Text(stringResource(R.string.settings_dynamic_color_body), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                    }
                    Switch(checked = settings.dynamicColor, onCheckedChange = null)
                }
            }
            Text(stringResource(R.string.settings_accent), style = MaterialTheme.typography.labelLarge)
            FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Tokens.ACCENTS.forEach { a ->
                    val selected = settings.accent == a.id
                    val name = stringResource(a.label)
                    Box(
                        Modifier.size(48.dp).selectable(selected, enabled = !settings.dynamicColor, role = Role.RadioButton) { scope.launch { repo.setAccent(a.id) } }
                            .semantics { contentDescription = name },
                        contentAlignment = Alignment.Center,
                    ) {
                        Surface(
                            shape = CircleShape,
                            color = Color(Oklch.toArgb(0.6, a.chroma, a.hue)),
                            border = if (selected) BorderStroke(3.dp, MaterialTheme.colorScheme.onSurface) else null,
                            modifier = Modifier.size(36.dp),
                        ) {
                            if (selected) Box(contentAlignment = Alignment.Center) { Icon(ScrinIcons.Check, null, tint = Color.White, modifier = Modifier.size(18.dp)) }
                        }
                    }
                }
            }
        }
        LanguageCard()
        SectionCard {
            SectionTitle(stringResource(R.string.settings_security))
            Text(stringResource(R.string.settings_fingerprint), style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
            Text(app.hub.fingerprint, style = CodeStyle)
            NavRow(stringResource(R.string.trusted_title), onTrusted)
            NavRow(stringResource(R.string.restricted_title), onRestrictedGuide)
        }
        RelayCard(settings.relayUrls) { scope.launch { repo.setRelays(it) } }
        ServerCard(settings.serverUrl) { scope.launch { repo.setServer(it) } }
        SectionCard {
            SectionTitle(stringResource(R.string.settings_about))
            NavRow(stringResource(R.string.about_title), onAbout)
        }
    }
}

@Composable
private fun RelayCard(current: String, onSave: (String) -> Unit) {
    RelayCardBody(current, onSave)
}

/** Rendezvous server for 9-digit scrin IDs (register as host, resolve as controller). */
@Composable
private fun ServerCard(current: String, onSave: (String) -> Unit) {
    var text by remember(current) { mutableStateOf(current) }
    val valid = ro.dragoscatalin.scrin.data.ServerUrl.valid(text)
    SectionCard {
        SectionTitle(stringResource(R.string.settings_server))
        Text(stringResource(R.string.settings_server_body), style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
        OutlinedTextField(
            value = text,
            onValueChange = { text = it },
            label = { Text(stringResource(R.string.settings_server_label)) },
            placeholder = { Text("https://scrin.example.org") },
            isError = !valid,
            supportingText = { Text(stringResource(if (valid) R.string.settings_relay_restart else R.string.settings_server_invalid)) },
            singleLine = true,
            modifier = Modifier.fillMaxWidth(),
        )
        Button(onClick = { onSave(text) }, enabled = valid && text != current, modifier = Modifier.heightIn(min = 48.dp)) {
            Text(stringResource(R.string.action_save))
        }
    }
}

/** Per-app language: in-app picker on Android 13+, system locale settings before. */
@Composable
private fun LanguageCard() {
    val ctx = LocalContext.current
    SectionCard {
        SectionTitle(stringResource(R.string.settings_language))
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            val lm = ctx.getSystemService(LocaleManager::class.java)
            var current by remember { mutableStateOf(lm.applicationLocales.toLanguageTags()) }
            val langs = listOf("" to R.string.lang_system, "en" to R.string.lang_en, "ro" to R.string.lang_ro)
            langs.forEach { (tag, label) ->
                Row(
                    Modifier.fillMaxWidth().heightIn(min = 48.dp).selectable(current == tag, role = Role.RadioButton) {
                        lm.applicationLocales = if (tag.isEmpty()) LocaleList.getEmptyLocaleList() else LocaleList.forLanguageTags(tag)
                        current = tag
                    },
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    androidx.compose.material3.RadioButton(selected = current == tag, onClick = null)
                    Text(stringResource(label), Modifier.padding(start = 12.dp))
                }
            }
        } else {
            Text(stringResource(R.string.settings_language_system_only), style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
            TextButton(onClick = { ctx.startActivity(Intent(Settings.ACTION_LOCALE_SETTINGS).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)) }) {
                Text(stringResource(R.string.settings_language_open))
            }
        }
    }
}

@Composable
private fun RelayCardBody(current: String, onSave: (String) -> Unit) {
    var text by remember(current) { mutableStateOf(current) }
    val valid = RelayUrls.parse(text) != null
    SectionCard {
        SectionTitle(stringResource(R.string.settings_network))
        Text(stringResource(R.string.settings_relay_body), style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
        OutlinedTextField(
            value = text,
            onValueChange = { text = it },
            label = { Text(stringResource(R.string.settings_relay_label)) },
            placeholder = { Text("https://relay.example.org") },
            isError = !valid,
            supportingText = { Text(stringResource(if (valid) R.string.settings_relay_restart else R.string.settings_relay_invalid)) },
            singleLine = true,
            modifier = Modifier.fillMaxWidth(),
        )
        Button(onClick = { onSave(text) }, enabled = valid && text != current, modifier = Modifier.heightIn(min = 48.dp)) {
            Text(stringResource(R.string.action_save))
        }
    }
}

@Composable
fun TrustedDevicesScreen(hub: SessionHub, onBack: () -> Unit) {
    var list by remember { mutableStateOf(runCatching { hub.trusted() }.getOrDefault(emptyList())) }
    ScreenColumn(stringResource(R.string.trusted_title), onBack) {
        Text(stringResource(R.string.trusted_body), style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
        if (list.isEmpty()) {
            SectionCard { Text(stringResource(R.string.trusted_empty), style = MaterialTheme.typography.bodyLarge) }
        }
        list.forEach { d ->
            TrustedRow(d) {
                runCatching { hub.removeTrusted(d.deviceId) }
                list = runCatching { hub.trusted() }.getOrDefault(emptyList())
            }
        }
    }
}

@Composable
private fun TrustedRow(d: TrustedDevice, onRemove: () -> Unit) {
    SectionCard {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                Text(d.label, style = MaterialTheme.typography.titleMedium)
                Text(d.fingerprint, style = CodeStyle.copy(fontSize = MaterialTheme.typography.bodySmall.fontSize))
                Text(
                    stringResource(
                        when (d.profile) {
                            TrustProfile.VIEW_ONLY -> R.string.trust_view_only
                            TrustProfile.SUPPORT -> R.string.trust_support
                            TrustProfile.FULL -> R.string.trust_full
                        },
                    ),
                    style = MaterialTheme.typography.labelMedium,
                    color = MaterialTheme.colorScheme.primary,
                )
            }
            IconButton(onClick = onRemove, modifier = Modifier.size(48.dp)) {
                Icon(ScrinIcons.Delete, contentDescription = stringResource(R.string.trusted_remove, d.label))
            }
        }
    }
}

@Composable
fun AboutScreen(onBack: () -> Unit) {
    val ctx = LocalContext.current
    ScreenColumn(stringResource(R.string.about_title), onBack) {
        SectionCard {
            Text(stringResource(R.string.app_name), style = MaterialTheme.typography.headlineSmall, color = MaterialTheme.colorScheme.primary)
            Text(stringResource(R.string.about_version, BuildConfig.VERSION_NAME, BuildConfig.FLAVOR), style = MaterialTheme.typography.bodyMedium)
            Text(stringResource(R.string.about_body), style = MaterialTheme.typography.bodyMedium)
        }
        SectionCard {
            SectionTitle(stringResource(R.string.about_licence))
            Text(stringResource(R.string.about_licence_body), style = MaterialTheme.typography.bodyMedium)
            TextButton(onClick = {
                ctx.startActivity(Intent(Intent.ACTION_VIEW, "https://www.gnu.org/licenses/agpl-3.0.html".toUri()))
            }) { Text(stringResource(R.string.about_read_licence)) }
            TextButton(onClick = {
                ctx.startActivity(Intent(Intent.ACTION_VIEW, "https://github.com/scrin-app/scrin".toUri()))
            }) { Text(stringResource(R.string.about_source)) }
        }
        SectionCard {
            SectionTitle(stringResource(R.string.about_third_party))
            Text(stringResource(R.string.about_third_party_body), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
    }
}

/** Step-by-step help for Android 13+ "Restricted setting" on sideloaded installs. */
@Composable
fun RestrictedGuideScreen(onBack: () -> Unit) {
    val ctx = LocalContext.current
    val steps = listOf(R.string.restricted_step1, R.string.restricted_step2, R.string.restricted_step3, R.string.restricted_step4, R.string.restricted_step5)
    ScreenColumn(stringResource(R.string.restricted_title), onBack) {
        Text(stringResource(R.string.restricted_body), style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
        SectionCard {
            steps.forEachIndexed { i, res ->
                Row(horizontalArrangement = Arrangement.spacedBy(12.dp), verticalAlignment = Alignment.Top) {
                    Box(
                        Modifier.size(28.dp).background(MaterialTheme.colorScheme.primary, CircleShape),
                        contentAlignment = Alignment.Center,
                    ) { Text("${i + 1}", color = MaterialTheme.colorScheme.onPrimary, style = MaterialTheme.typography.labelLarge) }
                    Text(stringResource(res), style = MaterialTheme.typography.bodyLarge, modifier = Modifier.weight(1f))
                }
            }
        }
        Button(
            onClick = {
                val i = Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS, Uri.fromParts("package", ctx.packageName, null))
                ctx.startActivity(i.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
            },
            modifier = Modifier.fillMaxWidth().heightIn(min = 52.dp),
        ) { Text(stringResource(R.string.restricted_open_app_info)) }
        Button(
            onClick = { ctx.startActivity(Intent(Settings.ACTION_ACCESSIBILITY_SETTINGS).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)) },
            modifier = Modifier.fillMaxWidth().heightIn(min = 52.dp),
        ) { Text(stringResource(R.string.restricted_open_a11y)) }
    }
}
