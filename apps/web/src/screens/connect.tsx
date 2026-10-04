import { formatScrinId, useTranslation, type TranslationKey } from '@scrin/i18n';
import {
  Button,
  Card,
  cn,
  type ConnectErrorKind,
  type ConnectStage,
  SasEmoji,
  Skeleton,
  useMotionDuration,
} from '@scrin/ui';
import { Link, useNavigate } from '@tanstack/react-router';
import { AnimatePresence, m } from 'motion/react';
import {
  ArrowLeft,
  Ban,
  Check,
  Clock,
  KeyRound,
  Radar,
  ShieldAlert,
  ShieldCheck,
  UserCheck,
  WifiOff,
  X,
  type LucideIcon,
} from 'lucide-react';
import { useEffect, useState } from 'react';

import { Logo } from '../components/logo';
import { useConnectFlow } from '../lib/engine';
import { usePending, usePrefs } from '../lib/prefs';

const STEPS: {
  stage: ConnectStage;
  label: TranslationKey;
  hint: TranslationKey;
  icon: LucideIcon;
}[] = [
  { stage: 'locating', label: 'connect.stepLocate', hint: 'connect.stepLocateHint', icon: Radar },
  {
    stage: 'securing',
    label: 'connect.stepSecure',
    hint: 'connect.stepSecureHint',
    icon: ShieldCheck,
  },
  {
    stage: 'awaiting-approval',
    label: 'connect.stepApprove',
    hint: 'connect.stepApproveHint',
    icon: UserCheck,
  },
  {
    stage: 'connected',
    label: 'connect.stepConnected',
    hint: 'connect.stepConnectedHint',
    icon: Check,
  },
];

const ERRORS: Record<
  ConnectErrorKind,
  { title: TranslationKey; fix: TranslationKey; icon: LucideIcon }
> = {
  offline: { title: 'connect.errorOffline', fix: 'connect.errorOfflineFix', icon: WifiOff },
  'wrong-code': {
    title: 'connect.errorWrongCode',
    fix: 'connect.errorWrongCodeFix',
    icon: KeyRound,
  },
  rejected: { title: 'connect.errorRejected', fix: 'connect.errorRejectedFix', icon: Ban },
  timeout: { title: 'connect.errorTimeout', fix: 'connect.errorTimeoutFix', icon: Clock },
  'network-blocked': {
    title: 'connect.errorBlocked',
    fix: 'connect.errorBlockedFix',
    icon: ShieldAlert,
  },
  'sas-mismatch': {
    title: 'connect.errorSasMismatch',
    fix: 'connect.errorSasMismatchFix',
    icon: ShieldAlert,
  },
};

export function ConnectPage({ id }: { id: string }) {
  const [attempt, setAttempt] = useState(0);
  return (
    <ConnectAttempt
      key={attempt}
      id={id}
      onRetry={() => {
        setAttempt((a) => a + 1);
      }}
    />
  );
}

function ConnectAttempt({ id, onRetry }: { id: string; onRetry: () => void }) {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const code = usePending((s) => s.code);
  const addRecent = usePrefs((s) => s.addRecent);
  const { state, confirmSas, cancel } = useConnectFlow(id, code);
  const d = useMotionDuration(0.3);

  // Without a code (reload, deep link) there is nothing to connect with.
  useEffect(() => {
    if (!code) void navigate({ to: '/', search: { id }, replace: true });
  }, [code, id, navigate]);

  const connectedAndVerified = state.stage === 'connected' && state.sasConfirmed && !state.error;
  useEffect(() => {
    if (!connectedAndVerified || !state.sessionId) return undefined;
    addRecent({ id, name: formatScrinId(id), at: Date.now() });
    const timer = setTimeout(() => {
      void navigate({
        to: '/session/$id',
        params: { id },
        search: { s: state.sessionId ?? '' },
        viewTransition: true,
      });
    }, 700);
    return () => clearTimeout(timer);
  }, [connectedAndVerified, state.sessionId, id, navigate, addRecent]);

  const activeIndex = STEPS.findIndex((s) => s.stage === state.stage);
  // The connected step only "completes" once the human confirmed the emoji.
  const progress = state.error
    ? activeIndex
    : state.stage === 'connected' && !state.sasConfirmed
      ? 2.5
      : activeIndex + (state.stage === 'connected' ? 1 : 0.5);

  const leave = () => {
    cancel();
    void navigate({ to: '/' });
  };

  return (
    <div className="flex min-h-dvh flex-col bg-bg text-fg">
      <header className="mx-auto flex h-16 w-full max-w-3xl items-center justify-between px-4 pt-[env(safe-area-inset-top)]">
        <Link to="/" className="rounded-md" aria-label={t('common.back')}>
          <Logo />
        </Link>
        <Button variant="ghost" icon={<X aria-hidden />} onClick={leave}>
          {t('connect.cancel')}
        </Button>
      </header>

      <main
        id="main"
        className="@container mx-auto flex w-full max-w-3xl flex-1 flex-col gap-6 px-4 pb-10"
      >
        <div className="text-center">
          <h1 className="text-2xl font-semibold tracking-tight sm:text-3xl">
            {t('connect.title', { id: formatScrinId(id) })}
          </h1>
          {state.route ? (
            <p className="mt-1 text-sm text-muted">
              {t('connect.via', {
                route:
                  state.route === 'direct' ? t('connect.routeDirect') : t('connect.routeRelay'),
              })}
            </p>
          ) : (
            <Skeleton className="mx-auto mt-2 h-4 w-28" />
          )}
        </div>

        <Card className="p-pad">
          <Stepper activeIndex={activeIndex} progress={progress} failed={state.error !== null} />
        </Card>

        <AnimatePresence mode="wait" initial={false}>
          {state.error ? (
            <m.div
              key="error"
              initial={{ opacity: 0, y: 8 }}
              animate={{ opacity: 1, y: 0 }}
              exit={{ opacity: 0, y: -8 }}
              transition={{ duration: d }}
            >
              <ErrorPanel kind={state.error} onRetry={onRetry} onBack={leave} />
            </m.div>
          ) : state.sas && !state.sasConfirmed ? (
            <m.div
              key="sas"
              initial={{ opacity: 0, scale: 0.98 }}
              animate={{ opacity: 1, scale: 1 }}
              exit={{ opacity: 0, scale: 0.98 }}
              transition={{ duration: d }}
            >
              <Card className="@container">
                <h2 className="text-lg font-semibold">{t('connect.sasTitle')}</h2>
                <p className="mt-1 mb-4 text-sm text-muted">{t('connect.sasHint')}</p>
                <SasEmoji indices={state.sas} />
                <div className="mt-5 flex flex-wrap justify-end gap-2">
                  <Button
                    variant="outline"
                    icon={<X aria-hidden />}
                    onClick={() => void confirmSas(false)}
                  >
                    {t('connect.sasMismatch')}
                  </Button>
                  <Button icon={<Check aria-hidden />} onClick={() => void confirmSas(true)}>
                    {t('connect.sasMatch')}
                  </Button>
                </div>
              </Card>
            </m.div>
          ) : null}
        </AnimatePresence>
      </main>
    </div>
  );
}

