package ro.dragoscatalin.scrin.service

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.hardware.display.DisplayManager
import android.hardware.display.VirtualDisplay
import android.media.MediaCodec
import android.media.MediaCodecInfo
import android.media.MediaFormat
import android.media.projection.MediaProjection
import android.media.projection.MediaProjectionManager
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.HandlerThread
import android.os.IBinder
import android.util.DisplayMetrics
import android.util.Log
import android.view.Display
import androidx.core.app.NotificationCompat
import androidx.core.app.ServiceCompat
import androidx.core.content.IntentCompat
import ro.dragoscatalin.scrin.MainActivity
import ro.dragoscatalin.scrin.R
import ro.dragoscatalin.scrin.ScrinApp
import ro.dragoscatalin.scrin.core.MediaSinks
import ro.dragoscatalin.scrin.ffi.RemoteInput
import ro.dragoscatalin.scrin.ffi.VideoCodec
import ro.dragoscatalin.scrin.ffi.VideoConfigInfo
import ro.dragoscatalin.scrin.media.fitSize

/**
 * Host capture: MediaProjection → VirtualDisplay → MediaCodec H.264 encoder (Surface
 * input) → Rust core (FEC + datagrams). Runs as a `mediaProjection` foreground service
 * with a persistent "Remote session active — tap to stop" notification.
 *
 * Consent is requested for every session by the UI and passed in the start intent;
 * the token is never stored (D08).
 */
class ScreenCaptureService : Service(), MediaSinks {
    companion object {
        const val CHANNEL = "session"
        const val NOTIFICATION_ID = 7
        const val ACTION_START = "ro.dragoscatalin.scrin.capture.START"
        const val ACTION_STOP = "ro.dragoscatalin.scrin.capture.STOP"
        const val EXTRA_RESULT_CODE = "result_code"
        const val EXTRA_RESULT_DATA = "result_data"

        private const val TAG = "scrin.encoder"
        /** At most 1080p: long edge ≤ 1920 and short edge ≤ 1080, aspect preserved. */
        private const val MAX_LONG = 1920
        private const val MAX_SHORT = 1080
        private const val FPS = 30
        private const val BITRATE = 6_000_000
        /**
         * Periodic IDR (seconds) + on-demand sync frames. Not KEY_INTRA_REFRESH_PERIOD: a
         * gradual-refresh stream never carries another IDR, and a MediaCodec client that
         * joins late (or after loss) waits for one.
         */
        private const val KEYFRAME_INTERVAL_S = 2
        private const val STATS_EVERY_MS = 2_000L

        fun start(ctx: Context, resultCode: Int, data: Intent) {
            val i = Intent(ctx, ScreenCaptureService::class.java)
                .setAction(ACTION_START)
                .putExtra(EXTRA_RESULT_CODE, resultCode)
                .putExtra(EXTRA_RESULT_DATA, data)
            ctx.startForegroundService(i)
        }

        fun stop(ctx: Context) {
            ctx.startService(Intent(ctx, ScreenCaptureService::class.java).setAction(ACTION_STOP))
        }

        fun ensureChannel(ctx: Context) {
            val nm = ctx.getSystemService(NotificationManager::class.java)
            if (nm.getNotificationChannel(CHANNEL) == null) {
                val ch = NotificationChannel(CHANNEL, ctx.getString(R.string.notif_channel_session), NotificationManager.IMPORTANCE_LOW)
                ch.description = ctx.getString(R.string.notif_channel_session_desc)
                nm.createNotificationChannel(ch)
            }
        }
    }

