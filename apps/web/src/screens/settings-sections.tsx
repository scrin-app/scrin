import { Radio } from '@base-ui/react/radio';
import { RadioGroup as BaseRadioGroup } from '@base-ui/react/radio-group';
import {
  formatDate,
  LOCALE_NAMES,
  LOCALES,
  setLocale,
  useLocale,
  useTranslation,
  type Locale,
} from '@scrin/i18n';
import {
  ACCENT_PRESETS,
  Badge,
  Button,
  Card,
  cn,
  EmptyState,
  Field,
  Input,
  loadHostExtras,
  RadioGroup,
  Segmented,
  Select,
  Skeleton,
  Slider,
  SwitchRow,
  toast,
  Tooltip,
  useHost,
  useTheme,
  type Density,
  type HostExtras,
  type Mode,
  type MotionLevel,
  type StreamQuality,
  type Surface,
  type UpdateCheck,
} from '@scrin/ui';
import {
  Check,
  Cog,
  Download,
  ExternalLink,
  Gauge,
  Info,
  KeyRound,
  Keyboard,
  Languages,
  Monitor,
  Moon,
  Network,
  Palette,
  RefreshCw,
  ShieldCheck,
  Sun,
  Volume2,
} from 'lucide-react';
import { useEffect, useState, type ReactNode } from 'react';

import { usePrefs, type Prefs } from '../lib/prefs';
import type { SectionId } from './settings';
import { useUiPref } from './ui-prefs';

const SOURCE_URL = 'https://github.com/scrin-app/scrin';
const LINKS = [
  { labelKey: 'settings.website', href: 'https://scrin.dragoscatalin.ro' },
  { labelKey: 'settings.source', href: SOURCE_URL },
  { labelKey: 'settings.reportIssue', href: `${SOURCE_URL}/issues` },
  { labelKey: 'settings.securityPolicy', href: `${SOURCE_URL}/security/policy` },
] as const;

/** `undefined` while loading, `null` when this host has no extras (web). */
function useExtras(): HostExtras | null | undefined {
  const host = useHost();
  const [extras, setExtras] = useState<HostExtras | null | undefined>(undefined);
  useEffect(() => {
    let live = true;
    void loadHostExtras(host).then((x) => {
      if (live) setExtras(x);
    });
    return () => {
      live = false;
    };
  }, [host]);
  return extras;
}

function usePref<K extends keyof Prefs>(key: K): [Prefs[K], (v: Prefs[K]) => void] {
  const value = usePrefs((s) => s.prefs[key]);
  const set = usePrefs((s) => s.set);
  return [
    value,
    (v) => {
      const patch: Partial<Prefs> = {};
      patch[key] = v;
      set(patch);
    },
  ];
}

const ICONS: Record<SectionId, ReactNode> = {
  general: <Cog aria-hidden />,
  security: <ShieldCheck aria-hidden />,
  network: <Network aria-hidden />,
  video: <Monitor aria-hidden />,
  audio: <Volume2 aria-hidden />,
  input: <Keyboard aria-hidden />,
  appearance: <Palette aria-hidden />,
  language: <Languages aria-hidden />,
  updates: <Download aria-hidden />,
  about: <Info aria-hidden />,
};

const BODIES: Record<SectionId, () => ReactNode> = {
  general: () => <GeneralSection />,
  security: () => <SecuritySection />,
  network: () => <NetworkSection />,
  video: () => <VideoSection />,
  audio: () => <AudioSection />,
  input: () => <InputSection />,
  appearance: () => <AppearanceSection />,
  language: () => <LanguageSection />,
  updates: () => <UpdatesSection />,
  about: () => <AboutSection />,
};

export function SettingsSection({ id, title }: { id: SectionId; title: string }) {
  const Body = BODIES[id];
  return (
    <Card id={id} aria-labelledby={`${id}-h`} className="@container">
      <header className="mb-4 flex items-center gap-3">
        <span className="grid size-9 place-items-center rounded-lg bg-accent/12 text-accent [&_svg]:size-4.5">
          {ICONS[id]}
        </span>
        <h2 id={`${id}-h`} className="text-base font-semibold">
          {title}
        </h2>
      </header>
      <div className="flex flex-col gap-3">
        <Body />
      </div>
    </Card>
  );
}

