package ro.dragoscatalin.scrin.ffi

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Test
import java.nio.file.Files
import java.util.concurrent.LinkedBlockingQueue
import java.util.concurrent.TimeUnit

/** Runs the generated Kotlin bindings against the host build of scrin-ffi through JNA. */
class FfiBindingsTest {
    private fun core(seed: Byte, name: String = "jvm"): ScrinCore {
        val dir = Files.createTempDirectory("scrin-ffi").toFile()
        dir.deleteOnExit()
        return ScrinCore(dir.absolutePath, ByteArray(32) { seed }, CoreConfig(name, emptyList(), true, null))
    }

    @Test
    fun versionAndCodeValidation() {
        assertTrue(coreVersion().isNotBlank())
        assertTrue(isValidCode("abcd-efgh"))
        assertTrue(isValidCode("AB CD EF GH"))
        assertFalse(isValidCode("ABCD-EFG0"))
        assertFalse(isValidCode(""))
    }

    @Test
    fun identityIsDeterministicFromTheSeed() {
        val a = core(5)
        val b = core(5)
        assertEquals(64, a.deviceId().length)
        assertEquals(a.deviceId(), b.deviceId())
        assertTrue(Regex("^[a-z2-7]{4}(-[a-z2-7]{4}){3}$").matches(a.fingerprint()))
        assertEquals(32, a.identitySeed().size)
        a.close()
        b.close()
    }

    @Test
    fun oneTimeCodeAndTicket() {
        val c = core(6)
        assertFalse(c.codeValid())
        val code = c.newOneTimeCode()
        assertTrue(Regex("^[A-Z2-9]{4}-[A-Z2-9]{4}$").matches(code.display))
        assertTrue(code.expiresInS in 590u..600u)
        assertTrue(c.codeValid())
        val info = c.hostInfo()
        assertTrue(info.ticket.startsWith("scrin:" + c.deviceId()))
        assertEquals(null, info.scrinId)
        val parsed = parseTicket(info.ticket)
        assertEquals(c.deviceId(), parsed.deviceId)
        assertTrue(parsed.directAddresses >= 1u)
        c.close()
    }

    @Test
    fun badInputsThrowTypedErrors() {
        val c = core(7)
        val l = Recorder()
        try {
            c.connect("not a ticket", "ABCD-EFGH", l)
            fail("expected InvalidInput")
        } catch (e: ScrinException.InvalidInput) {
            assertNotNull(e.message)
        }
        try {
            c.hostAccept(listOf(SessionPermission.VIEW))
            fail("expected State")
        } catch (_: ScrinException.State) {
        }
        c.close()
    }

    @Test
    fun trustListCrud() {
        val c = core(8)
        val other = core(9).deviceId()
        c.addTrusted(other, "Office", TrustProfile.SUPPORT, null)
        val list = c.listTrusted()
        assertEquals(1, list.size)
        assertEquals("Office", list[0].label)
        assertTrue(c.removeTrusted(other))
        assertTrue(c.listTrusted().isEmpty())
        c.close()
    }

    @Test
    fun quickConnectOverLoopbackDeliversMatchingSasThroughCallbacks() {
        val host = core(20, "host")
        val ctl = core(21, "phone")
        val code = host.newOneTimeCode()
        val ticket = host.hostInfo().ticket
        val hl = Recorder()
        val cl = Recorder()
        host.startHost(hl)
        hl.await { it == "state:LISTENING" }
        ctl.connect(ticket, code.display, cl)
        val hostSas = hl.await { it.startsWith("sas:") }
        val ctlSas = cl.await { it.startsWith("sas:") }
        assertEquals(hostSas, ctlSas)
        val req = hl.await { it.startsWith("request:") }
        assertEquals("request:phone:false", req)
        ctl.endSession()
        cl.await { it.startsWith("ended:") }
        hl.await { it.startsWith("ended:") }
        host.close()
        ctl.close()
    }

    /** Listener that records events as strings for easy assertions. */
    private class Recorder : SessionListener {
        private val q = LinkedBlockingQueue<String>()

        fun await(match: (String) -> Boolean): String {
            val deadline = System.nanoTime() + TimeUnit.SECONDS.toNanos(15)
            while (true) {
                val left = deadline - System.nanoTime()
                val e = q.poll(left.coerceAtLeast(0), TimeUnit.NANOSECONDS) ?: fail("timed out").let { "" }
                if (match(e)) return e
            }
        }

        override fun onState(state: SessionState) { q += "state:$state" }
        override fun onSas(sas: SasInfo) { q += "sas:" + sas.emoji.joinToString(" ") }
        override fun onIncomingRequest(request: IncomingRequest) { q += "request:${request.controllerName}:${request.verified}" }
        override fun onPermissions(granted: List<SessionPermission>) { q += "perms:$granted" }
        override fun onPermissionAsked(permission: SessionPermission) { q += "asked:$permission" }
        override fun onNotice(notice: Notice) { q += "notice:$notice" }
        override fun onStats(stats: SessionStats) = Unit
        override fun onRegistered(scrinId: String) { q += "registered:$scrinId" }
        override fun onVideoConfig(config: VideoConfigInfo) { q += "video:${config.width}x${config.height}" }
        override fun onVideoFrame(data: ByteArray, keyframe: Boolean, frameId: UInt, ptsUs: ULong) { q += "frame:${data.size}" }
        override fun onKeyframeRequest() { q += "kf" }
        override fun onInput(event: RemoteInput) { q += "input:$event" }
        override fun onEnded(end: EndInfo) { q += "ended:${end.kind}" }
        override fun onError(message: String) { q += "error:$message" }
    }
}
