import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { homedir } from 'node:os';
import { dirname, join } from 'node:path';

export interface CliConfig {
  baseUrl: string;
  apiKey: string;
}

export const DEFAULT_BASE_URL = 'https://api.scrin.dragoscatalin.ro';

/** `%APPDATA%\scrin\cli.json` on Windows, `$XDG_CONFIG_HOME/scrin/cli.json` elsewhere. */
export function configPath(env: NodeJS.ProcessEnv = process.env): string {
  if (env.SCRIN_CONFIG !== undefined && env.SCRIN_CONFIG !== '') return env.SCRIN_CONFIG;
  const base =
    process.platform === 'win32'
      ? (env.APPDATA ?? join(homedir(), 'AppData', 'Roaming'))
      : (env.XDG_CONFIG_HOME ?? join(homedir(), '.config'));
  return join(base, 'scrin', 'cli.json');
}

export async function readConfig(path: string): Promise<Partial<CliConfig>> {
  try {
    const raw: unknown = JSON.parse(await readFile(path, 'utf8'));
    if (typeof raw !== 'object' || raw === null) return {};
    const out: Partial<CliConfig> = {};
    if ('baseUrl' in raw && typeof raw.baseUrl === 'string') out.baseUrl = raw.baseUrl;
    if ('apiKey' in raw && typeof raw.apiKey === 'string') out.apiKey = raw.apiKey;
    return out;
  } catch {
    return {};
  }
}

/** Written with mode 0600; on Windows the per-user profile ACL protects it. */
export async function writeConfig(path: string, cfg: CliConfig): Promise<void> {
  await mkdir(dirname(path), { recursive: true });
  await writeFile(path, `${JSON.stringify(cfg, null, 2)}\n`, { encoding: 'utf8', mode: 0o600 });
}
