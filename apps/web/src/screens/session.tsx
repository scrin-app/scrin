import {
  formatBitrate,
  formatPercent,
  formatScrinId,
  useLocale,
  useTranslation,
} from '@scrin/i18n';
import {
  AlertDialog,
  Badge,
  Button,
  cn,
  IconButton,
  Input,
  Menu,
  MenuGroup,
  MenuItem,
  MenuRadioGroup,
  MenuRadioItem,
  MenuSeparator,
  Popover,
  Spinner,
  type SpecialKey,
  Tooltip,
  toast,
  useHost,
  useMotionDuration,
} from '@scrin/ui';
import { useNavigate } from '@tanstack/react-router';
import { AnimatePresence, m } from 'motion/react';
import {
  Activity,
  ChevronLeft,
  ChevronRight,
  Clipboard,
  Circle,
  FolderUp,
  Gauge,
  Keyboard,
  Maximize,
  MessageSquare,
  Minimize,
  Monitor,
  PhoneOff,
  Send,
} from 'lucide-react';
import { useEffect, useRef, useState, type ReactNode, type SubmitEvent } from 'react';

import { useSessionEvents } from '../lib/engine';

type Quality = 'auto' | 'balanced' | 'sharp' | 'speed';

export function SessionPage({ id, sessionId }: { id: string; sessionId: string | undefined }) {
  const { t } = useTranslation();
  const host = useHost();
  const navigate = useNavigate();
  const session = useSessionEvents(sessionId);
  const rootRef = useRef<HTMLDivElement>(null);
  const [fullscreen, setFullscreen] = useState(false);
  const [showStats, setShowStats] = useState(true);
  const [recording, setRecording] = useState(false);
  const [expanded, setExpanded] = useState(true);
  const [quality, setQuality] = useState<Quality>('auto');
  const [display, setDisplay] = useState('1');
  const [confirmEnd, setConfirmEnd] = useState(false);

  useEffect(() => {
    const onChange = () => setFullscreen(document.fullscreenElement !== null);
    document.addEventListener('fullscreenchange', onChange);
    return () => document.removeEventListener('fullscreenchange', onChange);
  }, []);

  useEffect(() => {
    if (session.ended) void navigate({ to: '/' });
  }, [session.ended, navigate]);

  const toggleFullscreen = async () => {
    if (document.fullscreenElement) await document.exitFullscreen();
    else await rootRef.current?.requestFullscreen();
  };

  const sendKeys = async (combo: SpecialKey, label: string) => {
    if (sessionId) await host.engine.sendKeys(sessionId, combo);
    toast(t('session.keySent', { key: label }));
  };

  const end = async () => {
    if (sessionId) await host.engine.endSession(sessionId);
    else void navigate({ to: '/' });
  };

  const keys: { combo: SpecialKey; label: string }[] = [
    { combo: 'ctrl-alt-del', label: t('session.ctrlAltDel') },
    { combo: 'win', label: t('session.winKey') },
    { combo: 'alt-tab', label: t('session.altTab') },
    { combo: 'print-screen', label: t('session.printScreen') },
    { combo: 'lock', label: t('session.lockScreen') },
  ];

  return (
    <div ref={rootRef} className="relative h-dvh w-full overflow-hidden bg-black text-fg">
      <h1 className="sr-only">{t('session.title', { id: formatScrinId(id) })}</h1>
      <main id="main" className="absolute inset-0 grid place-items-center">
        <RemoteCanvas id={id} hasVideo={session.stats !== null} />
      </main>

      {showStats && session.stats ? <StatsOverlay stats={session.stats} /> : null}

      <div className="pointer-events-none absolute inset-x-0 top-0 flex justify-center px-3 pt-[max(0.75rem,env(safe-area-inset-top))]">
        <Toolbar expanded={expanded} onToggle={() => setExpanded((e) => !e)}>
          <Menu
            trigger={
              <ToolButton label={t('session.quality')}>
                <Gauge aria-hidden />
              </ToolButton>
            }
          >
            <MenuGroup label={t('session.quality')}>
              <MenuRadioGroup<Quality> value={quality} onValueChange={setQuality}>
                <MenuRadioItem value="auto">{t('session.qualityAuto')}</MenuRadioItem>
                <MenuRadioItem value="balanced">{t('session.qualityBalanced')}</MenuRadioItem>
                <MenuRadioItem value="sharp">{t('session.qualitySharp')}</MenuRadioItem>
                <MenuRadioItem value="speed">{t('session.qualitySpeed')}</MenuRadioItem>
              </MenuRadioGroup>
            </MenuGroup>
          </Menu>
          <Menu
            trigger={
              <ToolButton label={t('session.display')}>
                <Monitor aria-hidden />
              </ToolButton>
            }
          >
            <MenuGroup label={t('session.display')}>
              <MenuRadioGroup<string> value={display} onValueChange={setDisplay}>
                {session.displays.map((dsp) => (
                  <MenuRadioItem key={dsp.id} value={String(dsp.id)}>
                    {t('session.displayN', { n: dsp.id })} · {dsp.width}×{dsp.height}
                  </MenuRadioItem>
                ))}
                <MenuRadioItem value="all">{t('session.allDisplays')}</MenuRadioItem>
              </MenuRadioGroup>
            </MenuGroup>
          </Menu>
          <Menu
            trigger={
              <ToolButton label={t('session.keys')}>
                <Keyboard aria-hidden />
              </ToolButton>
            }
          >
            {keys.map((k, i) => (
              <div key={k.combo}>
                {i === 1 ? <MenuSeparator /> : null}
                <MenuItem onClick={() => void sendKeys(k.combo, k.label)}>{k.label}</MenuItem>
              </div>
            ))}
          </Menu>
          <ToolButton
            label={t('session.clipboardSync')}
            onClick={() => toast(t('session.clipboard'), { description: t('common.comingSoon') })}
          >
            <Clipboard aria-hidden />
          </ToolButton>
          <ToolButton
            label={t('session.files')}
            onClick={() => toast(t('session.files'), { description: t('common.comingSoon') })}
          >
            <FolderUp aria-hidden />
          </ToolButton>
          <ChatPopover sessionId={sessionId} messages={session.chat} />
          <ToolButton
            label={recording ? t('session.recording') : t('session.record')}
            pressed={recording}
            onClick={() => setRecording((r) => !r)}
          >
            <Circle
              aria-hidden
              className={cn(recording && 'animate-pulse-soft fill-danger text-danger')}
            />
          </ToolButton>
          <ToolButton
            label={t('session.stats')}
            pressed={showStats}
            onClick={() => setShowStats((s) => !s)}
          >
            <Activity aria-hidden />
          </ToolButton>
          <ToolButton
            label={fullscreen ? t('session.exitFullscreen') : t('session.fullscreen')}
            onClick={() => void toggleFullscreen()}
          >
            {fullscreen ? <Minimize aria-hidden /> : <Maximize aria-hidden />}
          </ToolButton>
          <span aria-hidden className="mx-1 h-6 w-px bg-outline" />
          <Tooltip content={t('session.end')}>
            <IconButton
              label={t('session.end')}
              variant="danger"
              size="icon-sm"
              onClick={() => setConfirmEnd(true)}
            >
              <PhoneOff aria-hidden />
            </IconButton>
          </Tooltip>
        </Toolbar>
      </div>

      <AlertDialog
        open={confirmEnd}
        onOpenChange={setConfirmEnd}
        title={t('session.endConfirmTitle')}
        description={t('session.endConfirmBody')}
        confirmLabel={t('session.end')}
        destructive
        onConfirm={() => void end()}
      />
      {recording ? (
        <Badge tone="danger" className="absolute bottom-4 left-4">
          <Circle aria-hidden className="fill-danger" /> {t('session.recording')}
        </Badge>
      ) : null}
    </div>
  );
}

