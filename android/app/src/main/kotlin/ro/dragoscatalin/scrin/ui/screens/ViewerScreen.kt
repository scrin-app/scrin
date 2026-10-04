package ro.dragoscatalin.scrin.ui.screens

import android.view.SurfaceHolder
import android.view.SurfaceView
import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.FilledIconButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButtonDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.SegmentedButton
import androidx.compose.material3.SegmentedButtonDefaults
import androidx.compose.material3.SingleChoiceSegmentedButtonRow
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.input.pointer.positionChange
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import ro.dragoscatalin.scrin.R
import ro.dragoscatalin.scrin.core.SessionHub
import ro.dragoscatalin.scrin.ffi.RemoteInput
import ro.dragoscatalin.scrin.ffi.SessionPermission
import ro.dragoscatalin.scrin.media.VideoDecoder
import ro.dragoscatalin.scrin.ui.Format
import ro.dragoscatalin.scrin.ui.TouchMapper
import ro.dragoscatalin.scrin.ui.TouchMode
import ro.dragoscatalin.scrin.ui.components.ScrinIcons
import kotlin.math.abs

@Composable
fun ViewerScreen(hub: SessionHub, onClose: () -> Unit) {
    val ui by hub.ui.collectAsState()
    val mapper = remember { TouchMapper() }
    var mode by remember { mutableStateOf(TouchMode.TRACKPAD) }
    var keyboard by remember { mutableStateOf(false) }
    val canInput = SessionPermission.INPUT in ui.granted

    Box(Modifier.fillMaxSize().background(Color.Black)) {
        RemoteSurface(hub)
        Box(
            Modifier.fillMaxSize().pointerInput(canInput, mode) {
                if (!canInput) return@pointerInput
                awaitEachGesture {
                    val first = awaitFirstDown()
                    val w = size.width.toFloat()
                    val h = size.height.toFloat()
                    mapper.mode = mode
                    mapper.down(0, first.position.x, first.position.y, w, h).forEach(hub::sendInput)
                    var moved = 0f
                    var maxPointers = 1
                    var last = first.position
                    while (true) {
                        val ev = awaitPointerEvent()
                        val pressed = ev.changes.filter { it.pressed }
                        maxPointers = maxOf(maxPointers, pressed.size)
                        if (pressed.isEmpty()) break
                        val c = pressed.first()
                        val d = c.positionChange()
                        moved += abs(d.x) + abs(d.y)
                        if (pressed.size >= 2) {
                            mapper.scroll(d.y).forEach(hub::sendInput)
                        } else {
                            mapper.move(0, c.position.x, c.position.y, d.x, d.y, w, h).forEach(hub::sendInput)
                        }
                        last = c.position
                        ev.changes.forEach { it.consume() }
                    }
                    mapper.up(0, last.x, last.y, w, h).forEach(hub::sendInput)
                    if (moved < 12f) {
                        (if (maxPointers >= 2) mapper.twoFingerTap() else mapper.tap()).forEach(hub::sendInput)
                    }
                }
            },
        )
        if (ui.video == null) {
            Column(Modifier.align(Alignment.Center).padding(24.dp), horizontalAlignment = Alignment.CenterHorizontally) {
                Text(stringResource(R.string.viewer_waiting_video), color = Color.White, style = MaterialTheme.typography.titleMedium)
                Text(stringResource(R.string.viewer_waiting_video_body), color = Color.White.copy(alpha = 0.7f), style = MaterialTheme.typography.bodyMedium)
            }
        }
        ViewerToolbar(
            modifier = Modifier.align(Alignment.TopCenter).safeDrawingPadding().padding(12.dp),
            hub = hub,
            mode = mode,
            onMode = { mode = it },
            canInput = canInput,
            onKeyboard = { keyboard = !keyboard },
            onClose = onClose,
        )
        if (keyboard && canInput) {
            KeyboardBar(Modifier.align(Alignment.BottomCenter).safeDrawingPadding().padding(12.dp), onSend = { hub.sendInput(RemoteInput.Text(it)) })
        }
        ui.ended?.let { end ->
            Box(Modifier.fillMaxSize().background(Color.Black.copy(alpha = 0.6f)).padding(24.dp), contentAlignment = Alignment.Center) {
                EndedCard(end.kind, onClose)
            }
        }
    }
}

