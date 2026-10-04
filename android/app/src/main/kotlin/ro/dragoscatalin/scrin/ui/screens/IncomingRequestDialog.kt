package ro.dragoscatalin.scrin.ui.screens

import android.app.Activity
import android.media.projection.MediaProjectionManager
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.Checkbox
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.DialogProperties
import kotlinx.coroutines.delay
import ro.dragoscatalin.scrin.R
import ro.dragoscatalin.scrin.core.SessionHub
import ro.dragoscatalin.scrin.ffi.IncomingRequest
import ro.dragoscatalin.scrin.ffi.SessionPermission
import ro.dragoscatalin.scrin.service.ScreenCaptureService
import ro.dragoscatalin.scrin.ui.components.Badge
import ro.dragoscatalin.scrin.ui.components.SasRow
import ro.dragoscatalin.scrin.ui.components.ScrinIcons
import ro.dragoscatalin.scrin.ui.theme.CodeStyle
import ro.dragoscatalin.scrin.ui.theme.LocalScrinColors

/** Permissions an Android host can actually serve today. */
private val OFFERED = listOf(SessionPermission.VIEW, SessionPermission.INPUT, SessionPermission.CLIPBOARD, SessionPermission.CHAT)

/**
 * Pre-accept interstitial (ADR-0009): who is asking, verified badge, the scam warning,
 * permission selection and an Accept that unlocks only after the policy delay.
 * Accept then asks Android for screen-capture consent (every session, D08).
 */
@Composable
fun IncomingRequestDialog(hub: SessionHub, req: IncomingRequest) {
    val ctx = LocalContext.current
    val ui by hub.ui.collectAsState()
    val sas = ui.sas
    val start = remember(req) { android.os.SystemClock.elapsedRealtime() }
    var now by remember(req) { mutableLongStateOf(start) }
    LaunchedEffect(req) {
        while (now - start < req.acceptInMs.toLong()) {
            delay(200)
            now = android.os.SystemClock.elapsedRealtime()
        }
    }
    val waitMs = (req.acceptInMs.toLong() - (now - start)).coerceAtLeast(0)
    val offered = OFFERED.filter { it in req.allowed }
    var chosen by remember(req) { mutableStateOf(offered.filter { it in req.requested }.toSet()) }

    val consent = rememberLauncherForActivityResult(ActivityResultContracts.StartActivityForResult()) { r ->
        val data = r.data
        if (r.resultCode == Activity.RESULT_OK && data != null) {
            hub.accept(chosen.toList())
            ScreenCaptureService.start(ctx, r.resultCode, data)
        } else {
            hub.reject()
        }
    }

    AlertDialog(
        onDismissRequest = {},
        properties = DialogProperties(dismissOnBackPress = false, dismissOnClickOutside = false),
        icon = { Icon(ScrinIcons.Warning, contentDescription = null, tint = LocalScrinColors.current.warning) },
        title = { Text(stringResource(R.string.request_title)) },
        text = {
            Column(Modifier.verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(12.dp)) {
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    Text(req.controllerName.ifBlank { stringResource(R.string.request_unknown_device) }, style = MaterialTheme.typography.titleMedium, modifier = Modifier.weight(1f))
                    if (req.verified) {
                        Badge(stringResource(R.string.request_verified), LocalScrinColors.current.success, MaterialTheme.colorScheme.surface)
                    } else {
                        Badge(stringResource(R.string.request_unverified), LocalScrinColors.current.warning, MaterialTheme.colorScheme.surface)
                    }
                }
                Text(req.peerFingerprint, style = CodeStyle)
                Text(stringResource(R.string.scam_body), style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.error)
                sas?.let {
                    Text(stringResource(R.string.sas_body_host), style = MaterialTheme.typography.bodyMedium)
                    SasRow(it)
                }
                Text(stringResource(R.string.request_permissions), style = MaterialTheme.typography.labelLarge)
                offered.forEach { p ->
                    val on = p in chosen
                    Row(
                        Modifier.fillMaxWidth().heightIn(min = 48.dp).toggleable(on, role = Role.Checkbox) { v -> chosen = if (v) chosen + p else chosen - p },
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Checkbox(checked = on, onCheckedChange = null)
                        Text(stringResource(permissionLabel(p)), Modifier.weight(1f))
                    }
                }
                if (!req.verified) {
                    Text(stringResource(R.string.request_anonymous_caps), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
            }
        },
        confirmButton = {
            Button(
                enabled = waitMs == 0L && SessionPermission.VIEW in chosen,
                onClick = {
                    val mpm = ctx.getSystemService(MediaProjectionManager::class.java)
                    consent.launch(mpm.createScreenCaptureIntent())
                },
            ) {
                Text(if (waitMs > 0) stringResource(R.string.request_accept_in, ((waitMs + 999) / 1000).toInt()) else stringResource(R.string.request_accept))
            }
        },
        dismissButton = {
            Row {
                TextButton(onClick = { hub.stopAndReport() }) { Text(stringResource(R.string.request_report)) }
                TextButton(onClick = { hub.reject() }) { Text(stringResource(R.string.request_reject)) }
            }
        },
    )
}

fun permissionLabel(p: SessionPermission): Int = when (p) {
    SessionPermission.VIEW -> R.string.perm_view
    SessionPermission.INPUT -> R.string.perm_input
    SessionPermission.CLIPBOARD -> R.string.perm_clipboard
    SessionPermission.CHAT -> R.string.perm_chat
    else -> R.string.perm_other
}