/**
 * The floating toolbar morphs between a pill with every tool and a compact
 * handle. Only `layout` (transform) and opacity animate.
 */
function Toolbar({
  expanded,
  onToggle,
  children,
}: {
  expanded: boolean;
  onToggle: () => void;
  children: ReactNode;
}) {
  const { t } = useTranslation();
  const d = useMotionDuration(0.32);
  return (
    <m.div
      layout
      transition={{ type: 'spring', bounce: 0.18, visualDuration: d }}
      role="toolbar"
      aria-label={t('session.toolbar')}
      className="pointer-events-auto flex max-w-full items-center gap-0.5 overflow-x-auto rounded-2xl glass-panel p-1.5 shadow-2xl"
      style={{ borderRadius: 16 }}
    >
      <AnimatePresence initial={false} mode="popLayout">
        {expanded ? (
          <m.div
            key="tools"
            className="flex items-center gap-0.5"
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            transition={{ duration: d * 0.6 }}
          >
            {children}
          </m.div>
        ) : null}
      </AnimatePresence>
      <m.span layout="position">
        <IconButton
          size="icon-sm"
          label={expanded ? t('session.collapse') : t('session.moreTools')}
          aria-expanded={expanded}
          onClick={onToggle}
        >
          {expanded ? <ChevronLeft aria-hidden /> : <ChevronRight aria-hidden />}
        </IconButton>
      </m.span>
    </m.div>
  );
}

function ToolButton({
  label,
  pressed,
  children,
  ...rest
}: {
  label: string;
  pressed?: boolean;
  children: ReactNode;
  onClick?: () => void;
}) {
  return (
    <IconButton
      size="icon-sm"
      label={label}
      {...(pressed !== undefined ? { 'aria-pressed': pressed } : {})}
      className={cn(pressed && 'bg-accent/15 text-accent')}
      {...rest}
    >
      {children}
    </IconButton>
  );
}

