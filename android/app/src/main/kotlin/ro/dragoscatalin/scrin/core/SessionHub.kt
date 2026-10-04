package ro.dragoscatalin.scrin.core

import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import ro.dragoscatalin.scrin.ffi.EndInfo
import ro.dragoscatalin.scrin.ffi.IncomingRequest
import ro.dragoscatalin.scrin.ffi.Notice
import ro.dragoscatalin.scrin.ffi.RemoteInput
import ro.dragoscatalin.scrin.ffi.SasInfo
import ro.dragoscatalin.scrin.ffi.SessionListener
import ro.dragoscatalin.scrin.ffi.SessionPermission
import ro.dragoscatalin.scrin.ffi.SessionState
import ro.dragoscatalin.scrin.ffi.SessionStats
import ro.dragoscatalin.scrin.ffi.VideoConfigInfo
import ro.dragoscatalin.scrin.ui.ConnectForm

/** The host's current one-time code and when it lapses (monotonic millis). */
data class CodeState(val display: String, val expiresAtMs: Long, val totalMs: Long)

/** Media/input endpoints owned by Android services; set while they run. */
interface MediaSinks {
    fun onVideoConfig(config: VideoConfigInfo) {}
    fun onVideoFrame(data: ByteArray, keyframe: Boolean) {}
    fun onKeyframeRequest() {}
    fun onInput(event: RemoteInput) {}
}

/**
 * App-scoped bridge between the native core and the UI: implements the core's
 * [SessionListener], reduces callbacks into [SessionUi], and runs every core call
 * off the main thread.
 */
class SessionHub(
    private val core: CoreApi,
    private val scope: CoroutineScope,
    private val io: CoroutineDispatcher = Dispatchers.IO,
    private val clock: () -> Long = { android.os.SystemClock.elapsedRealtime() },
) : SessionListener {
    private val _ui = MutableStateFlow(SessionUi())
    val ui: StateFlow<SessionUi> = _ui.asStateFlow()

    private val _code = MutableStateFlow<CodeState?>(null)
    val code: StateFlow<CodeState?> = _code.asStateFlow()

    private val _ticket = MutableStateFlow<String?>(null)
    val ticket: StateFlow<String?> = _ticket.asStateFlow()

    val deviceId: String get() = core.deviceId
    val fingerprint: String get() = core.fingerprint

    @Volatile var hostSinks: MediaSinks? = null
    @Volatile var viewerSinks: MediaSinks? = null

    private fun dispatch(e: SessionEvent) = _ui.update { SessionReducer.reduce(it, e) }

    private fun call(block: CoreApi.() -> Unit) {
        scope.launch(io) {
            runCatching { core.block() }.onFailure { dispatch(SessionEvent.Error(it.message ?: it.toString())) }
        }
    }

    /** Fresh code and (re)start of the host loop, so the new code is the one that pairs. */
    fun refreshCode() {
        scope.launch(io) {
            runCatching {
                val info = core.newCode()
                val total = info.expiresInS.toLong() * 1000
                _code.value = CodeState(info.display, clock() + total, total)
                if (_ticket.value == null) _ticket.value = core.hostInfo().ticket
                core.startHost(this@SessionHub)
            }.onFailure { dispatch(SessionEvent.Error(it.message ?: it.toString())) }
        }
    }

    fun codeExpired(now: Long = clock()): Boolean = _code.value?.let { now >= it.expiresAtMs } ?: true

    /** Returns the form problem, or `null` when the connect attempt started. */
    fun connect(target: String, code: String): ConnectForm.Problem? {
        ConnectForm.validate(target, code)?.let { return it }
        _ui.update { SessionReducer.controllerStarted(it) }
        call { connect(ConnectForm.normalizeTarget(target), code, this@SessionHub) }
        return null
    }

    fun accept(permissions: List<SessionPermission>) = call { hostAccept(permissions) }
    fun reject() = call { hostReject() }
    fun grant(p: SessionPermission) = call { grant(p) }
    fun revoke(p: SessionPermission) = call { revoke(p) }
    fun requestPermission(p: SessionPermission) = call { requestPermission(p) }
    fun end() = call { endSession() }
    fun stopAndReport() = call { stopAndReport() }
    fun sendVideoConfig(c: VideoConfigInfo) = call { sendVideoConfig(c) }

    /** Hot path: called from the encoder thread, already off main. */
    fun sendVideoFrame(data: ByteArray, keyframe: Boolean) {
        runCatching { core.sendVideoFrame(data, keyframe) }
    }

    /** Hot path: touch events. */
    fun sendInput(e: RemoteInput) = call { sendInput(e) }
    fun requestKeyframe() = call { requestKeyframe() }

    fun dismissEnded() = _ui.update { it.copy(ended = null, notice = null, error = null, state = if (it.state == SessionState.ENDED) null else it.state, role = if (it.state == SessionState.ENDED) null else it.role) }
    fun dismissAsked() = _ui.update { it.copy(asked = null) }
    fun dismissError() = _ui.update { it.copy(error = null) }

    fun trusted() = core.listTrusted()
    fun removeTrusted(id: String) = core.removeTrusted(id)

    // ---- SessionListener (core worker threads) ----
    override fun onState(state: SessionState) = dispatch(SessionEvent.State(state))
    override fun onSas(sas: SasInfo) = dispatch(SessionEvent.Sas(sas))
    override fun onIncomingRequest(request: IncomingRequest) = dispatch(SessionEvent.Request(request))
    override fun onPermissions(granted: List<SessionPermission>) = dispatch(SessionEvent.Permissions(granted))
    override fun onPermissionAsked(permission: SessionPermission) = dispatch(SessionEvent.Asked(permission))
    override fun onNotice(notice: Notice) = dispatch(SessionEvent.NoticeEv(notice))
    override fun onStats(stats: SessionStats) = dispatch(SessionEvent.Stats(stats))
    override fun onVideoConfig(config: VideoConfigInfo) {
        dispatch(SessionEvent.Video(config))
        viewerSinks?.onVideoConfig(config)
    }
    override fun onVideoFrame(data: ByteArray, keyframe: Boolean) { viewerSinks?.onVideoFrame(data, keyframe) }
    override fun onKeyframeRequest() { hostSinks?.onKeyframeRequest() }
    override fun onInput(event: RemoteInput) { hostSinks?.onInput(event) }
    override fun onEnded(end: EndInfo) = dispatch(SessionEvent.Ended(end))
    override fun onError(message: String) = dispatch(SessionEvent.Error(message))
}
