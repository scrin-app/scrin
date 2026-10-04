package ro.dragoscatalin.scrin.core

import ro.dragoscatalin.scrin.ffi.EndInfo
import ro.dragoscatalin.scrin.ffi.IncomingRequest
import ro.dragoscatalin.scrin.ffi.Notice
import ro.dragoscatalin.scrin.ffi.SasInfo
import ro.dragoscatalin.scrin.ffi.SessionPermission
import ro.dragoscatalin.scrin.ffi.SessionState
import ro.dragoscatalin.scrin.ffi.SessionStats
import ro.dragoscatalin.scrin.ffi.VideoConfigInfo

enum class Role { HOST, CONTROLLER }

/** Everything the UI shows about the current (or last) session. */
data class SessionUi(
    val role: Role? = null,
    val state: SessionState? = null,
    /** The host loop is accepting connections. */
    val listening: Boolean = false,
    val sas: SasInfo? = null,
    val request: IncomingRequest? = null,
    /** The last request, kept while the session runs (allowed set, controller name). */
    val lastRequest: IncomingRequest? = null,
    val granted: List<SessionPermission> = emptyList(),
    val asked: SessionPermission? = null,
    val stats: SessionStats? = null,
    val video: VideoConfigInfo? = null,
    val ended: EndInfo? = null,
    val notice: Notice? = null,
    val error: String? = null,
) {
    val active: Boolean get() = state == SessionState.ACTIVE
    val running: Boolean get() = state != null && state != SessionState.ENDED && state != SessionState.LISTENING
}

/** One listener callback, as data. */
sealed interface SessionEvent {
    data class State(val state: SessionState) : SessionEvent
    data class Sas(val sas: SasInfo) : SessionEvent
    data class Request(val request: IncomingRequest) : SessionEvent
    data class Permissions(val granted: List<SessionPermission>) : SessionEvent
    data class Asked(val permission: SessionPermission) : SessionEvent
    data class NoticeEv(val notice: Notice) : SessionEvent
    data class Stats(val stats: SessionStats) : SessionEvent
    data class Video(val config: VideoConfigInfo) : SessionEvent
    data class Ended(val end: EndInfo) : SessionEvent
    data class Error(val message: String) : SessionEvent
}

/** Pure reducer from listener events to [SessionUi]. */
object SessionReducer {
    fun controllerStarted(prev: SessionUi): SessionUi =
        SessionUi(role = Role.CONTROLLER, state = SessionState.CONNECTING, listening = prev.listening)

    fun reduce(ui: SessionUi, e: SessionEvent): SessionUi = when (e) {
        is SessionEvent.State -> onState(ui, e.state)
        is SessionEvent.Sas -> ui.copy(sas = e.sas)
        is SessionEvent.Request -> ui.copy(request = e.request, lastRequest = e.request, state = SessionState.INCOMING_REQUEST)
        is SessionEvent.Permissions -> ui.copy(granted = e.granted, asked = ui.asked?.takeUnless { it in e.granted })
        is SessionEvent.Asked -> ui.copy(asked = e.permission)
        is SessionEvent.NoticeEv -> ui.copy(notice = e.notice)
        is SessionEvent.Stats -> ui.copy(stats = e.stats)
        is SessionEvent.Video -> ui.copy(video = e.config)
        is SessionEvent.Ended -> ui.copy(state = SessionState.ENDED, ended = e.end, request = null, asked = null, granted = emptyList())
        is SessionEvent.Error -> ui.copy(error = e.message)
    }

    private fun onState(ui: SessionUi, s: SessionState): SessionUi = when {
        s == SessionState.LISTENING -> ui.copy(listening = true)
        // A new incoming connection on the host loop starts a fresh host session.
        s == SessionState.PAIRING && (ui.role == null || ui.state == null || ui.state == SessionState.ENDED) ->
            SessionUi(role = Role.HOST, state = s, listening = ui.listening)
        s == SessionState.ACTIVE -> ui.copy(state = s, request = null)
        else -> ui.copy(state = s)
    }

    /** Progress step on the controller's connecting screen: 0..3. */
    fun controllerStep(ui: SessionUi): Int = when (ui.state) {
        SessionState.CONNECTING, null -> 0
        SessionState.PAIRING -> 1
        SessionState.AWAITING_ACCEPT -> 2
        SessionState.ACTIVE -> 3
        else -> 0
    }
}
