package ro.dragoscatalin.scrin.media

/**
 * Pure Annex-B helpers (unit-tested on the JVM): find NAL units, pull SPS/PPS out of a
 * keyframe for MediaCodec csd-0/csd-1, and read the coded size from an SPS so a
 * resolution change is detected without waiting for the decoder.
 */
object H264 {
    const val NAL_SLICE = 1
    const val NAL_IDR = 5
    const val NAL_SPS = 7
    const val NAL_PPS = 8

    /** One NAL unit: [start] is the offset of its start code, [payload] of its header byte. */
    data class Nal(val type: Int, val start: Int, val payload: Int, val end: Int)

    fun nals(data: ByteArray): List<Nal> {
        val starts = ArrayList<Pair<Int, Int>>()
        var i = 0
        while (i + 3 <= data.size) {
            if (data[i].toInt() == 0 && data[i + 1].toInt() == 0) {
                if (data[i + 2].toInt() == 1) {
                    starts += i to i + 3
                    i += 3
                    continue
                }
                if (i + 4 <= data.size && data[i + 2].toInt() == 0 && data[i + 3].toInt() == 1) {
                    starts += i to i + 4
                    i += 4
                    continue
                }
            }
            i++
        }
        return starts.mapIndexedNotNull { idx, (start, payload) ->
            if (payload >= data.size) return@mapIndexedNotNull null
            val end = if (idx + 1 < starts.size) starts[idx + 1].first else data.size
            Nal(data[payload].toInt() and 0x1f, start, payload, end)
        }
    }

    /** SPS and PPS of an access unit, each with its start code, or `null` if either is missing. */
    fun parameterSets(data: ByteArray): Pair<ByteArray, ByteArray>? {
        val all = nals(data)
        val sps = all.firstOrNull { it.type == NAL_SPS } ?: return null
        val pps = all.firstOrNull { it.type == NAL_PPS } ?: return null
        return data.copyOfRange(sps.start, sps.end) to data.copyOfRange(pps.start, pps.end)
    }

    fun hasSps(data: ByteArray): Boolean = nals(data).any { it.type == NAL_SPS }

    /** Coded picture size (cropping applied) from an SPS NAL (with or without start code). */
    fun spsSize(sps: ByteArray): Pair<Int, Int>? = runCatching {
        val nal = nals(sps).firstOrNull { it.type == NAL_SPS }
        val from = nal?.payload ?: 0
        val to = nal?.end ?: sps.size
        val r = BitReader(unescape(sps, from + 1, to))
        val profile = r.bits(8)
        r.bits(16) // constraint flags + level
        r.ue() // seq_parameter_set_id
        var chromaFormat = 1
        if (profile in HIGH_PROFILES) {
            chromaFormat = r.ue()
            if (chromaFormat == 3) r.bits(1)
            r.ue()
            r.ue()
            r.bits(1)
            if (r.bits(1) == 1) {
                repeat(if (chromaFormat != 3) 8 else 12) { idx -> if (r.bits(1) == 1) skipScalingList(r, if (idx < 6) 16 else 64) }
            }
        }
        r.ue() // log2_max_frame_num_minus4
        when (r.ue()) {
            0 -> r.ue()
            1 -> {
                r.bits(1)
                r.se()
                r.se()
                repeat(r.ue()) { r.se() }
            }
        }
        r.ue() // max_num_ref_frames
        r.bits(1)
        val wMbs = r.ue() + 1
        val hMapUnits = r.ue() + 1
        val frameMbsOnly = r.bits(1)
        if (frameMbsOnly == 0) r.bits(1)
        r.bits(1)
        var w = wMbs * 16
        var h = (2 - frameMbsOnly) * hMapUnits * 16
        if (r.bits(1) == 1) {
            val (cx, cy) = when (chromaFormat) {
                0 -> 1 to (2 - frameMbsOnly)
                1 -> 2 to 2 * (2 - frameMbsOnly)
                2 -> 2 to (2 - frameMbsOnly)
                else -> 1 to (2 - frameMbsOnly)
            }
            w -= (r.ue() + r.ue()) * cx
            h -= (r.ue() + r.ue()) * cy
        }
        if (w <= 0 || h <= 0) null else w to h
    }.getOrNull()

    private val HIGH_PROFILES = setOf(100, 110, 122, 244, 44, 83, 86, 118, 128, 138, 139, 134, 135)

    private fun skipScalingList(r: BitReader, size: Int) {
        var last = 8
        var next = 8
        repeat(size) {
            if (next != 0) next = (last + r.se() + 256) % 256
            last = if (next == 0) last else next
        }
    }

    /** Drops emulation-prevention bytes (00 00 03 → 00 00). */
    private fun unescape(b: ByteArray, from: Int, to: Int): ByteArray {
        val out = java.io.ByteArrayOutputStream(to - from)
        var zeros = 0
        for (i in from until to) {
            val v = b[i].toInt() and 0xff
            if (zeros >= 2 && v == 3) {
                zeros = 0
                continue
            }
            out.write(v)
            zeros = if (v == 0) zeros + 1 else 0
        }
        return out.toByteArray()
    }

    private class BitReader(private val b: ByteArray) {
        private var pos = 0
        fun bits(n: Int): Int {
            var v = 0
            repeat(n) {
                val byte = b[pos ushr 3].toInt() and 0xff
                v = (v shl 1) or ((byte ushr (7 - (pos and 7))) and 1)
                pos++
            }
            return v
        }
        fun ue(): Int {
            var zeros = 0
            while (bits(1) == 0) {
                zeros++
                require(zeros < 32)
            }
            return (1 shl zeros) - 1 + bits(zeros)
        }
        fun se(): Int {
            val k = ue()
            return if (k and 1 == 1) (k + 1) / 2 else -(k / 2)
        }
    }
}
