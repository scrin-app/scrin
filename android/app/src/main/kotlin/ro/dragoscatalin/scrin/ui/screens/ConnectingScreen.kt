package ro.dragoscatalin.scrin.ui.screens

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import ro.dragoscatalin.scrin.R
import ro.dragoscatalin.scrin.core.SessionHub
import ro.dragoscatalin.scrin.core.SessionReducer
import ro.dragoscatalin.scrin.ffi.EndKind
import ro.dragoscatalin.scrin.ui.components.SasRow
import ro.dragoscatalin.scrin.ui.components.ScrinIcons
import ro.dragoscatalin.scrin.ui.components.ScrinTopBar
import ro.dragoscatalin.scrin.ui.components.SectionCard
import ro.dragoscatalin.scrin.ui.components.SectionTitle

@Composable
fun ConnectingScreen(hub: SessionHub, onBack: () -> Unit) {
    val ui by hub.ui.collectAsState()
    val step = SessionReducer.controllerStep(ui)
    val steps = listOf(R.string.step_connecting, R.string.step_pairing, R.string.step_waiting_host, R.string.step_active)
    Column(Modifier.fillMaxSize().safeDrawingPadding()) {
        ScrinTopBar(stringResource(R.string.connecting_title), onBack = onBack)
        Column(
            Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(horizontal = 20.dp),
            verticalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            val end = ui.ended
            if (end != null) {
                EndedCard(end.kind, onBack)
                return@Column
            }
            SectionCard {
                steps.forEachIndexed { i, res -> StepRow(index = i, label = stringResource(res), state = stepState(i, step)) }
            }
            ui.sas?.let { sas ->
                SectionCard(container = MaterialTheme.colorScheme.primaryContainer) {
                    SectionTitle(stringResource(R.string.sas_title))
                    Text(stringResource(R.string.sas_body_controller), style = MaterialTheme.typography.bodyMedium)
                    SasRow(sas)
                    Text(stringResource(R.string.sas_mismatch_hint), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
            }
            OutlinedButton(onClick = onBack, modifier = Modifier.fillMaxWidth().heightIn(min = 48.dp)) { Text(stringResource(R.string.action_cancel)) }
        }
    }
}

private enum class StepState { DONE, CURRENT, TODO }

private fun stepState(i: Int, current: Int) = when {
    i < current -> StepState.DONE
    i == current -> StepState.CURRENT
    else -> StepState.TODO
}

@Composable
private fun StepRow(index: Int, label: String, state: StepState) {
    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(14.dp), modifier = Modifier.heightIn(min = 40.dp)) {
        Box(Modifier.size(32.dp), contentAlignment = Alignment.Center) {
            when (state) {
                StepState.DONE -> Surface(shape = CircleShape, color = MaterialTheme.colorScheme.primary, modifier = Modifier.size(28.dp)) {
                    Box(contentAlignment = Alignment.Center) { Icon(ScrinIcons.Check, null, tint = MaterialTheme.colorScheme.onPrimary, modifier = Modifier.size(18.dp)) }
                }
                StepState.CURRENT -> CircularProgressIndicator(Modifier.size(26.dp), strokeWidth = 3.dp)
                StepState.TODO -> Surface(shape = CircleShape, color = MaterialTheme.colorScheme.surfaceContainerHigh, modifier = Modifier.size(28.dp)) {
                    Box(contentAlignment = Alignment.Center) { Text("${index + 1}", style = MaterialTheme.typography.labelMedium) }
                }
            }
        }
        Text(
            label,
            style = MaterialTheme.typography.bodyLarge,
            color = if (state == StepState.TODO) MaterialTheme.colorScheme.onSurfaceVariant else MaterialTheme.colorScheme.onSurface,
        )
    }
}

@Composable
fun EndedCard(kind: EndKind, onDone: () -> Unit) {
    SectionCard {
        SectionTitle(stringResource(R.string.ended_title))
        Text(stringResource(endReason(kind)), style = MaterialTheme.typography.bodyLarge)
        Button(onClick = onDone, modifier = Modifier.fillMaxWidth().heightIn(min = 48.dp)) { Text(stringResource(R.string.action_done)) }
    }
}

fun endReason(kind: EndKind): Int = when (kind) {
    EndKind.CANCELLED -> R.string.end_cancelled
    EndKind.REJECTED -> R.string.end_rejected
    EndKind.HOST_STOPPED -> R.string.end_host_stopped
    EndKind.REPORTED -> R.string.end_reported
    EndKind.PEER_ENDED -> R.string.end_peer_ended
    EndKind.DISCONNECTED -> R.string.end_disconnected
    EndKind.TIME_LIMIT -> R.string.end_time_limit
    EndKind.CONNECT_FAILED -> R.string.end_connect_failed
    EndKind.PAIRING_FAILED -> R.string.end_pairing_failed
    EndKind.TIMEOUT -> R.string.end_timeout
}
