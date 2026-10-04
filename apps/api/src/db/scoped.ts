import { and, eq, type SQL } from 'drizzle-orm';
import type { PgColumn } from 'drizzle-orm/pg-core';
import type { Db } from './client.ts';

interface OrgTable {
  orgId: PgColumn;
}

/**
 * Tenant boundary. Every read and write of a domain table goes through the
 * `where` of a scope, so a row of another organisation is indistinguishable
 * from a missing one (404, never 403).
 */
export function scoped(db: Db, orgId: string) {
  return {
    db,
    orgId,
    where(table: OrgTable, ...conditions: (SQL | undefined)[]): SQL {
      const org = eq(table.orgId, orgId);
      return and(org, ...conditions) ?? org;
    },
  };
}

export type Scope = ReturnType<typeof scoped>;
