/** Deterministic fake data so screens look real and tests are stable. */

type DeviceOs = 'windows' | 'android' | 'macos' | 'linux';

export interface Device {
  id: string;
  name: string;
  os: DeviceOs;
  online: boolean;
  group: string;
  tags: string[];
  lastSeen: number;
}

export interface TrustedDevice {
  id: string;
  name: string;
  addedAt: number;
}

function mulberry32(seed: number) {
  let a = seed;
  return () => {
    a = (a + 0x6d2b79f5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4_294_967_296;
  };
}

const NAMES: readonly [string, ...string[]] = [
  'Reception PC',
  'Mom’s laptop',
  'Build server',
  'Studio workstation',
  'Galaxy A51',
  'Office NUC',
  'Warehouse kiosk',
  'Accounting desktop',
  'Media centre',
  'Dev box',
  'Front desk',
  'Conference room',
  'Pixel tablet',
  'Lab rig 2',
  'CAD station',
  'Point of sale',
];
const GROUPS = ['Family', 'Office', 'Clients', 'Lab'] as const;
const TAGS = ['windows-11', 'gaming', 'unattended', 'vip', 'printer', 'kiosk', 'ssh'] as const;
const OSES: readonly [DeviceOs, ...DeviceOs[]] = [
  'windows',
  'windows',
  'windows',
  'android',
  'linux',
  'macos',
];

const NOW = Date.UTC(2026, 9, 4, 12, 0, 0);

export function makeDevices(count: number, seed = 7): Device[] {
  const rnd = mulberry32(seed);
  const pick = <T>(list: readonly [T, ...T[]]): T =>
    list[Math.floor(rnd() * list.length)] ?? list[0];
  return Array.from({ length: count }, (_, i) => {
    const tagCount = Math.floor(rnd() * 3);
    const tags = [...new Set(Array.from({ length: tagCount }, () => pick(TAGS)))];
    return {
      id: String(100_000_000 + Math.floor(rnd() * 899_999_999)),
      name: `${pick(NAMES)}${i >= NAMES.length ? ` ${Math.floor(i / NAMES.length) + 1}` : ''}`,
      os: pick(OSES),
      online: rnd() > 0.45,
      group: pick(GROUPS),
      tags,
      lastSeen: NOW - Math.floor(rnd() * 21 * 86_400_000),
    };
  });
}

export const DEVICES: readonly Device[] = makeDevices(48);
export const DEVICE_GROUPS: readonly string[] = GROUPS;

export const TRUSTED: readonly TrustedDevice[] = DEVICES.slice(0, 3).map((d, i) => ({
  id: d.id,
  name: d.name,
  addedAt: NOW - (i + 1) * 9 * 86_400_000,
}));
