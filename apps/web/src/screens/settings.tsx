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
  type TranslationKey,
} from '@scrin/i18n';
import {
  ACCENT_PRESETS,
  Badge,
  Button,
  Card,
  CardHeader,
  cn,
  EmptyState,
  Field,
  Input,
  RadioGroup,
  Segmented,
  Select,
  Slider,
  SwitchRow,
  toast,
  useHost,
  useTheme,
  type Density,
  type Mode,
  type MotionLevel,
  type Surface,
} from '@scrin/ui';
import {
  Check,
  Cog,
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
  ShieldCheck,
  Sun,
  Volume2,
  type LucideIcon,
} from 'lucide-react';
import { useState, type ReactNode } from 'react';

import { PageHeader } from '../components/page-header';
import { usePrefs, type Prefs } from '../lib/prefs';

const SECTIONS: { id: string; label: TranslationKey; icon: LucideIcon }[] = [
  { id: 'general', label: 'settings.general', icon: Cog },
  { id: 'appearance', label: 'settings.appearance', icon: Palette },
  { id: 'language', label: 'settings.language', icon: Languages },
  { id: 'security', label: 'settings.security', icon: ShieldCheck },
  { id: 'network', label: 'settings.network', icon: Network },
  { id: 'video', label: 'settings.video', icon: Monitor },
  { id: 'audio', label: 'settings.audio', icon: Volume2 },
  { id: 'input', label: 'settings.input', icon: Keyboard },
  { id: 'about', label: 'settings.about', icon: Info },
];

const SOURCE_URL = 'https://github.com/scrin-app/scrin';

export function SettingsPage() {
  const { t } = useTranslation();
  return (
    <>
      <PageHeader title={t('settings.title')} />
      <div className="grid gap-6 @4xl:grid-cols-[13rem_minmax(0,1fr)] @7xl:grid-cols-[15rem_minmax(0,56rem)]">
        <nav
          aria-label={t('settings.sections')}
          className="@4xl:sticky @4xl:top-24 @4xl:self-start"
        >
          <ul className="-mx-1 flex gap-1 overflow-x-auto px-1 pb-1 @4xl:flex-col @4xl:overflow-visible">
            {SECTIONS.map(({ id, label, icon: Icon }) => (
              <li key={id} className="shrink-0">
                <a
                  href={`#${id}`}
                  className="flex h-10 items-center gap-2.5 rounded-lg px-3 text-sm font-medium whitespace-nowrap text-muted transition-colors duration-(--scrin-dur-fast) hover:bg-surface-2 hover:text-fg"
                >
                  <Icon aria-hidden className="size-4" />
                  {t(label)}
                </a>
              </li>
            ))}
          </ul>
        </nav>
        <div className="flex min-w-0 flex-col gap-4">
          <GeneralSection />
          <AppearanceSection />
          <LanguageSection />
          <SecuritySection />
          <NetworkSection />
          <VideoSection />
          <AudioSection />
          <InputSection />
          <AboutSection />
        </div>
      </div>
    </>
  );
}

