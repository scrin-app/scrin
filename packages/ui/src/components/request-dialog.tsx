import { AlertDialog as BaseAlert } from '@base-ui/react/alert-dialog';
import { useTranslation } from '@scrin/i18n';
import { BadgeCheck, Ban, Clock, ShieldAlert, ShieldCheck, UserRound } from 'lucide-react';
import { useEffect, useId, useRef, useState, type ReactNode } from 'react';

import { cn } from '../lib/cn';
import { Button } from './button';
import { Switch } from './controls';
import { Badge } from './display';
import type { IncomingRequest } from './host-role';
import {
  ANONYMOUS_FORBIDDEN,
  ANONYMOUS_MAX_MINUTES,
  PERMISSION_LABEL_KEY,
  PERMISSIONS,
  type PermissionName,
  type SessionKindName,
} from './permissions';
import { SasEmoji } from './sas-emoji';

const KIND_KEY = {
  anonymous: 'request.kindAnonymous',
  verified: 'request.kindVerified',
  trusted: 'request.kindTrusted',
  org: 'request.kindOrg',
} as const satisfies Record<SessionKindName, `request.${string}`>;

export interface RequestDialogProps {
  request: IncomingRequest;
  onAccept: (permissions: PermissionName[]) => void;
  /** `expired` when the countdown ran out, `user` when Deny was pressed. */
  onDeny: (reason: 'user' | 'expired') => void;
  /** Clock source; tests inject one. */
  now?: () => number;
}

/** Initial toggles: what was asked for, clipped to the policy ceiling. */
function initialGrants(req: IncomingRequest): Set<PermissionName> {
  const allowed = new Set(req.allowed);
  const asked: readonly PermissionName[] = req.requested.length > 0 ? req.requested : SUPPORT_SET;
  return new Set(
    asked.filter(
      (p) => allowed.has(p) && !(req.kind === 'anonymous' && ANONYMOUS_FORBIDDEN.has(p)),
    ),
  );
}

/** `Permissions::support()` — what a quick-support request asks for by default. */
const SUPPORT_SET: readonly PermissionName[] = ['view', 'input', 'clipboard', 'chat'];

/**
 * The host-side answer to an incoming connection (ADR-0009). It cannot be
 * dismissed by clicking outside or pressing Escape: the only ways out are
 * Accept, Deny, or the countdown, which denies. Accept stays disabled until
 * the anti-scam delay has passed; focus starts on Deny.
 */
