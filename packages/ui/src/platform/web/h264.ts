/**
 * H.264 Annex B helpers for WebCodecs: NAL unit scanning and the
 * `avc1.PPCCLL` codec string from the SPS (RFC 6381 §3.3).
 */

const NAL_SPS = 7;
const NAL_IDR = 5;

/** Byte ranges of the NAL units (without start codes) of an Annex B buffer. */
export function nalUnits(au: Uint8Array): Uint8Array[] {
  const out: Uint8Array[] = [];
  let start = -1;
  let i = 0;
  while (i + 2 < au.length) {
    if (au[i] === 0 && au[i + 1] === 0 && au[i + 2] === 1) {
      if (start >= 0) {
        // A 4-byte start code leaves one trailing zero on the previous unit.
        let end = i;
        if (end > start && au[end - 1] === 0) end -= 1;
        out.push(au.subarray(start, end));
      }
      i += 3;
      start = i;
    } else i += 1;
  }
  if (start >= 0 && start < au.length) out.push(au.subarray(start));
  return out;
}

export const nalType = (nal: Uint8Array): number => (nal[0] ?? 0) & 0x1f;

const hex = (b: number | undefined) => (b ?? 0).toString(16).padStart(2, '0');

/** `avc1.PPCCLL` from the first SPS of an access unit, or `null`. */
export function codecStringFromAnnexB(au: Uint8Array): string | null {
  const sps = nalUnits(au).find((n) => nalType(n) === NAL_SPS);
  if (!sps || sps.length < 4) return null;
  return `avc1.${hex(sps[1])}${hex(sps[2])}${hex(sps[3])}`;
}

/** Whether the access unit carries an IDR slice. */
export function hasIdr(au: Uint8Array): boolean {
  return nalUnits(au).some((n) => nalType(n) === NAL_IDR);
}
