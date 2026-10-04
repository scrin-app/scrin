package ro.dragoscatalin.scrin.media

import android.media.MediaCodec
import android.media.MediaFormat
import android.os.Build
import android.view.Surface
import ro.dragoscatalin.scrin.core.MediaSinks
import ro.dragoscatalin.scrin.ffi.RemoteInput
import ro.dragoscatalin.scrin.ffi.VideoCodec
import ro.dragoscatalin.scrin.ffi.VideoConfigInfo
import java.util.concurrent.atomic.AtomicBoolean

/**
 * Client decoder: Annex-B access units → MediaCodec (low latency) → SurfaceView.
 * Latest-frame-wins: if no input buffer is free, the frame is dropped and the
 * next keyframe resynchronises (ARCHITECTURE §5).
 */
class VideoDecoder(private val surface: Surface, private val onNeedKeyframe: () -> Unit) : MediaSinks {
    private companion object {
        const val MIME_AV1 = "video/av01"
    }

    private var codec: MediaCodec? = null
    private var config: VideoConfigInfo? = null
    private val waitingKeyframe = AtomicBoolean(true)
    private val info = MediaCodec.BufferInfo()

    @Synchronized
    override fun onVideoConfig(config: VideoConfigInfo) {
        if (config == this.config && codec != null) return
        release()
        this.config = config
        val mime = when (config.codec) {
            VideoCodec.H264 -> MediaFormat.MIMETYPE_VIDEO_AVC
            VideoCodec.HEVC -> MediaFormat.MIMETYPE_VIDEO_HEVC
            // Literal, not MediaFormat.MIMETYPE_VIDEO_AV1 (API 29+); minSdk 26 decoders
            // without AV1 simply fail createDecoderByType below and we fall back to no codec.
            VideoCodec.AV1 -> MIME_AV1
        }
        val fmt = MediaFormat.createVideoFormat(mime, config.width.toInt(), config.height.toInt()).apply {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) setInteger(MediaFormat.KEY_LOW_LATENCY, 1)
            setInteger(MediaFormat.KEY_PRIORITY, 0)
            if (config.codecConfig.isNotEmpty()) setByteBuffer("csd-0", java.nio.ByteBuffer.wrap(config.codecConfig))
        }
        codec = runCatching {
            MediaCodec.createDecoderByType(mime).apply {
                configure(fmt, surface, null, 0)
                start()
            }
        }.getOrNull()
        waitingKeyframe.set(true)
        onNeedKeyframe()
    }

    @Synchronized
    override fun onVideoFrame(data: ByteArray, keyframe: Boolean) {
        val c = codec ?: return
        if (waitingKeyframe.get() && !keyframe) return
        waitingKeyframe.set(false)
        runCatching {
            val idx = c.dequeueInputBuffer(0)
            if (idx < 0) {
                waitingKeyframe.set(true)
                onNeedKeyframe()
                return
            }
            c.getInputBuffer(idx)?.apply { clear(); put(data) }
            c.queueInputBuffer(idx, 0, data.size, System.nanoTime() / 1000, if (keyframe) MediaCodec.BUFFER_FLAG_KEY_FRAME else 0)
            drain(c)
        }.onFailure {
            waitingKeyframe.set(true)
            onNeedKeyframe()
        }
    }

    private fun drain(c: MediaCodec) {
        while (true) {
            val out = c.dequeueOutputBuffer(info, 0)
            if (out < 0) return
            // Render immediately: no jitter buffer on the hot path.
            c.releaseOutputBuffer(out, true)
        }
    }

    override fun onInput(event: RemoteInput) = Unit

    @Synchronized
    fun release() {
        codec?.let { runCatching { it.stop() }; runCatching { it.release() } }
        codec = null
        config = null
    }
}
