package ro.dragoscatalin.scrin.ui.screens

import android.view.SurfaceHolder
import android.view.SurfaceView
import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.calculateCentroid
import androidx.compose.foundation.gestures.calculatePan
import androidx.compose.foundation.gestures.calculateZoom
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.FilledIconButton
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.FilterChip
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
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clipToBounds
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.pointer.PointerEventPass
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.input.pointer.positionChange
import androidx.compose.ui.layout.layout
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalViewConfiguration
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.unit.Constraints
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import ro.dragoscatalin.scrin.R
import ro.dragoscatalin.scrin.core.SessionHub
import ro.dragoscatalin.scrin.ffi.RemoteInput
import ro.dragoscatalin.scrin.ffi.SessionPermission
import ro.dragoscatalin.scrin.ffi.SessionStats
import ro.dragoscatalin.scrin.media.VideoDecoder
import ro.dragoscatalin.scrin.ui.Format
import ro.dragoscatalin.scrin.ui.Keys
import ro.dragoscatalin.scrin.ui.TouchMapper
import ro.dragoscatalin.scrin.ui.TouchMode
import ro.dragoscatalin.scrin.ui.VRect
import ro.dragoscatalin.scrin.ui.Zoom
import ro.dragoscatalin.scrin.ui.components.ScrinIcons
import ro.dragoscatalin.scrin.ui.letterbox
import java.util.Locale
import kotlin.math.abs
import kotlin.math.roundToInt

@Composable
fun ViewerScreen(hub: SessionHub, onClose: () -> Unit) {
    val ui by hub.ui.collectAsState()
    var mode by remember { mutableStateOf(TouchMode.DIRECT) }
    var keyboard by remember { mutableStateOf(false) }
    var videoSize by remember { mutableStateOf(ui.video?.let { it.width.toInt() to it.height.toInt() }) }
    var viewport by remember { mutableStateOf(0 to 0) }
    var zoom by remember { mutableStateOf(Zoom()) }
    val canInput = SessionPermission.INPUT in ui.granted

    Box(Modifier.fillMaxSize().background(Color.Black)) {
        Box(Modifier.fillMaxSize().safeDrawingPadding().imePadding().clipToBounds().onSizeChanged { viewport = it.width to it.height }) {
            val ratio = videoSize?.let { (w, h) -> if (w > 0 && h > 0) w.toFloat() / h else null } ?: (16f / 9f)
            // Letterboxed base rectangle; the zoom scales and pans the picture inside the viewport.
            val base = letterbox(viewport.first.toFloat(), viewport.second.toFloat(), ratio)
            val rect = zoom.rect(base)
            Box(Modifier.placeAt(rect)) {
                RemoteSurface(hub, visiblePart(rect, viewport.first, viewport.second)) { w, h -> videoSize = w to h }
            }
            TouchLayer(hub, mode, canInput, base, zoom) { zoom = it }
        }
        if (ui.video == null) {
            Column(Modifier.align(Alignment.Center).padding(24.dp), horizontalAlignment = Alignment.CenterHorizontally) {
                Text(stringResource(R.string.viewer_waiting_video), color = Color.White, style = MaterialTheme.typography.titleMedium)
                Text(stringResource(R.string.viewer_waiting_video_body), color = Color.White.copy(alpha = 0.7f), style = MaterialTheme.typography.bodyMedium)
            }
        }
        ViewerToolbar(
            modifier = Modifier.align(Alignment.TopCenter).safeDrawingPadding().padding(12.dp),
            mode = mode,
            onMode = { mode = it },
            canInput = canInput,
            onKeyboard = { keyboard = !keyboard },
            zoomed = zoom.zoomed,
            onFit = { zoom = Zoom() },
            onClose = onClose,
        )
        ui.stats?.let { StatsChip(it, Modifier.align(Alignment.TopEnd).safeDrawingPadding().padding(top = 76.dp, end = 12.dp)) }
        if (keyboard && canInput) {
            KeyboardPanel(Modifier.align(Alignment.BottomCenter).safeDrawingPadding().imePadding().padding(8.dp), hub::sendInputs)
        }
        ui.ended?.let { end ->
            Box(Modifier.fillMaxSize().background(Color.Black.copy(alpha = 0.6f)).padding(24.dp), contentAlignment = Alignment.Center) {
                EndedCard(end.kind, onClose)
            }
        }
    }
}