function RemoteCanvas({ id, hasVideo }: { id: string; hasVideo: boolean }) {
  const { t } = useTranslation();
  return (
    <figure className="relative m-0 aspect-[43/18] max-h-full w-full max-w-full overflow-hidden">
      <figcaption className="sr-only">
        {t('session.canvasLabel', { id: formatScrinId(id) })}
      </figcaption>
      {/* Placeholder "desktop" until WebCodecs frames are wired in. */}
      <div className="absolute inset-0 bg-[radial-gradient(120%_120%_at_20%_10%,oklch(0.45_0.16_264),oklch(0.2_0.08_280)_55%,oklch(0.12_0.03_260))]" />
      <div className="absolute inset-x-[6%] top-[10%] bottom-[16%] grid grid-cols-3 gap-[2%] opacity-90">
        <div className="rounded-lg bg-white/10 backdrop-blur" />
        <div className="col-span-2 rounded-lg bg-white/15 backdrop-blur" />
      </div>
      <div className="absolute inset-x-0 bottom-0 flex h-[7%] items-center justify-center gap-[1%] bg-black/40">
        {Array.from({ length: 7 }, (_, i) => (
          <span key={i} className="aspect-square h-[60%] rounded bg-white/25" />
        ))}
      </div>
      {hasVideo ? null : (
        <div className="absolute inset-0 grid place-items-center bg-black/55 text-white">
          <span className="flex items-center gap-3 text-sm">
            <Spinner label={false} /> {t('session.waitingVideo')}
          </span>
        </div>
      )}
    </figure>
  );
}

function StatsOverlay({
  stats,
}: {
  stats: NonNullable<ReturnType<typeof useSessionEvents>['stats']>;
}) {
  const { t } = useTranslation();
  const locale = useLocale();
  const rows: [string, string][] = [
    [t('session.statLatency'), `${stats.latencyMs} ms`],
    [t('session.statBitrate'), formatBitrate(locale, stats.bitrateBps)],
    [t('session.fps', { fps: stats.fps }), ''],
    [t('session.statLoss'), formatPercent(locale, stats.lossRatio)],
    [t('session.statCodec'), stats.codec],
    [t('session.statResolution'), `${stats.width}×${stats.height}`],
    [t('session.statRoute'), stats.route],
  ];
  return (
    <aside
      aria-label={t('session.stats')}
      className="absolute right-3 bottom-3 w-56 rounded-xl glass-panel p-3 font-mono text-xs shadow-xl"
    >
      <dl className="grid grid-cols-[1fr_auto] gap-x-3 gap-y-1">
        {rows.map(([k, v]) => (
          <div key={k} className="contents">
            <dt className="text-muted">{k}</dt>
            <dd className="text-right text-fg tabular-nums">{v}</dd>
          </div>
        ))}
      </dl>
    </aside>
  );
}

function ChatPopover({
  sessionId,
  messages,
}: {
  sessionId: string | undefined;
  messages: readonly { from: 'remote' | 'local'; text: string; at: number }[];
}) {
  const { t } = useTranslation();
  const host = useHost();
  const [text, setText] = useState('');
  const d = useMotionDuration(0.2);
  const send = (e: SubmitEvent<HTMLFormElement>) => {
    e.preventDefault();
    const v = text.trim();
    if (!v || !sessionId) return;
    setText('');
    void host.engine.sendChat(sessionId, v);
  };
  return (
    <Popover
      title={t('session.chat')}
      className="w-80"
      trigger={
        <IconButton size="icon-sm" label={t('session.chat')}>
          <MessageSquare aria-hidden />
        </IconButton>
      }
    >
      <ul aria-live="polite" className="mb-3 flex max-h-64 min-h-24 flex-col gap-2 overflow-y-auto">
        {messages.length === 0 ? (
          <li className="m-auto text-xs text-muted">{t('session.chatEmpty')}</li>
        ) : (
          messages.map((msg) => (
            <m.li
              key={`${msg.at}-${msg.from}`}
              initial={{ opacity: 0, y: 4 }}
              animate={{ opacity: 1, y: 0 }}
              transition={{ duration: d }}
              className={cn(
                'max-w-[85%] rounded-2xl px-3 py-1.5 text-sm',
                msg.from === 'local'
                  ? 'self-end bg-accent text-accent-fg'
                  : 'self-start bg-surface-2 text-fg',
              )}
            >
              {msg.text}
            </m.li>
          ))
        )}
      </ul>
      <form onSubmit={send} className="flex gap-2">
        <Input
          value={text}
          onChange={(e) => setText(e.currentTarget.value)}
          placeholder={t('session.chatPlaceholder')}
          aria-label={t('session.chatPlaceholder')}
        />
        <Button type="submit" size="icon" aria-label={t('session.chatSend')}>
          <Send aria-hidden />
        </Button>
      </form>
    </Popover>
  );
}
