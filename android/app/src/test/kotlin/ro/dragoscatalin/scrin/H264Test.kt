package ro.dragoscatalin.scrin

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import ro.dragoscatalin.scrin.media.H264

class H264Test {
    /** Minimal bit writer to build SPS NALs field by field (spec §7.3.2.1.1). */
    private class Bits {
        private val bits = ArrayList<Int>()
        fun u(n: Int, v: Int) = apply { for (i in n - 1 downTo 0) bits += (v ushr i) and 1 }
        fun ue(v: Int) = apply {
            val x = v + 1
            val len = 32 - Integer.numberOfLeadingZeros(x)
            u(len - 1, 0)
            u(len, x)
        }
        fun bytes(): ByteArray {
            val b = ArrayList(bits).apply { add(1); while (size % 8 != 0) add(0) } // rbsp trailing bits
            return ByteArray(b.size / 8) { i -> (0 until 8).fold(0) { acc, k -> (acc shl 1) or b[i * 8 + k] }.toByte() }
        }
    }

    /** Baseline SPS for a `wMbs`×`hMbs` frame-only picture with optional bottom/right crop. */
    private fun sps(wMbs: Int, hMbs: Int, cropRight: Int = 0, cropBottom: Int = 0): ByteArray {
        val b = Bits().u(8, 66).u(8, 0xC0).u(8, 31).ue(0) // profile, constraints, level, id
            .ue(0) // log2_max_frame_num_minus4
            .ue(2) // pic_order_cnt_type
            .ue(1) // max_num_ref_frames
            .u(1, 0) // gaps
            .ue(wMbs - 1).ue(hMbs - 1)
            .u(1, 1) // frame_mbs_only
            .u(1, 1) // direct_8x8
        if (cropRight + cropBottom > 0) b.u(1, 1).ue(0).ue(cropRight).ue(0).ue(cropBottom) else b.u(1, 0)
        b.u(1, 0) // vui
        return byteArrayOf(0, 0, 0, 1, 0x67) + b.bytes()
    }

    private val pps = byteArrayOf(0, 0, 0, 1, 0x68, 0xce.toByte(), 0x3c, 0x80.toByte())
    private val idr = byteArrayOf(0, 0, 1, 0x65, 0x88.toByte(), 0x80.toByte())

    @Test fun splitsNalsWithBothStartCodeLengths() {
        val au = sps(80, 45) + pps + idr
        assertEquals(listOf(H264.NAL_SPS, H264.NAL_PPS, H264.NAL_IDR), H264.nals(au).map { it.type })
        assertTrue(H264.hasSps(au))
        assertFalse(H264.hasSps(idr))
        assertTrue(H264.nals(byteArrayOf()).isEmpty())
    }

    @Test fun extractsParameterSetsWithStartCodes() {
        val s = sps(80, 45)
        val (gotS, gotP) = H264.parameterSets(s + pps + idr) ?: error("no parameter sets")
        assertArrayEquals(s, gotS)
        assertArrayEquals(pps, gotP)
        assertNull(H264.parameterSets(idr))
        assertNull(H264.parameterSets(s + idr))
    }

    @Test fun readsTheCodedSizeFromAnSps() {
        assertEquals(1280 to 720, H264.spsSize(sps(80, 45)))
        // 1088 coded rows, cropped by 4 × 2 (4:2:0 frame) = 1080.
        assertEquals(1920 to 1080, H264.spsSize(sps(120, 68, cropBottom = 4)))
        // Phone-shaped: 1088x2400 coded is 68x150 MBs; crop 4 right → 1080 wide.
        assertEquals(1080 to 2400, H264.spsSize(sps(68, 150, cropRight = 4)))
        assertNull(H264.spsSize(byteArrayOf(0x67)))
    }
}
