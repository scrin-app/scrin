package ro.dragoscatalin.scrin.core

import ro.dragoscatalin.scrin.ffi.CodeInfo
import ro.dragoscatalin.scrin.ffi.HostInfo
import ro.dragoscatalin.scrin.ffi.PassphraseInfo
import ro.dragoscatalin.scrin.ffi.RemoteInput
import ro.dragoscatalin.scrin.ffi.ScrinCore
import ro.dragoscatalin.scrin.ffi.SessionListener
import ro.dragoscatalin.scrin.ffi.SessionPermission
import ro.dragoscatalin.scrin.ffi.TrustedDevice
import ro.dragoscatalin.scrin.ffi.VideoConfigInfo

/**
 * The slice of the native core the app uses. An interface so view-models are
 * unit-testable on the JVM without loading the Rust library.
 */
interface CoreApi {
    val deviceId: String
    val fingerprint: String
    fun newCode(): CodeInfo
    /** D24: five words on the current code slot. Needs a server, a registered host and a code. */
    fun newPassphrase(lang: String): PassphraseInfo
    fun hostInfo(): HostInfo
    fun startHost(listener: SessionListener)
    fun stopHost()
    fun connect(target: String, code: String, listener: SessionListener)
    fun hostAccept(permissions: List<SessionPermission>)
    fun hostReject()
    fun grant(permission: SessionPermission)
    fun revoke(permission: SessionPermission)
    fun requestPermission(permission: SessionPermission)
    fun sendInput(event: RemoteInput)
    fun sendVideoConfig(config: VideoConfigInfo)
    fun sendVideoFrame(data: ByteArray, keyframe: Boolean)
    fun requestKeyframe()
    fun endSession()
    fun stopAndReport()
    fun listTrusted(): List<TrustedDevice>
    fun removeTrusted(deviceId: String): Boolean
}

/** [CoreApi] backed by the UniFFI bindings. Calls may block: use Dispatchers.IO. */
class NativeCore(private val core: ScrinCore) : CoreApi {
    override val deviceId: String = core.deviceId()
    override val fingerprint: String = core.fingerprint()
    override fun newCode() = core.newOneTimeCode()
    override fun newPassphrase(lang: String) = core.newPassphrase(lang)
    override fun hostInfo() = core.hostInfo()
    override fun startHost(listener: SessionListener) = core.startHost(listener)
    override fun stopHost() = core.stopHost()
    override fun connect(target: String, code: String, listener: SessionListener) = core.connect(target, code, listener)
    override fun hostAccept(permissions: List<SessionPermission>) = core.hostAccept(permissions)
    override fun hostReject() = core.hostReject()
    override fun grant(permission: SessionPermission) = core.grantPermission(permission)
    override fun revoke(permission: SessionPermission) = core.revokePermission(permission)
    override fun requestPermission(permission: SessionPermission) = core.requestPermission(permission)
    override fun sendInput(event: RemoteInput) = core.sendInput(event)
    override fun sendVideoConfig(config: VideoConfigInfo) = core.sendVideoConfig(config)
    override fun sendVideoFrame(data: ByteArray, keyframe: Boolean) = core.sendVideoFrame(data, keyframe)
    override fun requestKeyframe() = core.requestKeyframe()
    override fun endSession() = core.endSession()
    override fun stopAndReport() = core.stopAndReport()
    override fun listTrusted() = core.listTrusted()
    override fun removeTrusted(deviceId: String) = core.removeTrusted(deviceId)
}
