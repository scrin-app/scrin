import {
  formatClock,
  formatCode,
  formatRelative,
  formatScrinId,
  useLocale,
  useTranslation,
} from '@scrin/i18n';
import {
  Button,
  Card,
  CardHeader,
  CodeDisplay,
  EmptyState,
  Field,
  IconButton,
  Input,
  Segmented,
  Skeleton,
  toast,
  useHost,
} from '@scrin/ui';
import { useQuery } from '@tanstack/react-query';
import { useNavigate } from '@tanstack/react-router';
import { useVirtualizer } from '@tanstack/react-virtual';
import { m } from 'motion/react';
import {
  ArrowRight,
  Copy,
  History,
  Laptop,
  Lightbulb,
  MessageSquareText,
  MonitorUp,
  RefreshCw,
  Share2,
  ShieldCheck,
} from 'lucide-react';
import { useEffect, useRef, useState, type SubmitEvent } from 'react';

import { PageHeader } from '../components/page-header';
import {
  PHRASE_TARGET,
  codeQuery,
  myIdQuery,
  passphraseQuery,
  useRegenerateCode,
  useSetPassphrase,
} from '../lib/engine';
import { usePending, usePrefs, type RecentConnection } from '../lib/prefs';
import { useStagger } from '../lib/use-stagger';

export function HomePage({ prefillId }: { prefillId?: string | undefined }) {
  const { t } = useTranslation();
  const stagger = useStagger();
  return (
    <>
      <PageHeader title={t('home.greeting')} subtitle={t('app.tagline')} />
      <div className="grid gap-4 @4xl:grid-cols-2 @7xl:grid-cols-[minmax(0,1fr)_minmax(0,1fr)_22rem]">
        <m.div {...stagger(0)}>
          <YourDeviceCard />
        </m.div>
        <m.div {...stagger(1)}>
          <ConnectCard key={prefillId ?? 'empty'} prefillId={prefillId} />
        </m.div>
        <m.div {...stagger(2)} className="@4xl:col-span-2 @7xl:col-span-1">
          <TipsCard />
        </m.div>
      </div>
    </>
  );
}

function YourDeviceCard() {
  const { t } = useTranslation();
  const host = useHost();
  const id = useQuery(myIdQuery);
  const code = useQuery(codeQuery);
  const regenerate = useRegenerateCode();
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(timer);
  }, []);

  const copy = async (text: string, message: string) => {
    await host.writeClipboard(text);
    toast.success(message);
  };

  const share = async () => {
    if (!id.data || !code.data || !host.share) return;
    await host.share({
      title: 'scrin',
      text: t('home.shareText', { id: formatScrinId(id.data), code: formatCode(code.data.code) }),
    });
  };

  const secondsLeft = code.data ? Math.max(0, (code.data.expiresAt - now) / 1000) : 0;

  return (
    <Card className="h-full">
      <CardHeader
        icon={<Laptop aria-hidden />}
        title={t('home.yourDevice')}
        description={t('home.yourDeviceHint')}
      />
      <div className="flex flex-col gap-5">
        {id.data ? (
          <CodeDisplay
            label={t('home.yourId')}
            value={formatScrinId(id.data)}
            onCopy={(text) => copy(text, t('home.idCopied'))}
          />
        ) : (
          <CodeSkeleton />
        )}
        <div className="h-px bg-outline" />
        {code.data ? (
          <div>
            <CodeDisplay
              label={t('home.oneTimeCode')}
              value={formatCode(code.data.code)}
              countdown={{ issuedAt: code.data.issuedAt, expiresAt: code.data.expiresAt }}
              onCopy={(text) => copy(text, t('home.codeCopied'))}
              actions={
                host.share ? (
                  <IconButton label={t('common.share')} onClick={() => void share()}>
                    <Share2 aria-hidden />
                  </IconButton>
                ) : null
              }
            />
            <div className="mt-3 flex flex-wrap items-center justify-between gap-2">
              <p className="text-xs text-muted" aria-live="off">
                {secondsLeft > 0
                  ? t('home.codeExpiresIn', { time: formatClock(secondsLeft) })
                  : t('home.codeExpired')}
              </p>
              <Button
                variant="secondary"
                size="sm"
                loading={regenerate.isPending}
                icon={<RefreshCw aria-hidden />}
                onClick={() =>
                  regenerate.mutate(undefined, {
                    onSuccess: () => toast.success(t('home.codeRegenerated')),
                  })
                }
              >
                {t('home.regenerate')}
              </Button>
            </div>
          </div>
        ) : (
          <CodeSkeleton ring />
        )}
        {host.engine.setPassphrase ? (
          <>
            <div className="h-px bg-outline" />
            <PassphrasePanel copy={copy} />
          </>
        ) : null}
      </div>
    </Card>
  );
}