export function RequestDialog({ request, onAccept, onDeny, now = Date.now }: RequestDialogProps) {
  const { t } = useTranslation();
  const [clock, setClock] = useState(now);
  const [grants, setGrants] = useState(() => initialGrants(request));
  const denyRef = useRef<HTMLButtonElement>(null);
  const answered = useRef(false);
  const descId = useId();

  useEffect(() => {
    const id = setInterval(() => setClock(now()), 250);
    return () => clearInterval(id);
  }, [now]);

  const left = Math.max(0, Math.ceil((request.expiresAt - clock) / 1000));
  const acceptIn = Math.max(0, Math.ceil((request.acceptEnabledAt - clock) / 1000));
  const expired = clock >= request.expiresAt;

  useEffect(() => {
    if (expired && !answered.current) {
      answered.current = true;
      onDeny('expired');
    }
  }, [expired, onDeny]);

  const answer = (accept: boolean) => {
    if (answered.current) return;
    answered.current = true;
    if (accept) onAccept(PERMISSIONS.filter((p) => grants.has(p)));
    else onDeny('user');
  };

  const allowed = new Set(request.allowed);
  const anonymous = request.kind === 'anonymous';
  const total = Math.max(1, request.expiresAt - request.acceptEnabledAt);
  const ratio = Math.min(1, Math.max(0, (request.expiresAt - clock) / total));

  return (
    <BaseAlert.Root open onOpenChange={() => undefined}>
      <BaseAlert.Portal>
        <BaseAlert.Backdrop className="fixed inset-0 z-50 bg-black/55 backdrop-blur-[2px] transition-opacity duration-(--scrin-dur) data-ending-style:opacity-0 data-starting-style:opacity-0" />
        <BaseAlert.Popup
          initialFocus={denyRef}
          aria-describedby={descId}
          className={cn(
            '@container fixed top-1/2 left-1/2 z-50 flex max-h-[calc(100dvh-1rem)] w-[min(36rem,calc(100vw-1rem))] -translate-x-1/2 -translate-y-1/2 flex-col',
            'overflow-hidden rounded-xl glass-panel shadow-2xl outline-none',
            'transition-[opacity,transform] duration-(--scrin-dur) ease-scrin',
            'data-ending-style:scale-95 data-ending-style:opacity-0 data-starting-style:scale-95 data-starting-style:opacity-0',
          )}
        >
          <div className="flex-1 overflow-y-auto p-5 @md:p-6">
            <BaseAlert.Title className="text-lg font-semibold text-fg">
              {t('request.title')}
            </BaseAlert.Title>

            <section
              id={descId}
              aria-labelledby={`${descId}-scam`}
              className="mt-4 flex gap-3 rounded-lg bg-danger/12 p-3 ring-1 ring-danger/40 ring-inset"
            >
              <ShieldAlert aria-hidden className="mt-0.5 size-5 shrink-0 text-danger" />
              <div>
                <h3 id={`${descId}-scam`} className="text-sm font-semibold text-fg">
                  {t('request.scamTitle')}
                </h3>
                <p className="mt-1 text-sm text-fg">{t('request.scamBody')}</p>
              </div>
            </section>

            <dl className="mt-4 grid gap-x-4 gap-y-2 text-sm @md:grid-cols-[auto_minmax(0,1fr)]">
              <dt className="text-muted">{t('request.from')}</dt>
              <dd className="flex min-w-0 flex-wrap items-center gap-2">
                <UserRound aria-hidden className="size-4 text-muted" />
                <span className="truncate font-medium text-fg">
                  {request.peer ? shortPeer(request.peer) : t('request.unknownPeer')}
                </span>
                <Badge tone={anonymous ? 'warning' : 'success'} data-kind={request.kind}>
                  {anonymous ? <ShieldAlert aria-hidden /> : <BadgeCheck aria-hidden />}
                  {t(KIND_KEY[request.kind])}
                </Badge>
              </dd>
              {request.fingerprint ? (
                <>
                  <dt className="text-muted">{t('request.fingerprint')}</dt>
                  <dd className="font-mono text-xs break-all text-fg">{request.fingerprint}</dd>
                </>
              ) : null}
            </dl>

            {anonymous ? (
              <section aria-labelledby={`${descId}-caps`} className="mt-4">
                <h3 id={`${descId}-caps`} className="text-sm font-semibold text-fg">
                  {t('request.capsTitle')}
                </h3>
                <ul className="mt-2 grid gap-1.5 text-sm text-fg @md:grid-cols-2">
                  <Cap icon={<Clock aria-hidden />}>
                    {t('request.capsDuration', { minutes: ANONYMOUS_MAX_MINUTES })}
                  </Cap>
                  <Cap icon={<Ban aria-hidden />}>{t('request.capsFiles')}</Cap>
                  <Cap icon={<Ban aria-hidden />}>{t('request.capsUnattended')}</Cap>
                  <Cap icon={<Ban aria-hidden />}>{t('request.capsPrivacy')}</Cap>
                </ul>
              </section>
            ) : null}

            {request.sas ? (
              <section aria-labelledby={`${descId}-sas`} className="mt-4">
                <h3 id={`${descId}-sas`} className="text-sm font-semibold text-fg">
                  {t('request.sasTitle')}
                </h3>
                <p className="mt-0.5 mb-2 text-xs text-muted">{t('request.sasHint')}</p>
                <SasEmoji indices={request.sas} />
              </section>
            ) : null}

            <fieldset className="mt-4">
              <legend className="text-sm font-semibold text-fg">
                {t('request.permissionsTitle')}
              </legend>
              <ul className="mt-2 grid gap-x-4 @md:grid-cols-2">
                {PERMISSIONS.map((p) => {
                  const permitted = allowed.has(p) && !(anonymous && ANONYMOUS_FORBIDDEN.has(p));
                  return (
                    <PermissionRow
                      key={p}
                      name={p}
                      label={t(PERMISSION_LABEL_KEY[p])}
                      hint={permitted ? undefined : t('request.notAllowed')}
                      checked={permitted && grants.has(p)}
                      disabled={!permitted}
                      onChange={(on) =>
                        setGrants((g) => {
                          const next = new Set(g);
                          if (on) next.add(p);
                          else next.delete(p);
                          return next;
                        })
                      }
                    />
                  );
                })}
              </ul>
            </fieldset>
          </div>

          <footer className="flex flex-wrap items-center gap-3 border-t border-outline bg-surface/60 px-5 py-3 @md:px-6">
            <p
              className="mr-auto flex items-center gap-2 text-xs text-muted"
              data-testid="auto-deny"
            >
              <span
                aria-hidden
                className="block h-1.5 w-16 overflow-hidden rounded-full bg-surface-2"
              >
                <span
                  className="block h-full w-full origin-left bg-warning transition-transform duration-200 ease-linear"
                  style={{ transform: `scaleX(${ratio})` }}
                />
              </span>
              {t('request.autoDeny', { seconds: left })}
            </p>
            <Button ref={denyRef} variant="outline" onClick={() => answer(false)}>
              {t('request.deny')}
            </Button>
            <Button
              variant={anonymous ? 'secondary' : 'primary'}
              icon={<ShieldCheck aria-hidden />}
              disabled={acceptIn > 0 || expired}
              focusableWhenDisabled
              onClick={() => answer(true)}
            >
              {acceptIn > 0 ? t('request.acceptIn', { seconds: acceptIn }) : t('request.accept')}
            </Button>
          </footer>
        </BaseAlert.Popup>
      </BaseAlert.Portal>
    </BaseAlert.Root>
  );
}

function Cap({ icon, children }: { icon: ReactNode; children: ReactNode }) {
  return (
    <li className="flex items-center gap-2 [&_svg]:size-4 [&_svg]:shrink-0 [&_svg]:text-warning">
      {icon}
      {children}
    </li>
  );
}

function PermissionRow({
  name,
  label,
  hint,
  checked,
  disabled,
  onChange,
}: {
  name: PermissionName;
  label: string;
  hint: string | undefined;
  checked: boolean;
  disabled: boolean;
  onChange: (on: boolean) => void;
}) {
  const id = useId();
  return (
    <li
      data-permission={name}
      className={cn('flex items-center justify-between gap-3 py-1.5', disabled && 'opacity-60')}
    >
      <span className="min-w-0">
        <label htmlFor={id} className="block text-sm text-fg">
          {label}
        </label>
        {hint ? (
          <span id={`${id}-h`} className="block text-xs text-muted">
            {hint}
          </span>
        ) : null}
      </span>
      <Switch
        id={id}
        checked={checked}
        disabled={disabled}
        onCheckedChange={(c: boolean) => onChange(c)}
        {...(hint ? { 'aria-describedby': `${id}-h` } : {})}
      />
    </li>
  );
}

/** A 64-hex device key shown as its first and last groups. */
function shortPeer(peer: string): string {
  return /^[0-9a-f]{64}$/i.test(peer) ? `${peer.slice(0, 8)}…${peer.slice(-8)}` : peer;
}
