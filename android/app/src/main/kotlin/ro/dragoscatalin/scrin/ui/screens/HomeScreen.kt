package ro.dragoscatalin.scrin.ui.screens

import android.content.ClipData
import android.content.Intent
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.FilledTonalIconButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.ClipEntry
import androidx.compose.ui.platform.LocalClipboard
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.TextFieldValue
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import ro.dragoscatalin.scrin.R
import ro.dragoscatalin.scrin.core.SessionHub
import ro.dragoscatalin.scrin.ui.ConnectForm
import ro.dragoscatalin.scrin.ui.Format
import ro.dragoscatalin.scrin.ui.components.CountdownRing
import ro.dragoscatalin.scrin.ui.components.ScrinIcons
import ro.dragoscatalin.scrin.ui.components.SectionCard
import ro.dragoscatalin.scrin.ui.components.SectionTitle
import ro.dragoscatalin.scrin.ui.theme.CodeStyle

@Composable
fun HomeScreen(hub: SessionHub, onConnect: () -> Unit, onHost: () -> Unit, onSettings: () -> Unit) {
    val code by hub.code.collectAsState()
    val ticket by hub.ticket.collectAsState()
    var now by remember { mutableLongStateOf(android.os.SystemClock.elapsedRealtime()) }
    LaunchedEffect(Unit) { if (hub.codeExpired()) hub.refreshCode() }
    LaunchedEffect(code) {
        while (true) {
            now = android.os.SystemClock.elapsedRealtime()
            if (hub.codeExpired(now)) hub.refreshCode()
            delay(1000)
        }
    }

    Column(
        Modifier.fillMaxSize().safeDrawingPadding().verticalScroll(rememberScrollState()).padding(horizontal = 20.dp, vertical = 12.dp),
        verticalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Column(Modifier.weight(1f)) {
                Text(stringResource(R.string.app_name), style = MaterialTheme.typography.displaySmall, color = MaterialTheme.colorScheme.primary)
                Text(stringResource(R.string.home_tagline), style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
            IconButton(onClick = onSettings, modifier = Modifier.size(48.dp)) {
                Icon(ScrinIcons.Settings, contentDescription = stringResource(R.string.settings_title))
            }
        }
        MyDeviceCard(hub, code?.display, code?.let { (it.expiresAtMs - now) / 1000 } ?: 0, code?.let { Format.progress(it.expiresAtMs - now, it.totalMs) } ?: 0f, ticket, onHost)
        ConnectCard(hub, onConnect)
    }
}

@Composable
private fun MyDeviceCard(hub: SessionHub, code: String?, secondsLeft: Long, progress: Float, ticket: String?, onHost: () -> Unit) {
    val ctx = LocalContext.current
    val clipboard = LocalClipboard.current
    val scope = rememberCoroutineScope()
    val shareTitle = stringResource(R.string.home_share_title)
    val shareText = if (ticket != null && code != null) stringResource(R.string.home_share_text, ticket, code) else ""
    SectionCard(container = MaterialTheme.colorScheme.primaryContainer) {
        SectionTitle(stringResource(R.string.home_my_device))
        Text(stringResource(R.string.home_your_id), style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
        Text(hub.fingerprint, style = CodeStyle.copy(fontSize = MaterialTheme.typography.titleLarge.fontSize))
        val scrinId by hub.scrinId.collectAsState()
        scrinId?.let { Text(stringResource(R.string.home_scrin_id, it.chunked(3).joinToString(" ")), style = CodeStyle) }
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(16.dp)) {
            val ringLabel = stringResource(R.string.home_code_expires_in, Format.countdown(secondsLeft))
            CountdownRing(progress, 88.dp, ringLabel) {
                Text(Format.countdown(secondsLeft), style = MaterialTheme.typography.labelLarge)
            }
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                Text(stringResource(R.string.home_one_time_code), style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                Text(code ?: "····-····", style = CodeStyle.copy(fontSize = MaterialTheme.typography.headlineMedium.fontSize))
                Text(stringResource(R.string.home_code_hint), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
        }
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
            FilledTonalIconButton(onClick = { hub.refreshCode() }, modifier = Modifier.size(48.dp)) {
                Icon(ScrinIcons.Refresh, contentDescription = stringResource(R.string.home_new_code))
            }
            FilledTonalIconButton(
                onClick = { if (ticket != null) scope.launch { clipboard.setClipEntry(ClipEntry(ClipData.newPlainText("scrin", ticket))) } },
                enabled = ticket != null,
                modifier = Modifier.size(48.dp),
            ) { Icon(ScrinIcons.Copy, contentDescription = stringResource(R.string.home_copy_id)) }
            FilledTonalIconButton(
                onClick = {
                    val send = Intent(Intent.ACTION_SEND).setType("text/plain").putExtra(Intent.EXTRA_TEXT, shareText)
                    ctx.startActivity(Intent.createChooser(send, shareTitle))
                },
                enabled = ticket != null && code != null,
                modifier = Modifier.size(48.dp),
            ) { Icon(ScrinIcons.Share, contentDescription = shareTitle) }
            Spacer(Modifier.weight(1f))
            Button(onClick = onHost, modifier = Modifier.heightIn(min = 48.dp)) { Text(stringResource(R.string.home_allow_control)) }
        }
    }
}

@Composable
private fun ConnectCard(hub: SessionHub, onConnect: () -> Unit) {
    var target by rememberSaveable { mutableStateOf("") }
    var code by rememberSaveable(stateSaver = TextFieldValue.Saver) { mutableStateOf(TextFieldValue("")) }
    var problem by remember { mutableStateOf<ConnectForm.Problem?>(null) }
    val submit = {
        problem = hub.connect(target, code.text)
        if (problem == null) onConnect()
    }
    SectionCard {
        SectionTitle(stringResource(R.string.home_connect_title))
        Text(stringResource(R.string.home_connect_body), style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
        OutlinedTextField(
            value = target,
            onValueChange = { target = it; problem = null },
            label = { Text(stringResource(R.string.home_partner_id)) },
            leadingIcon = { Icon(ScrinIcons.Link, contentDescription = null) },
            singleLine = true,
            isError = problem == ConnectForm.Problem.TARGET_EMPTY || problem == ConnectForm.Problem.TARGET_INVALID,
            supportingText = {
                when (problem) {
                    ConnectForm.Problem.TARGET_EMPTY -> Text(stringResource(R.string.error_target_empty))
                    ConnectForm.Problem.TARGET_INVALID -> Text(stringResource(R.string.error_target_invalid))
                    else -> Text(stringResource(R.string.home_partner_id_hint))
                }
            },
            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Ascii, imeAction = ImeAction.Next, autoCorrectEnabled = false),
            modifier = Modifier.fillMaxWidth(),
        )
        OutlinedTextField(
            value = code,
            onValueChange = {
                code = TextFieldValue(Format.codeInput(it.text), TextRange(Format.codeCaret(it.text, it.selection.end)))
                problem = null
            },
            label = { Text(stringResource(R.string.home_code_field)) },
            singleLine = true,
            textStyle = CodeStyle,
            isError = problem == ConnectForm.Problem.CODE_INCOMPLETE,
            supportingText = { if (problem == ConnectForm.Problem.CODE_INCOMPLETE) Text(stringResource(R.string.error_code_incomplete)) },
            keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.Characters, keyboardType = KeyboardType.Ascii, imeAction = ImeAction.Go, autoCorrectEnabled = false),
            keyboardActions = KeyboardActions(onGo = { submit() }),
            modifier = Modifier.fillMaxWidth(),
        )
        Row(verticalAlignment = Alignment.CenterVertically) {
            Icon(ScrinIcons.Shield, contentDescription = null, tint = MaterialTheme.colorScheme.primary, modifier = Modifier.size(18.dp))
            Spacer(Modifier.width(8.dp))
            Text(stringResource(R.string.home_e2e_note), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.weight(1f))
        }
        FilledTonalButton(onClick = submit, modifier = Modifier.fillMaxWidth().heightIn(min = 52.dp)) {
            Text(stringResource(R.string.home_connect_action))
        }
    }
}
