package ro.dragoscatalin.scrin.ui.screens

import android.Manifest
import android.content.ClipData
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import android.provider.Settings
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.ClipEntry
import androidx.compose.ui.platform.LocalClipboard
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import androidx.core.content.ContextCompat
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.compose.LocalLifecycleOwner
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import ro.dragoscatalin.scrin.R
import ro.dragoscatalin.scrin.ScrinApp
import ro.dragoscatalin.scrin.core.AdvancedProtection
import ro.dragoscatalin.scrin.core.Role
import ro.dragoscatalin.scrin.core.SessionHub
import ro.dragoscatalin.scrin.data.Settings as AppSettings
import ro.dragoscatalin.scrin.ffi.SessionState
import ro.dragoscatalin.scrin.service.RemoteInputService
import ro.dragoscatalin.scrin.service.ScreenCaptureService
import ro.dragoscatalin.scrin.ui.components.Badge
import ro.dragoscatalin.scrin.ui.components.SasRow
import ro.dragoscatalin.scrin.ui.components.ScrinIcons
import ro.dragoscatalin.scrin.ui.components.ScrinTopBar
import ro.dragoscatalin.scrin.ui.components.SectionCard
import ro.dragoscatalin.scrin.ui.components.SectionTitle
import ro.dragoscatalin.scrin.ui.theme.CodeStyle
import ro.dragoscatalin.scrin.ui.theme.LocalScrinColors