function Group({ label, hint, children }: { label: string; hint?: string; children: ReactNode }) {
  return (
    <div>
      <p className="mb-2 text-sm font-medium text-fg">{label}</p>
      {children}
      {hint ? <p className="mt-1.5 text-xs text-muted">{hint}</p> : null}
    </div>
  );
}

function sliderValue(v: number | readonly number[], fallback: number): number {
  return typeof v === 'number' ? v : (v[0] ?? fallback);
}

function GeneralSection() {
  const { t } = useTranslation();
  const [name, setName] = usePref('deviceName');
  const [startWith, setStartWith] = usePref('startWithSystem');
  const [tray, setTray] = usePref('minimizeToTray');
  return (
    <>
      <Field label={t('settings.deviceName')} description={t('settings.deviceNameHint')}>
        <Input value={name} maxLength={64} onChange={(e) => setName(e.currentTarget.value)} />
      </Field>
      <SwitchRow
        label={t('settings.startWithSystem')}
        checked={startWith}
        onCheckedChange={setStartWith}
      />
      <SwitchRow label={t('settings.minimizeToTray')} checked={tray} onCheckedChange={setTray} />
    </>
  );
}

interface TrustedRow {
  id: string;
  name: string;
  addedAt: number;
}

function SecuritySection() {
  const { t } = useTranslation();
  const locale = useLocale();
  const extras = useExtras();
  const [unattended, setUnattended] = usePref('unattended');
  const localTrusted = usePrefs((s) => s.trusted);
  const revokeLocal = usePrefs((s) => s.revokeTrusted);
  const [remote, setRemote] = useState<TrustedRow[] | null>(null);

  useEffect(() => {
    if (!extras) return undefined;
    let live = true;
    void extras.listTrusted().then((list) => {
      if (live) setRemote(list.map((p) => ({ id: p.device, name: p.label, addedAt: p.addedAt })));
    });
    return () => {
      live = false;
    };
  }, [extras]);

  const loading = extras === undefined || (extras !== null && remote === null);
  const rows: TrustedRow[] = extras ? (remote ?? []) : localTrusted;

  const revoke = (row: TrustedRow) => {
    if (extras) {
      void extras.removeTrusted(row.id).then(() => {
        setRemote((r) => (r ? r.filter((x) => x.id !== row.id) : r));
      });
    } else {
      revokeLocal(row.id);
    }
    toast(t('settings.revoked', { name: row.name }));
  };

  return (
    <>
      <SwitchRow
        label={t('settings.unattended')}
        description={t('settings.unattendedHint')}
        checked={unattended}
        onCheckedChange={setUnattended}
      />
      <div>
        <h3 className="mb-2 text-sm font-medium">{t('settings.trustedDevices')}</h3>
        {loading ? (
          <output aria-label={t('settings.trustedLoading')} className="flex flex-col gap-2">
            <Skeleton className="h-12 w-full" />
            <Skeleton className="h-12 w-full" />
          </output>
        ) : rows.length === 0 ? (
          <EmptyState
            icon={<ShieldCheck aria-hidden />}
            title={t('settings.trustedEmpty')}
            className="py-6"
          />
        ) : (
          <ul className="divide-y divide-outline rounded-lg border border-outline">
            {rows.map((d) => (
              <li key={d.id} className="flex items-center gap-3 px-3 py-2.5">
                <Monitor aria-hidden className="size-4 text-muted" />
                <div className="min-w-0 flex-1">
                  <p className="truncate text-sm font-medium">{d.name}</p>
                  <p className="text-xs text-muted">
                    {t('settings.trustedAdded', { date: formatDate(locale, d.addedAt) })}
                  </p>
                </div>
                <Button size="sm" variant="ghost" onClick={() => revoke(d)}>
                  {t('settings.revoke')}
                </Button>
              </li>
            ))}
          </ul>
        )}
      </div>
      <div className="flex flex-wrap items-center justify-between gap-3 rounded-lg bg-surface-2 p-3">
        <div className="min-w-0">
          <p className="text-sm font-medium">{t('settings.permanentPassword')}</p>
          <p className="text-xs text-muted">{t('settings.permanentPasswordHint')}</p>
          <p className="mt-0.5 text-xs text-muted">{t('settings.passwordUnavailable')}</p>
        </div>
        <Tooltip content={t('settings.passwordUnavailable')}>
          <Button
            variant="outline"
            size="sm"
            icon={<KeyRound aria-hidden />}
            disabled
            focusableWhenDisabled
          >
            {t('settings.setPassword')}
          </Button>
        </Tooltip>
      </div>
    </>
  );
}