/** Lays the content out at `r` (viewport pixels), even when `r` is larger than the viewport. */
private fun Modifier.placeAt(r: VRect): Modifier = layout { m, c ->
    val w = r.width.roundToInt().coerceAtLeast(1)
    val h = r.height.roundToInt().coerceAtLeast(1)
    val p = m.measure(Constraints.fixed(w, h))
    layout(c.maxWidth, c.maxHeight) { p.place(r.left.roundToInt(), r.top.roundToInt()) }
}

private enum class Multi { NONE, UNDECIDED, PINCH, SCROLL }

/** The part of `r` inside the viewport, in `r`'s own coordinates (SurfaceView clip bounds). */
private fun visiblePart(r: VRect, vw: Int, vh: Int) = android.graphics.Rect(
    (-r.left).roundToInt().coerceAtLeast(0),
    (-r.top).roundToInt().coerceAtLeast(0),
    (vw - r.left).roundToInt().coerceAtMost(r.width.roundToInt()),
    (vh - r.top).roundToInt().coerceAtMost(r.height.roundToInt()),
)

/**
 * Gestures over the whole viewport → mouse events (see [TouchMapper]) relative to the
 * zoomed picture. Two fingers: pinch = zoom/pan the picture locally (also any two-finger drag
 * while zoomed in), a two-finger drag at 1× = wheel, a two-finger tap = right click.
 */
@Composable
private fun TouchLayer(hub: SessionHub, mode: TouchMode, canInput: Boolean, base: VRect, zoom: Zoom, onZoom: (Zoom) -> Unit) {
    val mapper = remember { TouchMapper() }
    val scope = rememberCoroutineScope()
    val longPressMs = LocalViewConfiguration.current.longPressTimeoutMillis
    val baseNow by rememberUpdatedState(base)
    val zoomNow by rememberUpdatedState(zoom)
    val setZoom by rememberUpdatedState(onZoom)
    Box(
        Modifier.fillMaxSize().pointerInput(canInput, mode) {
            mapper.mode = mode
            fun send(events: List<RemoteInput>) { if (canInput) hub.sendInputs(events) }
            awaitEachGesture {
                val first = awaitFirstDown(requireUnconsumed = false)
                var r = zoomNow.rect(baseNow)
                send(mapper.down(first.position.x - r.left, first.position.y - r.top, r.width, r.height))
                var timer: Job? = scope.launch {
                    delay(longPressMs)
                    if (mapper.longPressPending) send(mapper.longPress())
                }
                var multi = Multi.NONE
                var zoomAcc = 1f
                var panAcc = Offset.Zero
                var last = first.position
                while (true) {
                    val ev = awaitPointerEvent(PointerEventPass.Main)
                    val pressed = ev.changes.filter { it.pressed }
                    if (pressed.isEmpty()) break
                    if (pressed.size >= 2 && multi == Multi.NONE) {
                        multi = Multi.UNDECIDED
                        timer?.cancel()
                        timer = null
                        send(mapper.cancel())
                    }
                    if (multi != Multi.NONE) {
                        val z = ev.calculateZoom()
                        val pan = ev.calculatePan()
                        if (multi == Multi.UNDECIDED) {
                            zoomAcc *= z
                            panAcc += pan
                            if (abs(zoomAcc - 1f) > PINCH_THRESHOLD) {
                                multi = Multi.PINCH
                            } else if (panAcc.getDistance() > viewConfiguration.touchSlop) {
                                multi = if (zoomNow.zoomed) Multi.PINCH else Multi.SCROLL
                            }
                        }
                        val c = ev.calculateCentroid(useCurrent = true)
                        when (multi) {
                            Multi.PINCH -> if (c != Offset.Unspecified) setZoom(zoomNow.transformed(baseNow, c.x, c.y, z, pan.x, pan.y))
                            Multi.SCROLL -> send(mapper.scroll(pan.y))
                            Multi.NONE, Multi.UNDECIDED -> Unit
                        }
                    } else {
                        val c = pressed.first()
                        val d = c.positionChange()
                        r = zoomNow.rect(baseNow)
                        if (abs(d.x) + abs(d.y) > 0f) {
                            send(mapper.move(c.position.x - r.left, c.position.y - r.top, d.x, d.y, r.width, r.height))
                        }
                        last = c.position
                    }
                    ev.changes.forEach { it.consume() }
                }
                timer?.cancel()
                when (multi) {
                    Multi.UNDECIDED -> send(mapper.twoFingerTap())
                    Multi.PINCH -> if (!zoomNow.zoomed) setZoom(Zoom())
                    Multi.SCROLL -> Unit
                    Multi.NONE -> {
                        r = zoomNow.rect(baseNow)
                        send(mapper.up(last.x - r.left, last.y - r.top, r.width, r.height))
                    }
                }
            }
        },
    )
}