@Composable
fun HostScreen(app: ScrinApp, settings: AppSettings, onBack: () -> Unit, onRestrictedGuide: () -> Unit) {
    val hub = app.hub
    val ui by hub.ui.collectAsState()
    val code by hub.code.collectAsState()
    val ticket by hub.ticket.collectAsState()
    val scrinId by hub.scrinId.collectAsState()
    val ctx = LocalContext.current
    val scope = rememberCoroutineScope()
    var a11yOn by remember { mutableStateOf(RemoteInputService.isEnabled(ctx)) }
    var aapm by remember { mutableStateOf(AdvancedProtection.enabled(ctx)) }
    var showDisclosure by remember { mutableStateOf(false) }
    var notifOk by remember { mutableStateOf(notificationsGranted(ctx)) }
    val notifLauncher = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { notifOk = it }

    // Re-check after returning from system settings.
    val owner = LocalLifecycleOwner.current
    DisposableEffect(owner) {
        val obs = LifecycleEventObserver { _, e ->
            if (e == Lifecycle.Event.ON_RESUME) {
                a11yOn = RemoteInputService.isEnabled(ctx)
                aapm = AdvancedProtection.enabled(ctx)
                notifOk = notificationsGranted(ctx)
            }
        }
        owner.lifecycle.addObserver(obs)
        onDispose { owner.lifecycle.removeObserver(obs) }
    }

    val hostActive = ui.role == Role.HOST && ui.state == SessionState.ACTIVE
    Column(Modifier.fillMaxSize().safeDrawingPadding()) {
        ScrinTopBar(stringResource(R.string.host_title), onBack = onBack)
        Column(
            Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(horizontal = 20.dp),
            verticalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            if (hostActive) {
                ActiveSessionCard(app)
            }
            if (aapm) {
                SectionCard(container = MaterialTheme.colorScheme.secondaryContainer) {
                    Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                        Icon(ScrinIcons.Shield, contentDescription = null)
                        Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
                            Text(stringResource(R.string.aapm_title), style = MaterialTheme.typography.titleSmall, modifier = Modifier.semantics { heading() })
                            Text(stringResource(R.string.aapm_body), style = MaterialTheme.typography.bodyMedium)
                        }
                    }
                }
            } else SectionCard {
                SectionTitle(stringResource(R.string.host_step_input))
                Text(stringResource(R.string.host_input_body), style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                if (a11yOn) {
                    Badge(stringResource(R.string.host_input_enabled), LocalScrinColors.current.success, MaterialTheme.colorScheme.surface)
                } else {
                    Button(onClick = { showDisclosure = true }, modifier = Modifier.fillMaxWidth().heightIn(min = 48.dp)) {
                        Text(stringResource(R.string.host_input_enable))
                    }
                    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                        TextButton(onClick = onRestrictedGuide) { Text(stringResource(R.string.host_restricted_link)) }
                    }
                }
            }
            if (!notifOk && Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                SectionCard {
                    SectionTitle(stringResource(R.string.host_step_notifications))
                    Text(stringResource(R.string.host_notifications_body), style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                    OutlinedButton(onClick = { notifLauncher.launch(Manifest.permission.POST_NOTIFICATIONS) }, modifier = Modifier.heightIn(min = 48.dp)) {
                        Text(stringResource(R.string.action_allow))
                    }
                }
            }
            SectionCard(container = MaterialTheme.colorScheme.primaryContainer) {
                SectionTitle(stringResource(R.string.host_step_share))
                Text(stringResource(R.string.host_share_body), style = MaterialTheme.typography.bodyMedium)
                Text(hub.fingerprint, style = CodeStyle)
                scrinId?.let { Text(stringResource(R.string.home_scrin_id, it.chunked(3).joinToString(" ")), style = CodeStyle) }
                ticket?.let { Text(it, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant) }
                Text(code?.display ?: "····-····", style = CodeStyle.copy(fontSize = MaterialTheme.typography.headlineMedium.fontSize))
                Text(
                    stringResource(if (ui.listening) R.string.host_listening else R.string.host_not_listening),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            PassphraseCard(hub)
            if (ui.role == Role.HOST && ui.state == SessionState.PAIRING) {
                SectionCard { Text(stringResource(R.string.host_pairing), style = MaterialTheme.typography.bodyLarge) }
            }
            ui.ended?.takeIf { ui.role == Role.HOST }?.let { EndedCard(it.kind) { hub.dismissEnded() } }
            ScamWarningCard()
        }
    }

    if (showDisclosure) {
        ProminentDisclosureDialog(
            onAgree = {
                showDisclosure = false
                scope.launch { app.settings.setDisclosureAccepted(true) }
                ctx.startActivity(Intent(Settings.ACTION_ACCESSIBILITY_SETTINGS).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
            },
            onDecline = { showDisclosure = false },
        )
    }
    // Keep the flag observable for future screens (e.g. skip re-showing on re-enable).
    settings.disclosureAccepted
}

private fun notificationsGranted(ctx: android.content.Context): Boolean =
    Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU ||
        ContextCompat.checkSelfPermission(ctx, Manifest.permission.POST_NOTIFICATIONS) == PackageManager.PERMISSION_GRANTED

/** D24: five dictated words instead of ID + code. Words 1–2 locate, 3–5 are the single-use secret. */
@Composable
private fun PassphraseCard(hub: SessionHub) {
    val on by hub.phraseOn.collectAsState()
    val phrase by hub.phrase.collectAsState()
    val failed by hub.phraseFailed.collectAsState()
    val clipboard = LocalClipboard.current
    val scope = rememberCoroutineScope()
    val lang = LocalConfiguration.current.locales[0].toLanguageTag()
    LaunchedEffect(on) {
        while (on) {
            hub.phraseTick()
            delay(1000)
        }
    }
    SectionCard {
        Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            Icon(ScrinIcons.Info, contentDescription = null)
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                Text(stringResource(R.string.phrase_title), style = MaterialTheme.typography.titleSmall, modifier = Modifier.semantics { heading() })
                Text(stringResource(R.string.phrase_hint), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
        }
        val words = phrase?.words.orEmpty()
        when {
            !hub.serverConfigured -> Text(
                stringResource(R.string.phrase_needs_server),
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            on && words.size == 5 -> {
                val a11y = stringResource(R.string.phrase_a11y, words.joinToString(", "))
                FlowRow(
                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                    verticalArrangement = Arrangement.spacedBy(8.dp),
                    modifier = Modifier.clearAndSetSemantics { contentDescription = a11y },
                ) {
                    words.forEachIndexed { i, w -> WordChip(w, accent = i >= 2) }
                }
                Text(stringResource(R.string.phrase_once), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                    FilledTonalButton(
                        onClick = { scope.launch { clipboard.setClipEntry(ClipEntry(ClipData.newPlainText("scrin", words.joinToString(" ")))) } },
                        modifier = Modifier.heightIn(min = 48.dp),
                    ) {
                        Icon(ScrinIcons.Copy, null, Modifier.size(18.dp))
                        Text(stringResource(R.string.phrase_copy), Modifier.padding(start = 8.dp))
                    }
                    FilledTonalButton(onClick = { hub.renewPhrase() }, modifier = Modifier.heightIn(min = 48.dp)) {
                        Icon(ScrinIcons.Refresh, null, Modifier.size(18.dp))
                        Text(stringResource(R.string.phrase_new), Modifier.padding(start = 8.dp))
                    }
                    TextButton(onClick = { hub.disablePhrase() }, modifier = Modifier.heightIn(min = 48.dp)) {
                        Text(stringResource(R.string.phrase_hide))
                    }
                }
            }
            on -> {
                if (failed) Text(stringResource(R.string.phrase_unavailable), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.error)
                TextButton(onClick = { hub.disablePhrase() }, modifier = Modifier.heightIn(min = 48.dp)) {
                    Text(stringResource(R.string.phrase_hide))
                }
            }
            else -> OutlinedButton(onClick = { hub.enablePhrase(lang) }, modifier = Modifier.heightIn(min = 48.dp)) {
                Text(stringResource(R.string.phrase_show))
            }
        }
    }
}

@Composable
private fun WordChip(word: String, accent: Boolean) {
    val scheme = MaterialTheme.colorScheme
    Surface(
        shape = RoundedCornerShape(8.dp),
        color = if (accent) scheme.primaryContainer else scheme.surfaceVariant,
        contentColor = if (accent) scheme.onPrimaryContainer else scheme.onSurfaceVariant,
        border = if (accent) BorderStroke(1.dp, scheme.primary.copy(alpha = 0.4f)) else null,
    ) {
        Text(word, style = CodeStyle.copy(fontSize = MaterialTheme.typography.titleMedium.fontSize), modifier = Modifier.padding(horizontal = 10.dp, vertical = 6.dp))
    }
}

/** Google Play "prominent disclosure": what, why, how to stop — shown BEFORE Accessibility settings. */
@Composable
private fun ProminentDisclosureDialog(onAgree: () -> Unit, onDecline: () -> Unit) {
    AlertDialog(
        onDismissRequest = onDecline,
        icon = { Icon(ScrinIcons.Info, contentDescription = null) },
        title = { Text(stringResource(R.string.disclosure_title)) },
        text = {
            Column(Modifier.verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(10.dp)) {
                Text(stringResource(R.string.disclosure_what), style = MaterialTheme.typography.bodyMedium)
                Text(stringResource(R.string.disclosure_data), style = MaterialTheme.typography.bodyMedium)
                Text(stringResource(R.string.disclosure_not), style = MaterialTheme.typography.bodyMedium)
                Text(stringResource(R.string.disclosure_off), style = MaterialTheme.typography.bodyMedium)
            }
        },
        confirmButton = { Button(onClick = onAgree) { Text(stringResource(R.string.disclosure_agree)) } },
        dismissButton = { TextButton(onClick = onDecline) { Text(stringResource(R.string.disclosure_decline)) } },
    )
}

@Composable
private fun ActiveSessionCard(app: ScrinApp) {
    val hub = app.hub
    val ui by hub.ui.collectAsState()
    val ctx = LocalContext.current
    val danger = LocalScrinColors.current.danger
    SectionCard(container = MaterialTheme.colorScheme.errorContainer) {
        Text(stringResource(R.string.host_active_title), style = MaterialTheme.typography.titleMedium, modifier = Modifier.semantics { heading() })
        ui.lastRequest?.let { Text(stringResource(R.string.host_active_with, it.controllerName.ifBlank { it.peerFingerprint }), style = MaterialTheme.typography.bodyMedium) }
        ui.sas?.let { SasRow(it) }
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
            Button(
                onClick = { hub.end(); ScreenCaptureService.stop(ctx) },
                modifier = Modifier.weight(1f).heightIn(min = 48.dp),
            ) {
                Icon(ScrinIcons.Stop, null, Modifier.size(18.dp))
                Text(stringResource(R.string.host_stop), Modifier.padding(start = 8.dp))
            }
            Button(
                onClick = { hub.stopAndReport(); ScreenCaptureService.stop(ctx) },
                colors = ButtonDefaults.buttonColors(containerColor = danger),
                modifier = Modifier.weight(1f).heightIn(min = 48.dp),
            ) {
                Icon(ScrinIcons.Flag, null, Modifier.size(18.dp))
                Text(stringResource(R.string.host_stop_report), Modifier.padding(start = 8.dp))
            }
        }
    }
}

@Composable
fun ScamWarningCard() {
    SectionCard {
        Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            Icon(ScrinIcons.Warning, contentDescription = null, tint = LocalScrinColors.current.warning)
            Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
                Text(stringResource(R.string.scam_title), style = MaterialTheme.typography.titleSmall)
                Text(stringResource(R.string.scam_body), style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
        }
    }
}