/** D24: five dictated words instead of ID + code (host side). */
function PassphrasePanel({ copy }: { copy: (text: string, message: string) => Promise<void> }) {
  const { t } = useTranslation();
  const locale = useLocale();
  const phrase = useQuery(passphraseQuery);
  const set = useSetPassphrase();
  const words = phrase.data?.words.split(' ') ?? [];

  const show = () =>
    set.mutate(locale, {
      onSuccess: (p) => {
        if (!p) toast.error(t('home.phraseUnavailable'));
      },
    });

  return (
    <section aria-labelledby="phrase-title" className="flex flex-col gap-3">
      <div className="flex items-start gap-3">
        <MessageSquareText aria-hidden className="mt-0.5 size-5 shrink-0 text-muted" />
        <div className="min-w-0 flex-1">
          <h3 id="phrase-title" className="text-sm font-medium">
            {t('home.phraseTitle')}
          </h3>
          <p className="text-xs text-muted">{t('home.phraseHint')}</p>
        </div>
      </div>
      {words.length === 5 ? (
        <>
          <ol aria-label={t('home.phraseTitle')} className="flex flex-wrap gap-2 font-mono text-lg">
            {words.map((w, i) => (
              <li
                // Position is the identity: the same word may repeat.
                key={`${String(i)}-${w}`}
                className={
                  i < 2
                    ? 'rounded-md bg-surface-2 px-2.5 py-1'
                    : 'rounded-md bg-accent/15 px-2.5 py-1 text-fg ring-1 ring-accent/40 ring-inset'
                }
              >
                {w}
              </li>
            ))}
          </ol>
          <p className="text-xs text-muted">{t('home.phraseOnce')}</p>
          <div className="flex flex-wrap gap-2">
            <Button
              variant="secondary"
              size="sm"
              icon={<Copy aria-hidden />}
              onClick={() => void copy(words.join(' '), t('home.phraseCopied'))}
            >
              {t('common.copy')}
            </Button>
            <Button
              variant="secondary"
              size="sm"
              loading={set.isPending}
              icon={<RefreshCw aria-hidden />}
              onClick={show}
            >
              {t('home.phraseNew')}
            </Button>
            <Button variant="ghost" size="sm" onClick={() => set.mutate(null)}>
              {t('home.phraseHide')}
            </Button>
          </div>
        </>
      ) : (
        <div>
          <Button variant="secondary" size="sm" loading={set.isPending} onClick={show}>
            {t('home.phraseShow')}
          </Button>
        </div>
      )}
    </section>
  );
}

function CodeSkeleton({ ring = false }: { ring?: boolean }) {
  return (
    <div className="flex items-center gap-3" aria-hidden>
      {ring ? <Skeleton className="size-12 rounded-full" /> : null}
      <div className="flex-1 space-y-2">
        <Skeleton className="h-3 w-20" />
        <Skeleton className="h-9 w-56 max-w-full" />
      </div>
      <Skeleton className="size-10" />
    </div>
  );
}