private const val PINCH_THRESHOLD = 0.08f

@Composable
private fun RemoteSurface(hub: SessionHub, clip: android.graphics.Rect, onSize: (Int, Int) -> Unit) {
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
                        val d = VideoDecoder(h.surface, onNeedKeyframe = { hub.requestKeyframe() }) { w, hh -> post { onSize(w, hh) } }
                        decoder = d
                        hub.ui.value.video?.let(d::onVideoConfig)
                        hub.viewerSinks = d
                        hub.requestKeyframe()
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
        // A SurfaceView punches through the window: Compose clipping does not apply, view clip
        // bounds do (API 33+), so a zoomed picture stays inside the viewer.
        update = { v -> if (v.clipBounds != clip) v.clipBounds = clip },
    )
}

@Composable
private fun ViewerToolbar(
    modifier: Modifier,
    mode: TouchMode,
    onMode: (TouchMode) -> Unit,
    canInput: Boolean,
    onKeyboard: () -> Unit,
    zoomed: Boolean,
    onFit: () -> Unit,
    onClose: () -> Unit,
) {
    Surface(modifier, shape = RoundedCornerShape(28.dp), color = MaterialTheme.colorScheme.surfaceContainerHigh.copy(alpha = 0.92f), tonalElevation = 3.dp) {
        Row(Modifier.padding(6.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
            SingleChoiceSegmentedButtonRow {
                SegmentedButton(
                    selected = mode == TouchMode.DIRECT,
                    onClick = { onMode(TouchMode.DIRECT) },
                    shape = SegmentedButtonDefaults.itemShape(0, 2),
                    enabled = canInput,
                    icon = { Icon(ScrinIcons.Touch, null, Modifier.size(18.dp)) },
                ) { Text(stringResource(R.string.viewer_mode_direct)) }
                SegmentedButton(
                    selected = mode == TouchMode.TRACKPAD,
                    onClick = { onMode(TouchMode.TRACKPAD) },
                    shape = SegmentedButtonDefaults.itemShape(1, 2),
                    enabled = canInput,
                    icon = { Icon(ScrinIcons.Mouse, null, Modifier.size(18.dp)) },
                ) { Text(stringResource(R.string.viewer_mode_trackpad)) }
            }
            FilledIconButton(onClick = onKeyboard, enabled = canInput, modifier = Modifier.size(48.dp)) {
                Icon(ScrinIcons.Keyboard, contentDescription = stringResource(R.string.viewer_keyboard))
            }
            if (zoomed) {
                FilledTonalButton(onClick = onFit, modifier = Modifier.heightIn(min = 48.dp)) { Text(stringResource(R.string.viewer_zoom_fit)) }
            }
            FilledIconButton(
                onClick = onClose,
                modifier = Modifier.size(48.dp),
                colors = IconButtonDefaults.filledIconButtonColors(containerColor = MaterialTheme.colorScheme.error, contentColor = MaterialTheme.colorScheme.onError),
            ) { Icon(ScrinIcons.Close, contentDescription = stringResource(R.string.viewer_disconnect)) }
        }
    }
}

/** fps · RTT · bitrate, updated once a second from the core's stats. */
@Composable
private fun StatsChip(s: SessionStats, modifier: Modifier) {
    val mbps = String.format(Locale.ROOT, "%.1f", s.bitrateBps.toDouble() / 1_000_000)
    val fps = String.format(Locale.ROOT, "%.0f", s.fps)
    val text = stringResource(R.string.viewer_stats, fps, s.rttMs.toInt(), mbps)
    val path = stringResource(if (s.direct) R.string.viewer_path_direct else R.string.viewer_path_relay)
    Surface(
        modifier.semantics { liveRegion = LiveRegionMode.Polite; contentDescription = "$text, $path" },
        shape = RoundedCornerShape(12.dp),
        color = Color.Black.copy(alpha = 0.6f),
    ) {
        Text("$text · $path", Modifier.padding(horizontal = 10.dp, vertical = 6.dp), color = Color.White, style = MaterialTheme.typography.labelMedium)
    }
}

/** Text field that sends typed text, plus Esc/Tab/arrows/Win/Ctrl+Alt+Del and sticky Ctrl/Alt. */
@Composable
private fun KeyboardPanel(modifier: Modifier, send: (List<RemoteInput>) -> Unit) {
    var text by remember { mutableStateOf("") }
    var ctrl by remember { mutableStateOf(false) }
    var alt by remember { mutableStateOf(false) }
    val focus = remember { FocusRequester() }
    val mods = (if (ctrl) Keys.MOD_CTRL else 0u) or (if (alt) Keys.MOD_ALT else 0u)
    fun key(usage: UInt) {
        send(Keys.press(usage, mods))
        ctrl = false
        alt = false
    }
    fun submit() {
        if (text.isEmpty()) return
        if (mods != 0u && text.length == 1 && text[0].isLetter()) {
            // Ctrl/Alt + letter is a shortcut: send the physical key (HID a = 0x04).
            key(0x04u + (text[0].lowercaseChar() - 'a').toUInt())
        } else {
            send(listOf(RemoteInput.Text(text)))
        }
        text = ""
    }
    Surface(modifier.fillMaxWidth().widthIn(max = 720.dp), shape = RoundedCornerShape(24.dp), color = MaterialTheme.colorScheme.surfaceContainerHigh) {
        Column(Modifier.padding(8.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
            Row(Modifier.horizontalScroll(rememberScrollState()), horizontalArrangement = Arrangement.spacedBy(6.dp), verticalAlignment = Alignment.CenterVertically) {
                SpecialKey(R.string.key_esc) { key(Keys.ESC) }
                SpecialKey(R.string.key_tab) { key(Keys.TAB) }
                FilterChip(selected = ctrl, onClick = { ctrl = !ctrl }, label = { Text(stringResource(R.string.key_ctrl)) }, modifier = Modifier.heightIn(min = 48.dp))
                FilterChip(selected = alt, onClick = { alt = !alt }, label = { Text(stringResource(R.string.key_alt)) }, modifier = Modifier.heightIn(min = 48.dp))
                SpecialKey(R.string.key_win) { send(Keys.tapModifier(Keys.WIN)) }
                SpecialKey(R.string.key_left) { key(Keys.LEFT) }
                SpecialKey(R.string.key_up) { key(Keys.UP) }
                SpecialKey(R.string.key_down) { key(Keys.DOWN) }
                SpecialKey(R.string.key_right) { key(Keys.RIGHT) }
                SpecialKey(R.string.key_backspace) { key(Keys.BACKSPACE) }
                SpecialKey(R.string.key_enter) { key(Keys.ENTER) }
                SpecialKey(R.string.key_ctrl_alt_del) { send(Keys.ctrlAltDel()) }
            }
            Row(verticalAlignment = Alignment.CenterVertically) {
                OutlinedTextField(
                    value = text,
                    onValueChange = { text = it },
                    singleLine = true,
                    label = { Text(stringResource(R.string.viewer_type_here)) },
                    keyboardOptions = KeyboardOptions(imeAction = ImeAction.Send, capitalization = KeyboardCapitalization.None, autoCorrectEnabled = false),
                    keyboardActions = KeyboardActions(onSend = { submit() }),
                    modifier = Modifier.weight(1f).focusRequester(focus),
                )
                TextButton(onClick = { submit() }, modifier = Modifier.heightIn(min = 48.dp)) { Text(stringResource(R.string.action_send)) }
            }
        }
    }
    LaunchedEffect(Unit) { runCatching { focus.requestFocus() } }
}

@Composable
private fun SpecialKey(label: Int, onClick: () -> Unit) {
    FilledTonalButton(onClick = onClick, modifier = Modifier.heightIn(min = 48.dp)) { Text(stringResource(label)) }
}

/** Format a session timer for the host bar. */
fun sessionClock(ms: Long): String = Format.duration(ms)