function Section({
  id,
  title,
  icon,
  children,
}: {
  id: string;
  title: string;
  icon: ReactNode;
  children: ReactNode;
}) {
  return (
    <Card id={id} aria-labelledby={`${id}-h`} className="scroll-mt-24">
      <header className="mb-4 flex items-center gap-3">
        <span className="grid size-9 place-items-center rounded-lg bg-accent/12 text-accent [&_svg]:size-4.5">
          {icon}
        </span>
        <h2 id={`${id}-h`} className="text-base font-semibold">
          {title}
        </h2>
      </header>
      <div className="flex flex-col gap-3">{children}</div>
    </Card>
  );
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

function GeneralSection() {
  const { t } = useTranslation();
  const [name, setName] = usePref('deviceName');
  const [startWith, setStartWith] = usePref('startWithSystem');
  const [tray, setTray] = usePref('minimizeToTray');
  return (
    <Section id="general" title={t('settings.general')} icon={<Cog aria-hidden />}>
      <Field label={t('settings.deviceName')} description={t('settings.deviceNameHint')}>
        <Input value={name} maxLength={64} onChange={(e) => setName(e.currentTarget.value)} />
      </Field>
      <SwitchRow
        label={t('settings.startWithSystem')}
        checked={startWith}
        onCheckedChange={setStartWith}
      />
      <SwitchRow label={t('settings.minimizeToTray')} checked={tray} onCheckedChange={setTray} />
    </Section>
  );
}

function AppearanceSection() {
  const { t } = useTranslation();
  const { settings, update } = useTheme();
  return (
    <Section id="appearance" title={t('settings.appearance')} icon={<Palette aria-hidden />}>
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
              value={
                ACCENT_PRESETS.find(
                  (p) => p.hue === settings.accent.hue && p.chroma === settings.accent.chroma,
                )?.id ?? ''
              }
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
                  )}
                  style={{ background: `oklch(0.62 ${p.chroma} ${p.hue})` }}
                >
                  <Radio.Indicator className="data-unchecked:hidden">
                    <Check aria-hidden className="size-4 text-white drop-shadow" />
                  </Radio.Indicator>
                </Radio.Root>
              ))}
            </BaseRadioGroup>
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
    </Section>
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

function LanguageSection() {
  const { t } = useTranslation();
  const locale = useLocale();
  return (
    <Section id="language" title={t('settings.language')} icon={<Languages aria-hidden />}>
      <p className="text-sm text-muted">{t('settings.languageHint')}</p>
      <RadioGroup<Locale>
        label={t('settings.language')}
        value={locale}
        onValueChange={(l) => {
          void setLocale(l).then(() => toast.success(t('settings.saved')));
        }}
        options={LOCALES.map((l) => ({ value: l, label: <span lang={l}>{LOCALE_NAMES[l]}</span> }))}
      />
    </Section>
  );
}

