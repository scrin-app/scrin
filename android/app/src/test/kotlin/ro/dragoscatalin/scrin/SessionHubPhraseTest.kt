package ro.dragoscatalin.scrin

import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.StandardTestDispatcher
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.advanceUntilIdle
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import ro.dragoscatalin.scrin.core.CoreApi
import ro.dragoscatalin.scrin.core.SessionHub
import ro.dragoscatalin.scrin.ffi.CodeInfo
import ro.dragoscatalin.scrin.ffi.EndInfo
import ro.dragoscatalin.scrin.ffi.EndKind
import ro.dragoscatalin.scrin.ffi.HostInfo
import ro.dragoscatalin.scrin.ffi.PassphraseInfo
import ro.dragoscatalin.scrin.ffi.RemoteInput
import ro.dragoscatalin.scrin.ffi.SessionListener
import ro.dragoscatalin.scrin.ffi.SessionPermission
import ro.dragoscatalin.scrin.ffi.SessionState
import ro.dragoscatalin.scrin.ffi.TrustedDevice
import ro.dragoscatalin.scrin.ffi.VideoConfigInfo

/** Records the core calls the passphrase logic makes, in order. */
private class FakeCore : CoreApi {
    val calls = mutableListOf<String>()
    var phraseFails = false
    private var n = 0
    override val deviceId = "00".repeat(32)
    override val fingerprint = "aaaa-bbbb-cccc-dddd"
    override fun newCode(): CodeInfo { calls += "newCode"; return CodeInfo("ABCD-EFG${n++}", 600u) }
    override fun newPassphrase(lang: String): PassphraseInfo {
        calls += "newPassphrase:$lang"
        if (phraseFails) error("a passphrase needs a server")
        return PassphraseInfo("apa bec cer drum ${n++}x", 600u)
    }
    override fun hostInfo() = HostInfo("scrin:" + "ab".repeat(32), deviceId, fingerprint, null)
    override fun startHost(listener: SessionListener) { calls += "startHost" }
    override fun stopHost() {}
    override fun connect(target: String, code: String, listener: SessionListener) { calls += "connect:$target|$code" }
    override fun hostAccept(permissions: List<SessionPermission>) {}
    override fun hostReject() {}
    override fun grant(permission: SessionPermission) {}
    override fun revoke(permission: SessionPermission) {}
    override fun requestPermission(permission: SessionPermission) {}
    override fun sendInput(event: RemoteInput) {}
    override fun sendVideoConfig(config: VideoConfigInfo) {}
    override fun sendVideoFrame(data: ByteArray, keyframe: Boolean) {}
    override fun requestKeyframe() {}
    override fun endSession() {}
    override fun stopAndReport() {}
    override fun listTrusted(): List<TrustedDevice> = emptyList()
    override fun removeTrusted(deviceId: String) = false
}

@OptIn(ExperimentalCoroutinesApi::class)
class SessionHubPhraseTest {
    private val dispatcher = StandardTestDispatcher()
    private val scope = TestScope(dispatcher)
    private val core = FakeCore()
    private var now = 1_000_000L

    private fun hub(server: Boolean = true) = SessionHub(core, scope, dispatcher, { now }, serverConfigured = server)

    @Test fun withoutAServerTheWordsStayOff() {
        val hub = hub(server = false)
        hub.refreshCode()
        hub.enablePhrase("en")
        scope.advanceUntilIdle()
        assertFalse(hub.phraseOn.value)
        assertNull(hub.phrase.value)
        assertFalse(core.calls.any { it.startsWith("newPassphrase") })
    }

    @Test fun wordsArmAfterTheCodeAndFollowEveryNewCode() {
        val hub = hub()
        hub.refreshCode()
        scope.advanceUntilIdle()
        hub.enablePhrase("ro-RO")
        scope.advanceUntilIdle()
        assertEquals(listOf("newCode", "startHost", "newPassphrase:ro-RO"), core.calls)
        val first = hub.phrase.value!!
        assertEquals(5, first.words.size)
        assertEquals(now + 600_000, first.expiresAtMs)

        core.calls.clear()
        hub.refreshCode()
        scope.advanceUntilIdle()
        // A new code drops the old words in the core: the phrase must be re-armed after it.
        assertEquals(listOf("newCode", "startHost", "newPassphrase:ro-RO"), core.calls)
        assertTrue(hub.phrase.value!!.words != first.words)
    }