function ConnectCard({ prefillId }: { prefillId?: string | undefined }) {
  const { t } = useTranslation();
  const host = useHost();
  const navigate = useNavigate();
  const setPendingCode = usePending((s) => s.setCode);
  const recent = usePrefs((s) => s.recent);
  const [mode, setMode] = useState<'id' | 'words'>('id');
  const [id, setId] = useState(prefillId ? formatScrinId(prefillId) : '');
  const [code, setCode] = useState('');
  const [words, setWords] = useState('');
  const [touched, setTouched] = useState(false);
  const codeRef = useRef<HTMLInputElement>(null);

  // Arriving with a prefilled ID (palette, devices list): the code is next.
  useEffect(() => {
    if (prefillId) codeRef.current?.focus();
  }, [prefillId]);

  const rawId = id.replace(/\D/g, '');
  const rawCode = code.replace(/\s/g, '');
  const wordList = words
    .trim()
    .split(/[\s,.-]+/)
    .filter(Boolean);
  const idError = touched && rawId.length !== 9 ? t('home.invalidId') : undefined;
  const codeError = touched && rawCode.length !== 8 ? t('home.invalidCode') : undefined;
  const wordsError = touched && wordList.length !== 5 ? t('home.invalidWords') : undefined;

  const submit = (e: SubmitEvent<HTMLFormElement>) => {
    e.preventDefault();
    setTouched(true);
    if (mode === 'words') {
      if (wordList.length !== 5) return;
      // The words travel as the pending "code"; the URL only says "phrase".
      setPendingCode(wordList.join(' '));
      void navigate({ to: '/connect/$id', params: { id: PHRASE_TARGET } });
      return;
    }
    if (rawId.length !== 9 || rawCode.length !== 8) return;
    setPendingCode(rawCode);
    void navigate({ to: '/connect/$id', params: { id: rawId } });
  };

  return (
    <Card className="flex h-full flex-col">
      <CardHeader
        icon={<MonitorUp aria-hidden />}
        title={t('home.connectTitle')}
        description={t('home.connectHint')}
      />
      {host.engine.supportsPassphrase ? (
        <Segmented
          className="mb-3"
          label={t('home.connectWith')}
          value={mode}
          onValueChange={(v) => {
            setMode(v);
            setTouched(false);
          }}
          options={[
            { value: 'id', label: t('home.modeIdCode') },
            { value: 'words', label: t('home.modeWords') },
          ]}
        />
      ) : null}
      <form
        onSubmit={submit}
        noValidate
        className={
          mode === 'words'
            ? 'grid gap-3 @md:grid-cols-[1fr_auto] @md:items-start'
            : 'grid gap-3 @md:grid-cols-[1fr_1fr_auto] @md:items-start'
        }
      >
        {mode === 'words' ? (
          <Field label={t('home.partnerWords')} error={wordsError}>
            <Input
              value={words}
              onChange={(e) => setWords(e.currentTarget.value)}
              autoComplete="off"
              autoCapitalize="none"
              spellCheck={false}
              placeholder={t('home.partnerWordsPlaceholder')}
              className="font-mono text-base"
            />
          </Field>
        ) : (
          <>
            <Field label={t('home.partnerId')} error={idError}>
              <Input
                value={id}
                onChange={(e) => setId(formatScrinId(e.currentTarget.value))}
                inputMode="numeric"
                autoComplete="off"
                placeholder={t('home.partnerIdPlaceholder')}
                className="font-mono text-base tracking-wider"
              />
            </Field>
            <Field label={t('home.partnerCode')} error={codeError}>
              <Input
                ref={codeRef}
                value={code}
                onChange={(e) => setCode(formatCode(e.currentTarget.value))}
                autoComplete="one-time-code"
                autoCapitalize="characters"
                spellCheck={false}
                placeholder={t('home.partnerCodePlaceholder')}
                className="font-mono text-base tracking-wider"
              />
            </Field>
          </>
        )}
        <Button
          type="submit"
          size="md"
          className="@md:mt-[1.625rem]"
          icon={<ArrowRight aria-hidden />}
        >
          {t('home.connect')}
        </Button>
      </form>

      <div className="mt-6 flex min-h-0 flex-1 flex-col">
        <h3 className="mb-2 flex items-center gap-2 text-sm font-medium text-muted">
          <History aria-hidden className="size-4" />
          {t('home.recent')}
        </h3>
        {recent.length === 0 ? (
          <EmptyState
            icon={<History aria-hidden />}
            title={t('home.recentEmpty')}
            className="py-8"
          />
        ) : (
          <RecentList
            items={recent}
            onPick={(r) => {
              setId(formatScrinId(r.id));
              codeRef.current?.focus();
            }}
          />
        )}
      </div>
    </Card>
  );
}

function RecentList({
  items,
  onPick,
}: {
  items: readonly RecentConnection[];
  onPick: (r: RecentConnection) => void;
}) {
  const { t } = useTranslation();
  const locale = useLocale();
  const parentRef = useRef<HTMLDivElement>(null);
  // eslint-disable-next-line react-hooks/incompatible-library -- TanStack Virtual returns unstable functions by design
  const virtualizer = useVirtualizer({
    count: items.length,
    getScrollElement: () => parentRef.current,
    estimateSize: () => 56,
    overscan: 6,
    initialRect: { width: 400, height: 280 },
  });
  return (
    <div ref={parentRef} className="max-h-72 overflow-y-auto rounded-lg border border-outline">
      <ul className="relative w-full" style={{ height: virtualizer.getTotalSize() }}>
        {virtualizer.getVirtualItems().map((row) => {
          const r = items[row.index];
          if (!r) return null;
          return (
            <li
              key={r.id}
              className="absolute inset-x-0 top-0"
              style={{ height: row.size, transform: `translateY(${row.start}px)` }}
            >
              <button
                type="button"
                onClick={() => onPick(r)}
                className="flex size-full items-center gap-3 px-3 text-left transition-colors duration-(--scrin-dur-fast) hover:bg-surface-2 focus-visible:-outline-offset-2"
              >
                <span className="grid size-9 shrink-0 place-items-center rounded-lg bg-surface-2 text-muted">
                  <Laptop aria-hidden className="size-4" />
                </span>
                <span className="min-w-0 flex-1">
                  <span className="block truncate text-sm font-medium text-fg">{r.name}</span>
                  <span className="block truncate text-xs text-muted">
                    <span className="font-mono">{formatScrinId(r.id)}</span> ·{' '}
                    {t('home.lastSeen', { time: formatRelative(locale, r.at) })}
                  </span>
                </span>
                <ArrowRight aria-hidden className="size-4 text-muted" />
              </button>
            </li>
          );
        })}
      </ul>
    </div>
  );
}

function TipsCard() {
  const { t } = useTranslation();
  const tips = [t('home.tip1'), t('home.tip2'), t('home.tip3')];
  return (
    <Card className="h-full">
      <CardHeader icon={<Lightbulb aria-hidden />} title={t('home.tipsTitle')} />
      <ul className="flex flex-col gap-3">
        {tips.map((tip) => (
          <li key={tip} className="flex gap-3 text-sm text-fg">
            <ShieldCheck aria-hidden className="mt-0.5 size-4 shrink-0 text-accent" />
            <span>{tip}</span>
          </li>
        ))}
      </ul>
    </Card>
  );
}