@Composable
private fun RemoteSurface(hub: SessionHub) {
    var decoder by remember { mutableStateOf<VideoDecoder?>(null) }
    DisposableEffect(Unit) {
        onDispose {
            hub.viewerSinks = null
            decoder?.release()
        }
    }
    AndroidView(
        modifier = Modifier.fillMaxSize(),
        factory = { ctx ->
            SurfaceView(ctx).apply {
                holder.addCallback(object : SurfaceHolder.Callback {
                    override fun surfaceCreated(h: SurfaceHolder) {
                        val d = VideoDecoder(h.surface) { hub.requestKeyframe() }
                        decoder = d
                        hub.viewerSinks = d
                        hub.ui.value.video?.let(d::onVideoConfig)
                    }
                    override fun surfaceChanged(h: SurfaceHolder, f: Int, w: Int, hh: Int) = Unit
                    override fun surfaceDestroyed(h: SurfaceHolder) {
                        hub.viewerSinks = null
                        decoder?.release()
                        decoder = null
                    }
                })
            }
        },
    )
}

@Composable
private fun ViewerToolbar(
    modifier: Modifier,
    hub: SessionHub,
    mode: TouchMode,
    onMode: (TouchMode) -> Unit,
    canInput: Boolean,
    onKeyboard: () -> Unit,
    onClose: () -> Unit,
) {
    val ui by hub.ui.collectAsState()
    Surface(modifier, shape = RoundedCornerShape(28.dp), color = MaterialTheme.colorScheme.surfaceContainerHigh.copy(alpha = 0.92f), tonalElevation = 3.dp) {
        Row(Modifier.padding(6.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
            SingleChoiceSegmentedButtonRow {
                SegmentedButton(
                    selected = mode == TouchMode.TRACKPAD,
                    onClick = { onMode(TouchMode.TRACKPAD) },
                    shape = SegmentedButtonDefaults.itemShape(0, 2),
                    enabled = canInput,
                    icon = { Icon(ScrinIcons.Mouse, null, Modifier.size(18.dp)) },
                ) { Text(stringResource(R.string.viewer_mode_trackpad)) }
                SegmentedButton(
                    selected = mode == TouchMode.DIRECT,
                    onClick = { onMode(TouchMode.DIRECT) },
                    shape = SegmentedButtonDefaults.itemShape(1, 2),
                    enabled = canInput,
                    icon = { Icon(ScrinIcons.Touch, null, Modifier.size(18.dp)) },
                ) { Text(stringResource(R.string.viewer_mode_direct)) }
            }
            FilledIconButton(onClick = onKeyboard, enabled = canInput, modifier = Modifier.size(48.dp)) {
                Icon(ScrinIcons.Keyboard, contentDescription = stringResource(R.string.viewer_keyboard))
            }
            ui.stats?.let { s ->
                Text(stringResource(R.string.viewer_rtt, s.rttMs.toInt()), style = MaterialTheme.typography.labelMedium)
            }
            FilledIconButton(
                onClick = onClose,
                modifier = Modifier.size(48.dp),
                colors = IconButtonDefaults.filledIconButtonColors(containerColor = MaterialTheme.colorScheme.error, contentColor = MaterialTheme.colorScheme.onError),
            ) { Icon(ScrinIcons.Close, contentDescription = stringResource(R.string.viewer_disconnect)) }
        }
    }
}

@Composable
private fun KeyboardBar(modifier: Modifier, onSend: (String) -> Unit) {
    var text by remember { mutableStateOf("") }
    val focus = remember { FocusRequester() }
    Surface(modifier.fillMaxWidth(), shape = RoundedCornerShape(24.dp), color = MaterialTheme.colorScheme.surfaceContainerHigh) {
        Row(Modifier.padding(8.dp), verticalAlignment = Alignment.CenterVertically) {
            OutlinedTextField(
                value = text,
                onValueChange = { text = it },
                singleLine = true,
                label = { Text(stringResource(R.string.viewer_type_here)) },
                keyboardOptions = KeyboardOptions(imeAction = ImeAction.Send),
                keyboardActions = androidx.compose.foundation.text.KeyboardActions(onSend = { if (text.isNotEmpty()) { onSend(text); text = "" } }),
                modifier = Modifier.weight(1f).focusRequester(focus),
            )
            TextButton(onClick = { if (text.isNotEmpty()) { onSend(text); text = "" } }) { Text(stringResource(R.string.action_send)) }
        }
    }
    DisposableEffect(Unit) {
        runCatching { focus.requestFocus() }
        onDispose { }
    }
}

/** Format a session timer for the host bar. */
fun sessionClock(ms: Long): String = Format.duration(ms)
