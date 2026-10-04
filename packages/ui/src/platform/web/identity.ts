/**
 * The browser's device identity (an Ed25519 key, `DeviceId` = public key).
 *
 * Preferred: a WebCrypto Ed25519 key pair generated **non-extractable** and
 * kept in IndexedDB as a `CryptoKey` (structured clone); the private key never
 * exists as bytes in JS. Only the 32-byte public key enters wasm (SPAKE2 binds
 * device ids only), and `Attest` is signed by `crypto.subtle.sign`.
 *
 * Fallback (browsers without WebCrypto Ed25519): a random 32-byte seed sealed
 * with a non-extractable AES-GCM key, both in IndexedDB; the seed is decrypted
 * only to sign (in wasm) and zeroed afterwards.
 *
 * Without IndexedDB (private mode, tests) the identity lives in memory only.
 */

import { ownedBytes } from '@scrin/protocol';

export interface BrowserIdentity {
  readonly publicKey: Uint8Array;
  readonly kind: 'webcrypto' | 'sealed-seed' | 'ephemeral';
  sign(msg: Uint8Array): Promise<Uint8Array>;
}

export interface SeedSigner {
  seedPublicKey(seed: Uint8Array): Uint8Array;
  seedSign(seed: Uint8Array, msg: Uint8Array): Uint8Array;
}

const DB = 'scrin';
const STORE = 'keys';
const KEY = 'device-v1';

type Stored =
  | { kind: 'webcrypto'; pair: CryptoKeyPair; publicKey: Uint8Array }
  | {
      kind: 'sealed-seed';
      wrap: CryptoKey;
      iv: Uint8Array;
      sealed: Uint8Array;
      publicKey: Uint8Array;
    };

function idb(): IDBFactory | null {
  try {
    return typeof indexedDB === 'undefined' ? null : indexedDB;
  } catch {
    return null;
  }
}

function request<T>(r: IDBRequest<T>): Promise<T> {
  return new Promise((resolve, reject) => {
    r.addEventListener('success', () => resolve(r.result));
    r.addEventListener('error', () => reject(r.error ?? new Error('IndexedDB error')));
  });
}

async function openDb(f: IDBFactory): Promise<IDBDatabase> {
  const r = f.open(DB, 1);
  r.addEventListener('upgradeneeded', () => {
    if (!r.result.objectStoreNames.contains(STORE)) r.result.createObjectStore(STORE);
  });
  return request(r);
}

function isStored(v: unknown): v is Stored {
  if (typeof v !== 'object' || v === null || !('kind' in v) || !('publicKey' in v)) return false;
  if (!(v.publicKey instanceof Uint8Array) || v.publicKey.length !== 32) return false;
  if (v.kind === 'webcrypto') return 'pair' in v;
  return v.kind === 'sealed-seed' && 'wrap' in v && v.wrap instanceof CryptoKey;
}

async function readStored(db: IDBDatabase): Promise<Stored | undefined> {
  const v: unknown = await request(db.transaction(STORE).objectStore(STORE).get(KEY));
  return isStored(v) ? v : undefined;
}

async function writeStored(db: IDBDatabase, v: Stored): Promise<void> {
  await request(db.transaction(STORE, 'readwrite').objectStore(STORE).put(v, KEY));
}

const bytes = (b: ArrayBuffer | Uint8Array) => (b instanceof Uint8Array ? b : new Uint8Array(b));
const buf = ownedBytes;

async function webCryptoPair(): Promise<{ pair: CryptoKeyPair; publicKey: Uint8Array } | null> {
  try {
    const pair = await crypto.subtle.generateKey({ name: 'Ed25519' }, false, ['sign', 'verify']);
    if (!('privateKey' in pair)) return null;
    const publicKey = bytes(await crypto.subtle.exportKey('raw', pair.publicKey));
    return publicKey.length === 32 ? { pair, publicKey } : null;
  } catch {
    return null;
  }
}

function fromStored(s: Stored, signer: SeedSigner): BrowserIdentity {
  if (s.kind === 'webcrypto') {
    return {
      kind: 'webcrypto',
      publicKey: s.publicKey,
      async sign(msg) {
        return bytes(await crypto.subtle.sign({ name: 'Ed25519' }, s.pair.privateKey, buf(msg)));
      },
    };
  }
  return {
    kind: 'sealed-seed',
    publicKey: s.publicKey,
    async sign(msg) {
      const seed = bytes(
        await crypto.subtle.decrypt({ name: 'AES-GCM', iv: buf(s.iv) }, s.wrap, buf(s.sealed)),
      );
      try {
        return signer.seedSign(seed, msg);
      } finally {
        seed.fill(0);
      }
    },
  };
}

async function sealedSeed(signer: SeedSigner): Promise<Stored> {
  const wrap = await crypto.subtle.generateKey({ name: 'AES-GCM', length: 256 }, false, [
    'encrypt',
    'decrypt',
  ]);
  const seed = crypto.getRandomValues(new Uint8Array(32));
  const iv = crypto.getRandomValues(new Uint8Array(12));
  try {
    const sealed = bytes(await crypto.subtle.encrypt({ name: 'AES-GCM', iv }, wrap, seed));
    return { kind: 'sealed-seed', wrap, iv, sealed, publicKey: signer.seedPublicKey(seed) };
  } finally {
    seed.fill(0);
  }
}

function ephemeral(signer: SeedSigner): BrowserIdentity {
  const seed = crypto.getRandomValues(new Uint8Array(32));
  return {
    kind: 'ephemeral',
    publicKey: signer.seedPublicKey(seed),
    sign: (msg) => Promise.resolve(signer.seedSign(seed, msg)),
  };
}

/** Loads (or creates once) this browser's identity. */
export async function loadIdentity(signer: SeedSigner): Promise<BrowserIdentity> {
  const f = idb();
  if (!f) return ephemeral(signer);
  let db: IDBDatabase;
  try {
    db = await openDb(f);
  } catch {
    return ephemeral(signer);
  }
  try {
    const existing = await readStored(db);
    if (existing) return fromStored(existing, signer);
    const wc = await webCryptoPair();
    const created: Stored = wc ? { kind: 'webcrypto', ...wc } : await sealedSeed(signer);
    await writeStored(db, created);
    return fromStored(created, signer);
  } finally {
    db.close();
  }
}