function SecuritySection() {
  const { t } = useTranslation();
  const locale = useLocale();
  const [unattended, setUnattended] = usePref('unattended');
  const trusted = usePrefs((s) => s.trusted);
  const revoke = usePrefs((s) => s.revokeTrusted);
  return (
    <Section id="security" title={t('settings.security')} icon={<ShieldCheck aria-hidden />}>
      <SwitchRow
        label={t('settings.unattended')}
        description={t('settings.unattendedHint')}
        checked={unattended}
        onCheckedChange={setUnattended}
      />
      <div>
        <h3 className="mb-2 text-sm font-medium">{t('settings.trustedDevices')}</h3>
        {trusted.length === 0 ? (
          <EmptyState
            icon={<ShieldCheck aria-hidden />}
            title={t('settings.trustedEmpty')}
            className="py-6"
          />
        ) : (
          <ul className="divide-y divide-outline rounded-lg border border-outline">
            {trusted.map((d) => (
              <li key={d.id} className="flex items-center gap-3 px-3 py-2.5">
                <Monitor aria-hidden className="size-4 text-muted" />
                <div className="min-w-0 flex-1">
                  <p className="truncate text-sm font-medium">{d.name}</p>
                  <p className="text-xs text-muted">
                    {t('settings.trustedAdded', { date: formatDate(locale, d.addedAt) })}
                  </p>
                </div>
                <Button
                  size="sm"
                  variant="ghost"
                  onClick={() => {
                    revoke(d.id);
                    toast(t('settings.revoked', { name: d.name }));
                  }}
                >
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
        </div>
        <Button variant="outline" size="sm" icon={<KeyRound aria-hidden />} disabled>
          {t('settings.setPassword')}
        </Button>
      </div>
    </Section>
  );
}

function NetworkSection() {
  const { t } = useTranslation();
  const [relay, setRelay] = usePref('relayUrl');
  const [direct, setDirect] = usePref('directConnections');
  const [draft, setDraft] = useState(relay);
  const [diag, setDiag] = useState<string>('idle');
  const invalid = draft !== '' && !/^https:\/\/[^\s/$.?#].[^\s]*$/i.test(draft);
  const run = () => {
    setDiag('running');
    setTimeout(
      () =>
        setDiag(t('settings.diagnosticsResult', { udp: t('common.on'), relay: 38, nat: 'EIM' })),
      1200,
    );
  };
  return (
    <Section id="network" title={t('settings.network')} icon={<Network aria-hidden />}>
      <Field
        label={t('settings.relayUrl')}
        description={t('settings.relayUrlHint')}
        error={invalid ? t('settings.relayUrlInvalid') : undefined}
      >
        <Input
          type="url"
          inputMode="url"
          value={draft}
          placeholder="https://relay.example.com"
          onChange={(e) => setDraft(e.currentTarget.value)}
          onBlur={() => {
            if (!invalid) setRelay(draft);
          }}
        />
      </Field>
      <SwitchRow
        label={t('settings.directConnections')}
        description={t('settings.directConnectionsHint')}
        checked={direct}
        onCheckedChange={setDirect}
      />
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
    </Section>
  );
}

function VideoSection() {
  const { t } = useTranslation();
  const [codec, setCodec] = usePref('codec');
  const [fps, setFps] = usePref('maxFps');
  const [hw, setHw] = usePref('hardwareDecode');
  return (
    <Section id="video" title={t('settings.video')} icon={<Monitor aria-hidden />}>
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
          <label htmlFor="fps" className="text-sm font-medium">
            {t('settings.maxFps')}
          </label>
          <Select
            id="fps"
            value={fps}
            onValueChange={setFps}
            options={(['30', '60', '120', '144', '240'] as const).map((v) => ({
              value: v,
              label: `${v} fps`,
            }))}
          />
        </div>
      </div>
      <SwitchRow label={t('settings.hardwareDecode')} checked={hw} onCheckedChange={setHw} />
    </Section>
  );
}

function AudioSection() {
  const { t } = useTranslation();
  const [audio, setAudio] = usePref('audio');
  const [mic, setMic] = usePref('microphone');
  const [volume, setVolume] = usePref('volume');
  return (
    <Section id="audio" title={t('settings.audio')} icon={<Volume2 aria-hidden />}>
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
          onValueChange={(v: number | readonly number[]) =>
            setVolume(typeof v === 'number' ? v : (v[0] ?? 0))
          }
        />
      </div>
    </Section>
  );
}

function InputSection() {
  const { t } = useTranslation();
  const [mode, setMode] = usePref('keyboardMode');
  const [relative, setRelative] = usePref('relativeMouse');
  const [scroll, setScroll] = usePref('scrollSpeed');
  return (
    <Section id="input" title={t('settings.input')} icon={<Keyboard aria-hidden />}>
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
          onValueChange={(v: number | readonly number[]) =>
            setScroll(typeof v === 'number' ? v : (v[0] ?? 5))
          }
        />
      </div>
    </Section>
  );
}

function AboutSection() {
  const { t } = useTranslation();
  const host = useHost();
  return (
    <Section id="about" title={t('settings.about')} icon={<Info aria-hidden />}>
      <CardHeader
        headingLevel={3}
        title="scrin"
        description={t('settings.version', { version: host.platform.version })}
      />
      <p className="text-sm">{t('settings.licence')}</p>
      <p className="text-sm text-muted">{t('settings.licenceHint')}</p>
      <div>
        <a
          href={SOURCE_URL}
          target="_blank"
          rel="noopener noreferrer"
          className="inline-flex items-center gap-1.5 text-sm font-medium text-accent underline-offset-4 hover:underline"
        >
          {t('settings.source')} <ExternalLink aria-hidden className="size-3.5" />
        </a>
      </div>
    </Section>
  );
}
