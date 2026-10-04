import { passkey } from '@better-auth/passkey';
import { betterAuth } from 'better-auth';
import { drizzleAdapter } from 'better-auth/adapters/drizzle';
import { organization, twoFactor } from 'better-auth/plugins';
import { createAccessControl } from 'better-auth/plugins/access';
import { adminAc, defaultStatements, ownerAc } from 'better-auth/plugins/organization/access';
import type { Db } from '../db/client.ts';
import { authSchema } from '../db/schema/auth.ts';
import type { Logger } from '../logger.ts';
import type { Mailer } from '../mailer.ts';

// Organisation-management permissions enforced by better-auth's own endpoints
// (invite, change role, delete org). Domain permissions live in ../authz.ts.
const ac = createAccessControl(defaultStatements);
const orgRoles = {
  owner: ac.newRole({ ...ownerAc.statements }),
  admin: ac.newRole({ ...adminAc.statements }),
  technician: ac.newRole({ organization: [], member: [], invitation: [], team: [], ac: ['read'] }),
  viewer: ac.newRole({ organization: [], member: [], invitation: [], team: [], ac: ['read'] }),
};

export interface AuthConfig {
  db: Db;
  secret: string;
  baseURL: string;
  appURL: string;
  trustedOrigins: string[];
  passkeyRpId: string;
  requireEmailVerification: boolean;
  mailer: Mailer;
  logger: Logger;
}

interface AuthSession {
  user: { id: string; email: string };
  session: { activeOrganizationId?: string | null | undefined };
}

/** The part of better-auth the API uses; keeps the exported type portable. */
export interface Auth {
  handler: (request: Request) => Promise<Response>;
  getSession: (headers: Headers) => Promise<AuthSession | null>;
}

export function createAuth(cfg: AuthConfig): Auth {
  const origins = [cfg.baseURL, cfg.appURL, ...cfg.trustedOrigins];
  const auth = betterAuth({
    appName: 'scrin',
    secret: cfg.secret,
    baseURL: cfg.baseURL,
    basePath: '/api/auth',
    trustedOrigins: origins,
    database: drizzleAdapter(cfg.db, { provider: 'pg', schema: authSchema }),
    logger: {
      // Route better-auth's own logs into pino; it never receives secrets from us.
      log: (level, message) => {
        cfg.logger[level]({ source: 'better-auth' }, message);
      },
    },
    emailAndPassword: {
      enabled: true,
      // Password hashing: better-auth's default scrypt (N=16384, r=16, p=1).
      minPasswordLength: 12,
      maxPasswordLength: 256,
      requireEmailVerification: cfg.requireEmailVerification,
      sendResetPassword: async ({ user, url }) => {
        await cfg.mailer.send({
          to: user.email,
          subject: 'Reset your scrin password',
          text: `Open this link to choose a new password (valid for 1 hour):\n\n${url}\n\nIf you did not ask for this, ignore this email.`,
        });
      },
    },
    emailVerification: {
      sendOnSignUp: cfg.requireEmailVerification,
      autoSignInAfterVerification: true,
      sendVerificationEmail: async ({ user, url }) => {
        await cfg.mailer.send({
          to: user.email,
          subject: 'Verify your scrin email',
          text: `Confirm your email address for scrin:\n\n${url}`,
        });
      },
    },
    advanced: {
      useSecureCookies: cfg.baseURL.startsWith('https://'),
      cookiePrefix: 'scrin',
    },
    plugins: [
      passkey({ rpID: cfg.passkeyRpId, rpName: 'scrin', origin: origins }),
      twoFactor({ issuer: 'scrin' }),
      organization({
        ac,
        roles: orgRoles,
        creatorRole: 'owner',
        sendInvitationEmail: async ({ email, organization: org, invitation, inviter }) => {
          await cfg.mailer.send({
            to: email,
            subject: `${inviter.user.name} invited you to ${org.name} on scrin`,
            text: `You were invited to join ${org.name} as ${invitation.role}.\n\nAccept: ${cfg.appURL}/invite/${invitation.id}`,
          });
        },
      }),
    ],
  });
  return {
    handler: (request) => auth.handler(request),
    getSession: (headers) => auth.api.getSession({ headers }),
  };
}