type RelayMode = 'auto' | 'always' | 'direct';

const SERVER_RE = /^https?:\/\/[^\s/$.?#][^\s]*$/i;

function NetworkSection() {
  const { t } = useTranslation();
  const extras = useExtras();
  const [relay, setRelay] = usePref('relayUrl');
  const [, setDirect] = usePref('directConnections');
  const [mode, setMode] = useUiPref('relayMode');
  const [draft, setDraft] = useState(relay);
  const [diag, setDiag] = useState<string>('idle');

  useEffect(() => {
    if (!extras) return undefined;
    let live = true;
    void extras.getServer().then((s) => {
      if (live && s !== null) setDraft(s);
    });
    return () => {
      live = false;
    };
  }, [extras]);

  const invalid = draft !== '' && !SERVER_RE.test(draft);
  const save = () => {
    if (invalid || draft === relay) return;
    setRelay(draft);
    if (extras) {
      void extras
        .setServer(draft)
        .then(() =>
          toast.success(t('settings.serverSaved'), { description: t('settings.serverRestart') }),
        )
        .catch(() => toast.error(t('settings.serverInvalid')));
    }
  };
  const changeMode = (m: RelayMode) => {
    setMode(m);
    setDirect(m !== 'always');
  };
  const run = () => {
    setDiag('running');
    setTimeout(
      () =>
        setDiag(t('settings.diagnosticsResult', { udp: t('common.on'), relay: 38, nat: 'EIM' })),
      1200,
    );
  };
  return (
    <>
      <Field
        label={t('settings.serverUrl')}
        description={t('settings.serverUrlHint')}
        error={invalid ? t('settings.serverInvalid') : undefined}
      >
        <Input
          type="url"
          inputMode="url"
          autoComplete="url"
          value={draft}
          placeholder="https://relay.example.com"
          onChange={(e) => setDraft(e.currentTarget.value)}
          onBlur={save}
        />
      </Field>
      <Group label={t('settings.relayMode')}>
        <RadioGroup<RelayMode>
          label={t('settings.relayMode')}
          value={mode}
          onValueChange={changeMode}
          options={[
            {
              value: 'auto',
              label: t('settings.relayAuto'),
              description: t('settings.relayAutoHint'),
            },
            {
              value: 'always',
              label: t('settings.relayAlways'),
              description: t('settings.relayAlwaysHint'),
            },
            {
              value: 'direct',
              label: t('settings.relayDirect'),
              description: t('settings.relayDirectHint'),
            },
          ]}
        />
      </Group>
      <div className="flex flex-wrap items-center gap-3">
        <Button
          variant="secondary"
          icon={<Gauge aria-hidden />}
          loading={diag === 'running'}
          onClick={run}
        >
          {diag === 'running' ? t('settings.diagnosticsRunning') : t('settings.diagnostics')}
        </Button>
        <p aria-live="polite" className="font-mono text-xs text-muted">
          {diag !== 'idle' && diag !== 'running' ? diag : ''}
        </p>
      </div>
    </>
  );
}

function VideoSection() {
  const { t } = useTranslation();
  const host = useHost();
  const [codec, setCodec] = usePref('codec');
  const [fps, setFps] = usePref('maxFps');
  const [hw, setHw] = usePref('hardwareDecode');
  const [quality, setQuality] = useUiPref('defaultQuality');
  const [hwEncode, setHwEncode] = useUiPref('hardwareEncode');
  return (
    <>
      <div className="grid gap-3 @xl:grid-cols-2">
        <div className="flex flex-col gap-1.5">
          <label htmlFor="codec" className="text-sm font-medium">
            {t('settings.codec')}
          </label>
          <Select
            id="codec"
            value={codec}
            onValueChange={setCodec}
            options={[
              { value: 'auto', label: t('settings.codecAuto') },
              { value: 'av1', label: 'AV1' },
              { value: 'hevc', label: 'HEVC' },
              { value: 'h264', label: 'H.264' },
            ]}
          />
        </div>
        <div className="flex flex-col gap-1.5">
          <label htmlFor="quality" className="text-sm font-medium">
            {t('settings.defaultQuality')}
          </label>
          <Select<StreamQuality>
            id="quality"
            value={quality}
            onValueChange={setQuality}
            options={[
              { value: 'auto', label: t('session.qualityAuto') },
              { value: 'balanced', label: t('session.qualityBalanced') },
              { value: 'sharp', label: t('session.qualitySharp') },
              { value: 'speed', label: t('session.qualitySpeed') },
            ]}
          />
        </div>
        <div className="flex flex-col gap-1.5">
          <label htmlFor="fps" className="text-sm font-medium">
            {t('settings.maxFps')}
          </label>
          <Select
            id="fps"
            value={fps}
            onValueChange={setFps}
            options={(['30', '60', '120', '144', '240'] as const).map((v) => ({
              value: v,
              label: t('session.fps', { fps: v }),
            }))}
          />
        </div>
      </div>
      <SwitchRow label={t('settings.hardwareDecode')} checked={hw} onCheckedChange={setHw} />
      {host.platform.canHost ? (
        <SwitchRow
          label={t('settings.hardwareEncode')}
          description={t('settings.hardwareEncodeHint')}
          checked={hwEncode}
          onCheckedChange={setHwEncode}
        />
      ) : null}
    </>
  );
}

function AudioSection() {
  const { t } = useTranslation();
  const [audio, setAudio] = usePref('audio');
  const [mic, setMic] = usePref('microphone');
  const [volume, setVolume] = usePref('volume');
  return (
    <>
      <SwitchRow label={t('settings.audioEnabled')} checked={audio} onCheckedChange={setAudio} />
      <SwitchRow label={t('settings.microphone')} checked={mic} onCheckedChange={setMic} />
      <div>
        <p className="mb-2 flex justify-between text-sm font-medium">
          {t('settings.volume')}{' '}
          <span className="font-mono text-muted tabular-nums">{volume}%</span>
        </p>
        <Slider
          label={t('settings.volume')}
          value={volume}
          min={0}
          max={100}
          disabled={!audio}
          onValueChange={(v: number | readonly number[]) => setVolume(sliderValue(v, 0))}
        />
      </div>
    </>
  );
}

function InputSection() {
  const { t } = useTranslation();
  const [mode, setMode] = usePref('keyboardMode');
  const [relative, setRelative] = usePref('relativeMouse');
  const [scroll, setScroll] = usePref('scrollSpeed');
  return (
    <>
      <Group label={t('settings.keyboardMode')}>
        <RadioGroup<'scancode' | 'translate'>
          label={t('settings.keyboardMode')}
          value={mode}
          onValueChange={setMode}
          options={[
            { value: 'scancode', label: t('settings.keyboardScancode') },
            { value: 'translate', label: t('settings.keyboardTranslate') },
          ]}
        />
      </Group>
      <SwitchRow
        label={t('settings.relativeMouse')}
        checked={relative}
        onCheckedChange={setRelative}
      />
      <div>
        <p className="mb-2 flex justify-between text-sm font-medium">
          {t('settings.scrollSpeed')}{' '}
          <span className="font-mono text-muted tabular-nums">{scroll}</span>
        </p>
        <Slider
          label={t('settings.scrollSpeed')}
          value={scroll}
          min={1}
          max={10}
          onValueChange={(v: number | readonly number[]) => setScroll(sliderValue(v, 5))}
        />
      </div>
    </>
  );
}

function AppearanceSection() {
  const { t } = useTranslation();
  const { settings, update } = useTheme();
  const preset = ACCENT_PRESETS.find(
    (p) => p.hue === settings.accent.hue && p.chroma === settings.accent.chroma,
  );
  return (
    <div className="grid gap-6 @3xl:grid-cols-[minmax(0,1fr)_18rem]">
      <div className="flex flex-col gap-5">
        <Group label={t('settings.mode')}>
          <Segmented<Mode>
            label={t('settings.mode')}
            value={settings.mode}
            onValueChange={(mode) => update({ mode })}
            options={[
              { value: 'light', label: t('settings.modeLight'), icon: <Sun aria-hidden /> },
              { value: 'dark', label: t('settings.modeDark'), icon: <Moon aria-hidden /> },
              { value: 'system', label: t('settings.modeSystem'), icon: <Monitor aria-hidden /> },
            ]}
          />
        </Group>
        <Group label={t('settings.accent')}>
          <BaseRadioGroup
            aria-label={t('settings.accent')}
            value={preset?.id ?? ''}
            onValueChange={(v: unknown) => {
              const p = ACCENT_PRESETS.find((x) => x.id === v);
              if (p) update({ accent: { hue: p.hue, chroma: p.chroma } });
            }}
            className="flex flex-wrap gap-2"
          >
            {ACCENT_PRESETS.map((p) => (
              <Radio.Root
                key={p.id}
                value={p.id}
                aria-label={t('theme.accentPreset', { name: t(p.labelKey) })}
                title={t(p.labelKey)}
                className={cn(
                  'group grid size-9 cursor-pointer place-items-center rounded-full ring-offset-2 ring-offset-surface',
                  'transition-transform duration-(--scrin-dur-fast) hover:scale-110 data-checked:ring-2 data-checked:ring-fg',
                  'focus-visible:outline-2 focus-visible:outline-offset-4 focus-visible:outline-accent',
                )}
                style={{ background: `oklch(0.62 ${p.chroma} ${p.hue})` }}
              >
                <Radio.Indicator className="data-unchecked:hidden">
                  <Check aria-hidden className="size-4 text-white drop-shadow" />
                </Radio.Indicator>
              </Radio.Root>
            ))}
          </BaseRadioGroup>
          <div className="mt-3">
            <p className="mb-2 flex justify-between text-xs font-medium text-muted">
              {t('settings.customHue')}
              <span className="font-mono tabular-nums">{Math.round(settings.accent.hue)}°</span>
            </p>
            <div
              aria-hidden
              className="mb-1 h-2 rounded-full"
              style={{
                background: `linear-gradient(90deg, ${[0, 60, 120, 180, 240, 300, 360]
                  .map((h) => `oklch(0.62 ${settings.accent.chroma} ${h})`)
                  .join(', ')})`,
              }}
            />
            <Slider
              label={t('settings.customHue')}
              value={settings.accent.hue}
              min={0}
              max={359}
              onValueChange={(v: number | readonly number[]) =>
                update({
                  accent: {
                    hue: sliderValue(v, settings.accent.hue),
                    chroma: Math.max(settings.accent.chroma, 0.08),
                  },
                })
              }
            />
          </div>
        </Group>
        <Group label={t('settings.surface')}>
          <Segmented<Surface>
            label={t('settings.surface')}
            value={settings.surface}
            onValueChange={(surface) => update({ surface })}
            options={[
              { value: 'solid', label: t('settings.surfaceSolid') },
              { value: 'glass', label: t('settings.surfaceGlass') },
              { value: 'amoled', label: t('settings.surfaceAmoled') },
            ]}
          />
        </Group>
        <Group label={t('settings.density')}>
          <Segmented<Density>
            label={t('settings.density')}
            value={settings.density}
            onValueChange={(density) => update({ density })}
            options={[
              { value: 'compact', label: t('settings.densityCompact') },
              { value: 'comfortable', label: t('settings.densityComfortable') },
              { value: 'spacious', label: t('settings.densitySpacious') },
            ]}
          />
        </Group>
        <Group label={t('settings.motion')} hint={t('settings.motionHint')}>
          <Segmented<MotionLevel>
            label={t('settings.motion')}
            value={settings.motion}
            onValueChange={(motion) => update({ motion })}
            options={[
              { value: 'off', label: t('settings.motionOff') },
              { value: 'subtle', label: t('settings.motionSubtle') },
              { value: 'full', label: t('settings.motionFull') },
            ]}
          />
        </Group>
      </div>
      <section
        aria-label={t('settings.preview')}
        className="self-start rounded-xl bg-bg p-4 ring-1 ring-outline"
      >
        <p className="mb-3 text-xs font-medium tracking-wide text-muted uppercase">
          {t('settings.preview')}
        </p>
        <div className="rounded-lg glass-panel p-4">
          <p className="font-semibold text-fg">{t('settings.previewSample')}</p>
          <p className="mt-1 text-sm text-muted">{t('settings.previewBody')}</p>
          <div className="mt-4 flex flex-wrap gap-2">
            <Button size="sm">{t('common.save')}</Button>
            <Button size="sm" variant="outline">
              {t('common.cancel')}
            </Button>
          </div>
          <div className="mt-3 flex flex-wrap gap-1.5">
            <Badge tone="accent">{t('common.online')}</Badge>
            <Badge tone="success">AV1</Badge>
            <Badge tone="warning">{t('session.recording')}</Badge>
          </div>
        </div>
      </section>
    </div>
  );
}

function LanguageSection() {
  const { t } = useTranslation();
  const locale = useLocale();
  return (
    <>
      <p className="text-sm text-muted">{t('settings.languageHint')}</p>
      <RadioGroup<Locale>
        label={t('settings.language')}
        value={locale}
        onValueChange={(l) => {
          void setLocale(l).then(() => toast.success(t('settings.saved')));
        }}
        options={LOCALES.map((l) => ({ value: l, label: <span lang={l}>{LOCALE_NAMES[l]}</span> }))}
      />
    </>
  );
}

function UpdatesSection() {
  const { t } = useTranslation();
  const host = useHost();
  const extras = useExtras();
  const [channel, setChannel] = useUiPref('updateChannel');
  const [state, setState] = useState<'idle' | 'checking' | UpdateCheck>('idle');
  const desktop = host.platform.kind === 'desktop';

  const check = () => {
    if (!extras) return;
    setState('checking');
    void extras.checkUpdate().then(setState);
  };

  let result = '';
  if (typeof state === 'object') {
    if (state.status === 'up-to-date') result = t('settings.upToDate');
    else if (state.status === 'available')
      result = t('settings.updateAvailable', { version: state.version });
    else result = t('settings.updateFailed');
  }

  return (
    <>
      <Group label={t('settings.updateChannel')} hint={t('settings.updateChannelHint')}>
        <Segmented<'stable' | 'beta'>
          label={t('settings.updateChannel')}
          value={channel}
          onValueChange={setChannel}
          options={[
            { value: 'stable', label: t('settings.channelStable') },
            { value: 'beta', label: t('settings.channelBeta') },
          ]}
        />
      </Group>
      {desktop ? (
        <div className="flex flex-wrap items-center gap-3">
          <Button
            variant="secondary"
            icon={<RefreshCw aria-hidden />}
            loading={state === 'checking'}
            disabled={extras === null}
            onClick={check}
          >
            {state === 'checking' ? t('settings.checking') : t('settings.checkUpdates')}
          </Button>
          <p aria-live="polite" className="text-sm text-muted">
            {extras === null ? t('settings.updatesUnavailable') : result}
          </p>
        </div>
      ) : (
        <p className="text-sm text-muted">{t('settings.updatesWeb')}</p>
      )}
    </>
  );
}

function AboutSection() {
  const { t } = useTranslation();
  const host = useHost();
  return (
    <>
      <div>
        <h3 className="text-base font-semibold text-fg">scrin</h3>
        <p className="text-sm text-muted">
          {t('settings.version', { version: host.platform.version })}
        </p>
      </div>
      <p className="text-sm">{t('settings.licence')}</p>
      <p className="text-sm text-muted">{t('settings.licenceHint')}</p>
      <nav aria-label={t('settings.links')}>
        <ul className="flex flex-wrap gap-x-5 gap-y-2">
          {LINKS.map((l) => (
            <li key={l.labelKey}>
              <a
                href={l.href}
                target="_blank"
                rel="noopener noreferrer"
                className="inline-flex min-h-6 items-center gap-1.5 text-sm font-medium text-accent underline-offset-4 hover:underline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-accent"
              >
                {t(l.labelKey)} <ExternalLink aria-hidden className="size-3.5" />
              </a>
            </li>
          ))}
        </ul>
      </nav>
    </>
  );
}