    @Test fun renewsFifteenSecondsBeforeTheLocatorLapses() {
        val hub = hub()
        hub.refreshCode()
        hub.enablePhrase("en")
        scope.advanceUntilIdle()
        val expires = hub.phrase.value!!.expiresAtMs
        core.calls.clear()

        now = expires - SessionHub.PHRASE_RENEW_EARLY_MS - 1
        hub.phraseTick(now)
        scope.advanceUntilIdle()
        assertTrue(core.calls.isEmpty())

        now = expires - SessionHub.PHRASE_RENEW_EARLY_MS
        hub.phraseTick(now)
        scope.advanceUntilIdle()
        assertEquals(listOf("newPassphrase:en"), core.calls)
        assertEquals(now + 600_000, hub.phrase.value!!.expiresAtMs)
    }

    @Test fun aFailureIsShownAndRetriedLater() {
        val hub = hub()
        core.phraseFails = true
        hub.refreshCode()
        hub.enablePhrase("en")
        scope.advanceUntilIdle()
        assertTrue(hub.phraseFailed.value)
        assertNull(hub.phrase.value)
        assertTrue(hub.ui.value.error == null)
        core.calls.clear()

        hub.phraseTick(now + SessionHub.PHRASE_RETRY_MS - 1)
        scope.advanceUntilIdle()
        assertTrue(core.calls.isEmpty())

        core.phraseFails = false
        now += SessionHub.PHRASE_RETRY_MS
        hub.phraseTick(now)
        scope.advanceUntilIdle()
        assertEquals(listOf("newPassphrase:en"), core.calls)
        assertFalse(hub.phraseFailed.value)
        assertEquals(5, hub.phrase.value!!.words.size)
    }

    @Test fun registrationRetriesWordsThatFailedBeforeIt() {
        val hub = hub()
        core.phraseFails = true
        hub.refreshCode()
        hub.enablePhrase("en")
        scope.advanceUntilIdle()
        core.phraseFails = false
        hub.onRegistered("123456789")
        scope.advanceUntilIdle()
        assertEquals(5, hub.phrase.value!!.words.size)
    }

    @Test fun turningOffDisarmsTheWordsWithAFreshCode() {
        val hub = hub()
        hub.refreshCode()
        hub.enablePhrase("en")
        scope.advanceUntilIdle()
        core.calls.clear()
        hub.disablePhrase()
        scope.advanceUntilIdle()
        assertFalse(hub.phraseOn.value)
        assertNull(hub.phrase.value)
        assertEquals(listOf("newCode", "startHost"), core.calls)
        hub.phraseTick(now + 3_600_000)
        scope.advanceUntilIdle()
        assertEquals(listOf("newCode", "startHost"), core.calls)
    }

    @Test fun aHostSessionEndingRotatesCodeAndWords() {
        val hub = hub()
        hub.refreshCode()
        hub.enablePhrase("en")
        scope.advanceUntilIdle()
        val first = hub.phrase.value!!.words
        core.calls.clear()
        hub.onState(SessionState.PAIRING)
        hub.onEnded(EndInfo(EndKind.PAIRING_FAILED, "wrong code", null))
        scope.advanceUntilIdle()
        assertEquals(listOf("newCode", "startHost", "newPassphrase:en"), core.calls)
        assertTrue(hub.phrase.value!!.words != first)
    }

    @Test fun controllerConnectsByWordsWithAnEmptyCode() {
        val hub = hub()
        assertEquals(ro.dragoscatalin.scrin.ui.ConnectForm.Problem.WORDS_INVALID, hub.connectWords("apa bec cer"))
        assertNull(hub.connectWords(" Apa-bec, cer drum  EST "))
        scope.advanceUntilIdle()
        assertEquals(listOf("connect:Apa bec cer drum EST|"), core.calls)
    }
}