function Stepper({
  activeIndex,
  progress,
  failed,
}: {
  activeIndex: number;
  progress: number;
  failed: boolean;
}) {
  const { t } = useTranslation();
  const ratio = Math.min(1, Math.max(0, progress / (STEPS.length - 1)));
  return (
    <ol className="relative grid grid-cols-4 gap-2">
      {/* Morphing track: one scaleX transform, compositor only. */}
      <div
        aria-hidden
        className="absolute top-5 right-[12.5%] left-[12.5%] h-1 rounded-full bg-surface-2"
      >
        <div
          className={cn(
            'h-full origin-left rounded-full transition-transform duration-(--scrin-dur-slow) ease-scrin',
            failed ? 'bg-danger' : 'bg-accent',
          )}
          style={{ transform: `scaleX(${ratio})` }}
        />
      </div>
      {STEPS.map((step, i) => {
        const done =
          i < activeIndex || (i === activeIndex && progress >= STEPS.length - 1 && !failed);
        const active = i === activeIndex && !done;
        const Icon = step.icon;
        return (
          <li
            key={step.stage}
            className="relative flex flex-col items-center gap-2 text-center"
            aria-current={active ? 'step' : undefined}
          >
            <span
              className={cn(
                'relative z-10 grid size-10 place-items-center rounded-full border-2 transition-colors duration-(--scrin-dur)',
                done && 'border-accent bg-accent text-accent-fg',
                active && !failed && 'border-accent bg-surface text-accent',
                active && failed && 'border-danger bg-surface text-danger',
                !done && !active && 'border-outline bg-surface text-muted',
              )}
            >
              {active && !failed ? (
                <span
                  aria-hidden
                  className="absolute inset-0 animate-ping rounded-full border-2 border-accent opacity-40"
                />
              ) : null}
              {done ? (
                <Check aria-hidden className="size-5" />
              ) : (
                <Icon aria-hidden className="size-5" />
              )}
            </span>
            <span
              className={cn(
                'text-xs font-medium sm:text-sm',
                active || done ? 'text-fg' : 'text-muted',
              )}
            >
              {t(step.label)}
            </span>
            <span className="hidden text-xs text-muted @xl:block">{t(step.hint)}</span>
          </li>
        );
      })}
    </ol>
  );
}

function ErrorPanel({
  kind,
  onRetry,
  onBack,
}: {
  kind: ConnectErrorKind;
  onRetry: () => void;
  onBack: () => void;
}) {
  const { t } = useTranslation();
  const e = ERRORS[kind];
  const Icon = e.icon;
  return (
    <Card className="border-danger/40" role="alert">
      <div className="flex gap-4">
        <span className="grid size-12 shrink-0 place-items-center rounded-xl bg-danger/12 text-danger">
          <Icon aria-hidden className="size-6" />
        </span>
        <div className="min-w-0">
          <p className="text-xs font-medium tracking-wide text-muted uppercase">
            {t('connect.errorTitle')}
          </p>
          <h2 className="mt-0.5 text-lg font-semibold">{t(e.title)}</h2>
          <p className="mt-1 text-sm text-muted">{t(e.fix)}</p>
        </div>
      </div>
      <div className="mt-5 flex flex-wrap justify-end gap-2">
        {kind === 'network-blocked' ? (
          <Link
            to="/settings"
            hash="network"
            className="mr-auto text-sm font-medium text-accent underline-offset-4 hover:underline"
          >
            {t('connect.runDiagnostics')}
          </Link>
        ) : null}
        <Button variant="ghost" icon={<ArrowLeft aria-hidden />} onClick={onBack}>
          {t('common.back')}
        </Button>
        {kind !== 'sas-mismatch' && kind !== 'wrong-code' ? (
          <Button onClick={onRetry}>{t('common.retry')}</Button>
        ) : null}
      </div>
    </Card>
  );
}
