package ro.dragoscatalin.scrin

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import ro.dragoscatalin.scrin.core.Role
import ro.dragoscatalin.scrin.core.SessionEvent
import ro.dragoscatalin.scrin.core.SessionReducer
import ro.dragoscatalin.scrin.core.SessionUi
import ro.dragoscatalin.scrin.ffi.EndInfo
import ro.dragoscatalin.scrin.ffi.EndKind
import ro.dragoscatalin.scrin.ffi.IncomingRequest
import ro.dragoscatalin.scrin.ffi.SasInfo
import ro.dragoscatalin.scrin.ffi.SessionPermission
import ro.dragoscatalin.scrin.ffi.SessionState

class SessionReducerTest {
    private fun run(start: SessionUi, vararg events: SessionEvent) = events.fold(start) { ui, e -> SessionReducer.reduce(ui, e) }

    private val request = IncomingRequest(
        peerId = "00".repeat(32), peerFingerprint = "aaaa-bbbb-cccc-dddd", verified = false, unattended = false,
        controllerName = "laptop", requested = listOf(SessionPermission.VIEW), allowed = listOf(SessionPermission.VIEW, SessionPermission.INPUT),
        acceptInMs = 5_000uL, expiresInMs = 60_000uL,
    )

    @Test fun controllerHappyPathAdvancesTheStepper() {
        var ui = SessionReducer.controllerStarted(SessionUi(listening = true))
        assertEquals(Role.CONTROLLER, ui.role)
        assertTrue(ui.listening)
        assertEquals(0, SessionReducer.controllerStep(ui))
        ui = run(ui, SessionEvent.State(SessionState.PAIRING))
        assertEquals(1, SessionReducer.controllerStep(ui))
        assertEquals(Role.CONTROLLER, ui.role)
        ui = run(ui, SessionEvent.Sas(SasInfo(listOf("🐶"), listOf("dog"))), SessionEvent.State(SessionState.AWAITING_ACCEPT))
        assertEquals(2, SessionReducer.controllerStep(ui))
        ui = run(ui, SessionEvent.State(SessionState.ACTIVE), SessionEvent.Permissions(listOf(SessionPermission.VIEW)))
        assertTrue(ui.active)
        assertEquals(3, SessionReducer.controllerStep(ui))
    }

    @Test fun hostPairingStartsAFreshHostSessionAndKeepsListening() {
        val idle = run(SessionUi(), SessionEvent.State(SessionState.LISTENING))
        assertTrue(idle.listening)
        assertNull(idle.role)
        val ui = run(idle, SessionEvent.State(SessionState.PAIRING), SessionEvent.Request(request))
        assertEquals(Role.HOST, ui.role)
        assertTrue(ui.listening)
        assertEquals(SessionState.INCOMING_REQUEST, ui.state)
        assertEquals(request, ui.request)
        val active = run(ui, SessionEvent.State(SessionState.ACTIVE))
        assertNull(active.request)
        assertEquals(request, active.lastRequest)
    }

    @Test fun endClearsLiveStateButKeepsTheReason() {
        val ui = run(
            SessionUi(role = Role.HOST, state = SessionState.ACTIVE, granted = listOf(SessionPermission.VIEW)),
            SessionEvent.Asked(SessionPermission.INPUT),
            SessionEvent.Ended(EndInfo(EndKind.PEER_ENDED, "", 1_000uL)),
        )
        assertEquals(SessionState.ENDED, ui.state)
        assertEquals(EndKind.PEER_ENDED, ui.ended?.kind)
        assertTrue(ui.granted.isEmpty())
        assertNull(ui.asked)
        assertFalse(ui.running)
    }

    @Test fun grantingTheAskedPermissionClearsThePrompt() {
        val ui = run(SessionUi(role = Role.HOST, state = SessionState.ACTIVE), SessionEvent.Asked(SessionPermission.INPUT))
        assertEquals(SessionPermission.INPUT, ui.asked)
        val after = run(ui, SessionEvent.Permissions(listOf(SessionPermission.VIEW, SessionPermission.INPUT)))
        assertNull(after.asked)
    }
}
