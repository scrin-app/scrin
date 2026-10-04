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

        private const val MAX_EDGE = 1280
        private const val FPS = 30
        private const val BITRATE = 4_000_000
        private const val INTRA_REFRESH_FRAMES = FPS * 2

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

        val metrics = resources.displayMetrics
        val (w, h) = scaled(metrics.widthPixels, metrics.heightPixels)
        val fmt = MediaFormat.createVideoFormat(MediaFormat.MIMETYPE_VIDEO_AVC, w, h).apply {
            setInteger(MediaFormat.KEY_COLOR_FORMAT, MediaCodecInfo.CodecCapabilities.COLOR_FormatSurface)
            setInteger(MediaFormat.KEY_BIT_RATE, BITRATE)
            setInteger(MediaFormat.KEY_BITRATE_MODE, MediaCodecInfo.EncoderCapabilities.BITRATE_MODE_CBR)
            setInteger(MediaFormat.KEY_FRAME_RATE, FPS)
            setInteger(MediaFormat.KEY_I_FRAME_INTERVAL, 10)
            setInteger(MediaFormat.KEY_PRIORITY, 0)
            setInteger(MediaFormat.KEY_INTRA_REFRESH_PERIOD, INTRA_REFRESH_FRAMES)
            setInteger(MediaFormat.KEY_PROFILE, MediaCodecInfo.CodecProfileLevel.AVCProfileBaseline)
            // Repeat the last frame when the screen is static so late joiners still get pictures.
            setLong(MediaFormat.KEY_REPEAT_PREVIOUS_FRAME_AFTER, 1_000_000L / FPS * 10)
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) setInteger(MediaFormat.KEY_LOW_LATENCY, 1)
        }
        val enc = MediaCodec.createEncoderByType(MediaFormat.MIMETYPE_VIDEO_AVC)
        enc.setCallback(EncoderCallback(w, h), handler)
        enc.configure(fmt, null, null, MediaCodec.CONFIGURE_FLAG_ENCODE)
        val input = enc.createInputSurface()
        enc.start()
        encoder = enc
        display = p.createVirtualDisplay(
            "scrin", w, h, metrics.densityDpi, DisplayManager.VIRTUAL_DISPLAY_FLAG_AUTO_MIRROR, input, null, handler,
        )
        hub.hostSinks = this
    }

    private fun scaled(w: Int, h: Int): Pair<Int, Int> {
        val long = maxOf(w, h)
        val f = if (long > MAX_EDGE) MAX_EDGE.toFloat() / long else 1f
        // Encoders want even (ideally 16-aligned) dimensions.
        fun even(v: Float) = (v.toInt() / 16) * 16
        return even(w * f) to even(h * f)
    }

    private inner class EncoderCallback(private val w: Int, private val h: Int) : MediaCodec.Callback() {
        override fun onInputBufferAvailable(codec: MediaCodec, index: Int) = Unit

        override fun onOutputBufferAvailable(codec: MediaCodec, index: Int, info: MediaCodec.BufferInfo) {
            val buf = codec.getOutputBuffer(index)
            if (buf != null && info.size > 0) {
                val bytes = ByteArray(info.size)
                buf.position(info.offset)
                buf.get(bytes)
                if (info.flags and MediaCodec.BUFFER_FLAG_CODEC_CONFIG != 0) {
                    hub.sendVideoConfig(VideoConfigInfo(VideoCodec.H264, w.toUInt(), h.toUInt(), FPS.toUInt(), BITRATE.toUInt(), bytes))
                } else {
                    hub.sendVideoFrame(bytes, info.flags and MediaCodec.BUFFER_FLAG_KEY_FRAME != 0)
                }
            }
            codec.releaseOutputBuffer(index, false)
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
        RemoteInputService.dispatch(event)
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
