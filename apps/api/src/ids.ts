const BASE62 = '0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz';

/** Uniform base62 string from the OS CSPRNG (rejection sampling, no modulo bias). */
export function randomBase62(length: number): string {
  let out = '';
  while (out.length < length) {
    for (const byte of crypto.getRandomValues(new Uint8Array(length * 2))) {
      const v = byte & 63;
      if (v < 62 && out.length < length) out += BASE62.charAt(v);
    }
  }
  return out;
}

export type IdPrefix =
  'dev' | 'grp' | 'chl' | 'abk' | 'pol' | 'ses' | 'aud' | 'whk' | 'whd' | 'jit' | 'key';

const PUBLIC_ID_LENGTH = 20;

/** Public, unguessable identifier exposed by the API (`dev_…`). Internal PKs never leave the server. */
export function newId(prefix: IdPrefix): string {
  return `${prefix}_${randomBase62(PUBLIC_ID_LENGTH)}`;
}
