import type { Auth } from './auth/auth.ts';
import type { Permission } from './authz.ts';
import type { Db } from './db/client.ts';
import type { Logger } from './logger.ts';
import type { Mailer } from './mailer.ts';

export type Actor =
  | {
      kind: 'user';
      userId: string;
      orgId: string;
      role: string;
      permissions: ReadonlySet<Permission>;
    }
  | {
      kind: 'api_key';
      keyId: string;
      /** The user who created the key; actions are attributed to the key. */
      userId: string;
      orgId: string;
      permissions: ReadonlySet<Permission>;
    };

export interface AppDeps {
  db: Db;
  auth: Auth;
  logger: Logger;
  mailer: Mailer;
  ping: () => Promise<void>;
  webhookAllowInsecure: boolean;
  publicUrl: string;
  now?: () => Date;
}

export interface AppEnv {
  Variables: {
    requestId: string;
    log: Logger;
    actor: Actor;
  };
}