    private val hub get() = (application as ScrinApp).hub
    private var projection: MediaProjection? = null
    private var display: VirtualDisplay? = null
    private var encoder: MediaCodec? = null
    private var thread: HandlerThread? = null

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        when (intent?.action) {
            ACTION_STOP -> {
                hub.end()
                stopSelf()
                return START_NOT_STICKY
            }
            ACTION_START -> {
                startForegroundNow()
                val code = intent.getIntExtra(EXTRA_RESULT_CODE, 0)
                val data = IntentCompat.getParcelableExtra(intent, EXTRA_RESULT_DATA, Intent::class.java)
                if (data == null) {
                    stopSelf()
                    return START_NOT_STICKY
                }
                startCapture(code, data)
            }
        }
        return START_NOT_STICKY
    }

    private fun startForegroundNow() {
        ensureChannel(this)
        val open = PendingIntent.getActivity(
            this, 0, Intent(this, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP),
            PendingIntent.FLAG_IMMUTABLE,
        )
        val stop = PendingIntent.getService(
            this, 1, Intent(this, ScreenCaptureService::class.java).setAction(ACTION_STOP),
            PendingIntent.FLAG_IMMUTABLE,
        )
        val n: Notification = NotificationCompat.Builder(this, CHANNEL)
            .setSmallIcon(R.drawable.ic_stat_scrin)
            .setContentTitle(getString(R.string.notif_session_title))
            .setContentText(getString(R.string.notif_session_text))
            .setOngoing(true)
            .setCategory(NotificationCompat.CATEGORY_SERVICE)
            .setContentIntent(open)
            .addAction(0, getString(R.string.notif_open), open)
            .addAction(0, getString(R.string.notif_stop), stop)
            .build()
        val type = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) ServiceInfo.FOREGROUND_SERVICE_TYPE_MEDIA_PROJECTION else 0
        ServiceCompat.startForeground(this, NOTIFICATION_ID, n, type)
    }

    private fun startCapture(resultCode: Int, data: Intent) {
        val mpm = getSystemService(MediaProjectionManager::class.java)
        val p = mpm.getMediaProjection(resultCode, data) ?: return stopSelf()
        projection = p
        val t = HandlerThread("scrin-encoder").also { it.start() }
        thread = t
        val handler = Handler(t.looper)
        p.registerCallback(object : MediaProjection.Callback() {
            override fun onStop() {
                hub.end()
                stopSelf()
            }
        }, handler)

        val real = realMetrics()
        val enc = MediaCodec.createEncoderByType(MediaFormat.MIMETYPE_VIDEO_AVC)
        val caps = enc.codecInfo.getCapabilitiesForType(MediaFormat.MIMETYPE_VIDEO_AVC).videoCapabilities
        val (w, h) = fitSize(real.widthPixels, real.heightPixels, MAX_LONG, MAX_SHORT) { cw, ch -> caps?.isSizeSupported(cw, ch) ?: true }
        if (!configure(enc, w, h)) {
            enc.release()
            hub.end()
            stopSelf()
            return
        }
        enc.setCallback(EncoderCallback(w, h), handler)
        val input = enc.createInputSurface()
        enc.start()
        encoder = enc
        Log.i(TAG, "encoder ${enc.name} ${w}x$h from ${real.widthPixels}x${real.heightPixels} @ $FPS fps ${BITRATE / 1000} kb/s CBR")
        display = p.createVirtualDisplay(
            "scrin", w, h, real.densityDpi, DisplayManager.VIRTUAL_DISPLAY_FLAG_AUTO_MIRROR, input, null, handler,
        )
        hub.hostSinks = this
    }

    /** CBR, low latency, periodic IDR; profile/level hints dropped if the encoder refuses them. */
    private fun configure(enc: MediaCodec, w: Int, h: Int): Boolean {
        fun format(withProfile: Boolean) = MediaFormat.createVideoFormat(MediaFormat.MIMETYPE_VIDEO_AVC, w, h).apply {
            setInteger(MediaFormat.KEY_COLOR_FORMAT, MediaCodecInfo.CodecCapabilities.COLOR_FormatSurface)
            setInteger(MediaFormat.KEY_BIT_RATE, BITRATE)
            setInteger(MediaFormat.KEY_BITRATE_MODE, MediaCodecInfo.EncoderCapabilities.BITRATE_MODE_CBR)
            setInteger(MediaFormat.KEY_FRAME_RATE, FPS)
            setInteger(MediaFormat.KEY_I_FRAME_INTERVAL, KEYFRAME_INTERVAL_S)
            setInteger(MediaFormat.KEY_PRIORITY, 0)
            setInteger(MediaFormat.KEY_LATENCY, 1)
            if (withProfile) {
                setInteger(MediaFormat.KEY_PROFILE, MediaCodecInfo.CodecProfileLevel.AVCProfileBaseline)
                setInteger(MediaFormat.KEY_LEVEL, MediaCodecInfo.CodecProfileLevel.AVCLevel41)
            }
            // Repeat the last frame when the screen is static so late joiners still get pictures.
            setLong(MediaFormat.KEY_REPEAT_PREVIOUS_FRAME_AFTER, 1_000_000L / FPS * 10)
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) setInteger(MediaFormat.KEY_LOW_LATENCY, 1)
        }
        return listOf(true, false).any { withProfile ->
            runCatching { enc.configure(format(withProfile), null, null, MediaCodec.CONFIGURE_FLAG_ENCODE) }
                .onFailure { Log.w(TAG, "configure ${w}x$h profile=$withProfile failed: $it") }
                .isSuccess
        }
    }

    /** Full panel size including system bars (the projection mirrors all of it). */
    @Suppress("DEPRECATION") // getRealMetrics: the non-deprecated WindowMetrics needs a visual context.
    private fun realMetrics(): DisplayMetrics {
        val m = DisplayMetrics()
        getSystemService(DisplayManager::class.java).getDisplay(Display.DEFAULT_DISPLAY)?.getRealMetrics(m)
        return if (m.widthPixels > 0) m else resources.displayMetrics
    }

    private inner class EncoderCallback(private val w: Int, private val h: Int) : MediaCodec.Callback() {
        private var frames = 0
        private var bytes = 0L
        private var keys = 0
        private var since = System.nanoTime()

        override fun onInputBufferAvailable(codec: MediaCodec, index: Int) = Unit

        override fun onOutputBufferAvailable(codec: MediaCodec, index: Int, info: MediaCodec.BufferInfo) {
            val buf = codec.getOutputBuffer(index)
            if (buf != null && info.size > 0) {
                val bytes = ByteArray(info.size)
                buf.position(info.offset)
                buf.get(bytes)
                if (info.flags and MediaCodec.BUFFER_FLAG_CODEC_CONFIG != 0) {
                    Log.i(TAG, "codec config ${bytes.size} bytes")
                    hub.sendVideoConfig(VideoConfigInfo(VideoCodec.H264, w.toUInt(), h.toUInt(), FPS.toUInt(), BITRATE.toUInt(), bytes))
                } else {
                    val key = info.flags and MediaCodec.BUFFER_FLAG_KEY_FRAME != 0
                    hub.sendVideoFrame(bytes, key)
                    count(bytes.size, key)
                }
            }
            codec.releaseOutputBuffer(index, false)
        }

        private fun count(size: Int, key: Boolean) {
            frames++
            bytes += size
            if (key) keys++
            val ms = (System.nanoTime() - since) / 1_000_000
            if (ms >= STATS_EVERY_MS) {
                Log.i(TAG, "encoded fps=%.1f kbps=%d keyframes=%d".format(java.util.Locale.ROOT, frames * 1000.0 / ms, bytes * 8 / ms, keys))
                frames = 0
                bytes = 0
                keys = 0
                since = System.nanoTime()
            }
        }

        override fun onError(codec: MediaCodec, e: MediaCodec.CodecException) {
            hub.end()
            stopSelf()
        }

        override fun onOutputFormatChanged(codec: MediaCodec, format: MediaFormat) = Unit
    }

    /** The controller lost a frame: ask the encoder for a sync frame now. */
    override fun onKeyframeRequest() {
        val enc = encoder ?: return
        runCatching { enc.setParameters(Bundle().apply { putInt(MediaCodec.PARAMETER_KEY_REQUEST_SYNC_FRAME, 0) }) }
    }

    override fun onInput(event: RemoteInput) {
        if (!RemoteInputService.dispatch(event) && !warnedNoInput) {
            warnedNoInput = true
            Log.w(TAG, "input received but the remote-control accessibility service is off")
        }
    }

    private var warnedNoInput = false

    /** The core ended the host session: release MediaProjection and the notification. */
    override fun onSessionEnded() {
        stopSelf()
    }

    override fun onDestroy() {
        hub.hostSinks = null
        runCatching { display?.release() }
        runCatching { encoder?.stop() }
        runCatching { encoder?.release() }
        runCatching { projection?.stop() }
        thread?.quitSafely()
        display = null
        encoder = null
        projection = null
        super.onDestroy()
    }
}
