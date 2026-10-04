package ro.dragoscatalin.scrin.media

import android.media.MediaCodec
import android.media.MediaCodecInfo
import android.media.MediaCodecList
import android.media.MediaFormat
import android.os.Build
import android.os.Handler
import android.os.HandlerThread
import android.util.Log
import android.view.Surface
import ro.dragoscatalin.scrin.core.MediaSinks
import ro.dragoscatalin.scrin.ffi.VideoCodec
import ro.dragoscatalin.scrin.ffi.VideoConfigInfo
import java.nio.ByteBuffer
import java.util.ArrayDeque

/**
 * Client decoder: complete H.264 Annex-B access units → MediaCodec (async, low latency)
 * → the viewer's SurfaceView.
 *
 * The codec is (re)configured from the SPS/PPS of the first keyframe (csd-0 / csd-1), so a
 * host that sends parameter sets in band (Windows MF/openh264) and one that sends them in
 * `VideoConfig.codec_config` (Android) both work; an SPS with a new size reconfigures it.
 * Latest-frame-wins: when MediaCodec has no free input buffer the frame is dropped and the
 * next keyframe resynchronises.
 */
class VideoDecoder(
    private val surface: Surface,
    private val onNeedKeyframe: () -> Unit,
    private val onSize: (Int, Int) -> Unit = { _, _ -> },
) : MediaSinks {
    private companion object {
        const val TAG = "scrin.decoder"
        const val MAX_QUEUE = 4
        const val STATS_EVERY_MS = 2_000L
    }

    private val thread = HandlerThread("scrin-decoder").also { it.start() }
    private val handler = Handler(thread.looper)
    private val lock = Any()

    // Guarded by `lock`.
    private var codec: MediaCodec? = null
    private var size: Pair<Int, Int>? = null
    private var csd: Pair<ByteArray, ByteArray>? = null
    private var waitingKeyframe = true
    private val freeInputs = ArrayDeque<Int>()
    private val pending = ArrayDeque<Triple<ByteArray, Boolean, Long>>()
    private var released = false

    @Volatile var rendered = 0L
        private set
    @Volatile var decoderName: String = ""
        private set
    private var statFrames = 0L
    private var statAt = System.nanoTime()

    /** Host announced its stream; parameter sets may arrive here or in band. */
    override fun onVideoConfig(config: VideoConfigInfo) {
        if (config.codec != VideoCodec.H264) {
            Log.w(TAG, "unsupported codec ${config.codec}")
            return
        }
        synchronized(lock) {
            if (config.codecConfig.isNotEmpty()) H264.parameterSets(config.codecConfig)?.let { csd = it }
            if (size == null) size = config.width.toInt() to config.height.toInt()
            waitingKeyframe = true
        }
        onNeedKeyframe()
    }

    override fun onVideoFrame(data: ByteArray, keyframe: Boolean, ptsUs: Long) {
        synchronized(lock) {
            if (released) return
            if (keyframe) onKeyframe(data)
            if (codec == null || (waitingKeyframe && !keyframe)) return
            waitingKeyframe = false
            if (pending.size >= MAX_QUEUE) {
                // Decoder behind: drop and resync instead of building latency.
                pending.clear()
                waitingKeyframe = true
                handler.post(onNeedKeyframe)
                return
            }
            pending.addLast(Triple(data, keyframe, ptsUs))
            feed()
        }
    }

    /** New SPS/PPS or size → (re)create the codec before this keyframe is queued. */
    private fun onKeyframe(data: ByteArray) {
        val ps = H264.parameterSets(data)
        if (ps != null) {
            val newSize = H264.spsSize(ps.first)
            val changed = codec == null || (newSize != null && newSize != size) || !ps.first.contentEquals(csd?.first)
            csd = ps
            if (newSize != null) size = newSize
            if (changed) start()
        } else if (codec == null && csd != null) {
            start()
        }
    }

    private fun start() {
        stopCodec()
        val (w, h) = size ?: return
        val (sps, pps) = csd ?: return
        fun format(operatingRate: Boolean) = MediaFormat.createVideoFormat(MediaFormat.MIMETYPE_VIDEO_AVC, w, h).apply {
            setByteBuffer("csd-0", ByteBuffer.wrap(sps))
            setByteBuffer("csd-1", ByteBuffer.wrap(pps))
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) setInteger(MediaFormat.KEY_LOW_LATENCY, 1)
            setInteger(MediaFormat.KEY_PRIORITY, 0)
            // Decode as fast as frames arrive, not at a nominal rate.
            if (operatingRate) setInteger(MediaFormat.KEY_OPERATING_RATE, Short.MAX_VALUE.toInt())
        }
        val fmt = format(operatingRate = true)
        val name = pickDecoder(fmt)
        val c = runCatching { if (name != null) MediaCodec.createByCodecName(name) else MediaCodec.createDecoderByType(MediaFormat.MIMETYPE_VIDEO_AVC) }
            .getOrElse {
                Log.w(TAG, "no H.264 decoder: $it")
                return
            }
        c.setCallback(Callbacks(c), handler)
        val ok = runCatching { c.configure(fmt, surface, null, 0) }
            .recoverCatching {
                // Some decoders reject the operating-rate hint; retry without it.
                c.reset()
                c.setCallback(Callbacks(c), handler)
                c.configure(format(operatingRate = false), surface, null, 0)
            }
            .map { c.start() }
            .isSuccess
        if (!ok) {
            runCatching { c.release() }
            Log.w(TAG, "decoder configure failed for ${w}x$h")
            return
        }
        codec = c
        decoderName = c.name
        Log.i(TAG, "decoder ${c.name} ${w}x$h low-latency=${Build.VERSION.SDK_INT >= Build.VERSION_CODES.R}")
        onSize(w, h)
    }

    /** Hardware, low-latency-capable decoder first. */
    private fun pickDecoder(fmt: MediaFormat): String? {
        val infos = MediaCodecList(MediaCodecList.REGULAR_CODECS).codecInfos.filter { info ->
            !info.isEncoder && info.supportedTypes.any { it.equals(MediaFormat.MIMETYPE_VIDEO_AVC, ignoreCase = true) }
        }
        fun score(i: MediaCodecInfo): Int {
            var s = 0
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q && i.isHardwareAccelerated) s += 2
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R &&
                i.getCapabilitiesForType(MediaFormat.MIMETYPE_VIDEO_AVC).isFeatureSupported(MediaCodecInfo.CodecCapabilities.FEATURE_LowLatency)
            ) {
                s += 1
            }
            return s
        }
        return infos.filter { runCatching { it.getCapabilitiesForType(MediaFormat.MIMETYPE_VIDEO_AVC).isFormatSupported(withoutCsd(fmt)) }.getOrDefault(false) }
            .maxByOrNull(::score)?.name
    }

    private fun withoutCsd(fmt: MediaFormat) = MediaFormat.createVideoFormat(
        MediaFormat.MIMETYPE_VIDEO_AVC,
        fmt.getInteger(MediaFormat.KEY_WIDTH),
        fmt.getInteger(MediaFormat.KEY_HEIGHT),
    )

    /** Moves pending access units into free input buffers. Caller holds `lock`. */
    private fun feed() {
        val c = codec ?: return
        while (pending.isNotEmpty() && freeInputs.isNotEmpty()) {
            val idx = freeInputs.removeFirst()
            val (data, key, pts) = pending.removeFirst()
            val ok = runCatching {
                val buf = c.getInputBuffer(idx) ?: error("no input buffer")
                buf.clear()
                buf.put(data)
                c.queueInputBuffer(idx, 0, data.size, pts, if (key) MediaCodec.BUFFER_FLAG_KEY_FRAME else 0)
            }.isSuccess
            if (!ok) {
                pending.clear()
                waitingKeyframe = true
                handler.post(onNeedKeyframe)
                return
            }
        }
    }

    private inner class Callbacks(private val owner: MediaCodec) : MediaCodec.Callback() {
        override fun onInputBufferAvailable(codec: MediaCodec, index: Int) {
            synchronized(lock) {
                if (this@VideoDecoder.codec !== owner) return
                freeInputs.addLast(index)
                feed()
            }
        }

        override fun onOutputBufferAvailable(codec: MediaCodec, index: Int, info: MediaCodec.BufferInfo) {
            // Render immediately: no jitter buffer on the hot path.
            runCatching { codec.releaseOutputBuffer(index, info.size > 0) }
            if (info.size <= 0) return
            rendered++
            statFrames++
            val now = System.nanoTime()
            val ms = (now - statAt) / 1_000_000
            if (ms >= STATS_EVERY_MS) {
                Log.i(TAG, "decoded fps=%.1f total=%d decoder=%s".format(java.util.Locale.ROOT, statFrames * 1000.0 / ms, rendered, decoderName))
                statFrames = 0
                statAt = now
            }
        }

        override fun onError(codec: MediaCodec, e: MediaCodec.CodecException) {
            Log.w(TAG, "decoder error ${e.diagnosticInfo}")
            synchronized(lock) {
                if (this@VideoDecoder.codec !== owner) return
                stopCodec()
                waitingKeyframe = true
            }
            onNeedKeyframe()
        }

        override fun onOutputFormatChanged(codec: MediaCodec, format: MediaFormat) {
            val w = format.getInteger(MediaFormat.KEY_WIDTH)
            val h = format.getInteger(MediaFormat.KEY_HEIGHT)
            onSize(w, h)
        }
    }

    /** Caller holds `lock`. */
    private fun stopCodec() {
        codec?.let { runCatching { it.stop() }; runCatching { it.release() } }
        codec = null
        freeInputs.clear()
        pending.clear()
    }

    fun release() {
        synchronized(lock) {
            released = true
            stopCodec()
        }
        thread.quitSafely()
    }
}
