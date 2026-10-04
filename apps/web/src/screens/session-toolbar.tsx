import { formatBitrate, formatPercent, useLocale, useTranslation } from '@scrin/i18n';
import {
  Button,
  cn,
  Input,
  loadHostExtras,
  Menu,
  MenuGroup,
  MenuItem,
  MenuRadioGroup,
  MenuRadioItem,
  MenuSeparator,
  MorphToolbar,
  Popover,
  type SessionStats,
  type SpecialKey,
  type StreamQuality,
  ToolbarButton,
  ToolbarSeparator,
  TOOLBAR_EDGES,
  type ToolbarEdge,
  toast,
  useHost,
  useMotionDuration,
} from '@scrin/ui';
import { m } from 'motion/react';
import {
  Activity,
  ChevronLeft,
  ChevronRight,
  Circle,
  Clipboard,
  EyeOff,
  FolderUp,
  Gauge,
  GripVertical,
  Keyboard,
  Maximize,
  MessageSquare,
  Minimize,
  Monitor,
  PanelTop,
  PhoneOff,
  Send,
} from 'lucide-react';
import { useState, type SubmitEvent } from 'react';

import type { SessionState } from '../lib/engine';
import { useUiPref } from './ui-prefs';

const EDGE_LABEL = {
  top: 'session.positionTop',
  bottom: 'session.positionBottom',
  left: 'session.positionLeft',
  right: 'session.positionRight',
} as const satisfies Record<ToolbarEdge, `session.${string}`>;

export interface SessionToolbarProps {
  sessionId: string | undefined;
  session: SessionState;
  fullscreen: boolean;
  onToggleFullscreen: () => void;
  recording: boolean;
  onToggleRecording: () => void;
  onEnd: () => void;
}

/** D-003: every in-session tool, in the morphing toolbar. */
export function SessionToolbar({
  sessionId,
  session,
  fullscreen,
  onToggleFullscreen,
  recording,
  onToggleRecording,
  onEnd,
}: SessionToolbarProps) {
  const { t } = useTranslation();
  const host = useHost();
  const [defaultQuality] = useUiPref('defaultQuality');
  const [quality, setQualityState] = useState<StreamQuality>(defaultQuality);
  const [display, setDisplay] = useState('1');
  const [expanded, setExpanded] = useUiPref('toolbarExpanded');
  const [edge, setEdge] = useUiPref('toolbarEdge');
  const [autoHide, setAutoHide] = useUiPref('toolbarAutoHide');
  const [showStats, setShowStats] = useUiPref('showStats');
  const vertical = edge === 'left' || edge === 'right';
  const menuSide =
    edge === 'top' ? 'bottom' : edge === 'bottom' ? 'top' : edge === 'left' ? 'right' : 'left';

  const setQuality = (q: StreamQuality) => {
    setQualityState(q);
    if (!sessionId) return;
    void loadHostExtras(host).then((x) => x?.setQuality(sessionId, q));
  };

  const sendKeys = async (combo: SpecialKey, label: string) => {
    if (sessionId) await host.engine.sendKeys(sessionId, combo);
    toast(t('session.keySent', { key: label }));
  };

  const keys: { combo: SpecialKey; label: string }[] = [
    { combo: 'ctrl-alt-del', label: t('session.ctrlAltDel') },
    { combo: 'win', label: t('session.winKey') },
    { combo: 'alt-tab', label: t('session.altTab') },
    { combo: 'print-screen', label: t('session.printScreen') },
    { combo: 'lock', label: t('session.lockScreen') },
  ];

  return (
    <>
      {showStats && session.stats ? <StatsOverlay stats={session.stats} edge={edge} /> : null}
      <MorphToolbar
        label={t('session.toolbar')}
        expanded={expanded}
        onExpandedChange={setExpanded}
        expandLabel={t('session.moreTools')}
        collapseLabel={t('session.collapse')}
        edge={edge}
        onEdgeChange={setEdge}
        moveLabel={t('session.moveToolbar')}
        autoHide={autoHide}
        expandIcon={
          vertical ? (
            <ChevronRight aria-hidden className="rotate-90" />
          ) : (
            <ChevronRight aria-hidden />
          )
        }
        collapseIcon={
          vertical ? <ChevronLeft aria-hidden className="rotate-90" /> : <ChevronLeft aria-hidden />
        }
        gripIcon={<GripVertical aria-hidden className={cn(vertical && 'rotate-90')} />}
        collapsedContent={
          session.stats ? (
            <span className="px-2 font-mono text-xs text-muted tabular-nums">
              {t('session.fps', { fps: session.stats.fps })}
            </span>
          ) : null
        }
      >
        <Menu
          side={menuSide}
          trigger={
            <ToolbarButton label={t('session.quality')}>
              <Gauge aria-hidden />
            </ToolbarButton>
          }
        >
          <MenuGroup label={t('session.quality')}>
            <MenuRadioGroup<StreamQuality> value={quality} onValueChange={setQuality}>
              <MenuRadioItem value="auto">{t('session.qualityAuto')}</MenuRadioItem>
              <MenuRadioItem value="balanced">{t('session.qualityBalanced')}</MenuRadioItem>
              <MenuRadioItem value="sharp">{t('session.qualitySharp')}</MenuRadioItem>
              <MenuRadioItem value="speed">{t('session.qualitySpeed')}</MenuRadioItem>
            </MenuRadioGroup>
          </MenuGroup>
        </Menu>
        <Menu
          side={menuSide}
          trigger={
            <ToolbarButton label={t('session.display')}>
              <Monitor aria-hidden />
            </ToolbarButton>
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
          side={menuSide}
          trigger={
            <ToolbarButton label={t('session.keys')}>
              <Keyboard aria-hidden />
            </ToolbarButton>
          }
        >
          {keys.map((k, i) => (
            <div key={k.combo}>
              {i === 3 ? <MenuSeparator /> : null}
              <MenuItem onClick={() => void sendKeys(k.combo, k.label)}>{k.label}</MenuItem>
            </div>
          ))}
        </Menu>
        <ToolbarButton
          label={t('session.clipboardSync')}
          onClick={() => toast(t('session.clipboard'), { description: t('common.comingSoon') })}
        >
          <Clipboard aria-hidden />
        </ToolbarButton>
        <ToolbarButton
          label={t('session.files')}
          onClick={() => toast(t('session.files'), { description: t('common.comingSoon') })}
        >
          <FolderUp aria-hidden />
        </ToolbarButton>
        <ChatPopover sessionId={sessionId} messages={session.chat} side={menuSide} />
        <ToolbarButton
          label={recording ? t('session.recording') : t('session.record')}
          pressed={recording}
          onClick={onToggleRecording}
        >
          <Circle
            aria-hidden
            className={cn(recording && 'animate-pulse-soft fill-danger text-danger')}
          />
        </ToolbarButton>
        <ToolbarButton
          label={t('session.stats')}
          pressed={showStats}
          onClick={() => setShowStats(!showStats)}
        >
          <Activity aria-hidden />
        </ToolbarButton>
        <Menu
          side={menuSide}
          trigger={
            <ToolbarButton label={t('session.toolbarPosition')}>
              <PanelTop aria-hidden />
            </ToolbarButton>
          }
        >
          <MenuGroup label={t('session.toolbarPosition')}>
            <MenuRadioGroup<ToolbarEdge> value={edge} onValueChange={setEdge}>
              {TOOLBAR_EDGES.map((e) => (
                <MenuRadioItem key={e} value={e}>
                  {t(EDGE_LABEL[e])}
                </MenuRadioItem>
              ))}
            </MenuRadioGroup>
          </MenuGroup>
        </Menu>
        <ToolbarButton
          label={t('session.autoHide')}
          pressed={autoHide}
          onClick={() => setAutoHide(!autoHide)}
        >
          <EyeOff aria-hidden />
        </ToolbarButton>
        <ToolbarButton
          label={fullscreen ? t('session.exitFullscreen') : t('session.fullscreen')}
          onClick={onToggleFullscreen}
        >
          {fullscreen ? <Minimize aria-hidden /> : <Maximize aria-hidden />}
        </ToolbarButton>
        <ToolbarSeparator vertical={vertical} />
        <ToolbarButton label={t('session.end')} tone="danger" onClick={onEnd}>
          <PhoneOff aria-hidden />
        </ToolbarButton>
      </MorphToolbar>
    </>
  );
}

const STATS_POS: Record<ToolbarEdge, string> = {
  top: 'right-3 bottom-3',
  bottom: 'top-3 right-3',
  left: 'right-3 bottom-3',
  right: 'bottom-3 left-3',
};

/** Live numbers from the session engine; missing ones read "n/a", never 0. */
export function StatsOverlay({ stats, edge }: { stats: SessionStats; edge: ToolbarEdge }) {
  const { t } = useTranslation();
  const locale = useLocale();
  const decodeMs: unknown = Reflect.get(stats, 'decodeMs');
  const rows: [string, string][] = [
    [t('session.statFps'), t('session.fps', { fps: stats.fps })],
    [t('session.statBitrate'), formatBitrate(locale, stats.bitrateBps)],
    [t('session.statLatency'), `${stats.latencyMs} ms`],
    [
      t('session.statDecode'),
      typeof decodeMs === 'number' ? `${decodeMs.toFixed(1)} ms` : t('session.statUnavailable'),
    ],
    [t('session.statLoss'), formatPercent(locale, stats.lossRatio)],
    [t('session.statCodec'), stats.codec],
    [t('session.statResolution'), `${stats.width}×${stats.height}`],
    [t('session.statRoute'), stats.route],
  ];
  return (
    <aside
      aria-label={t('session.stats')}
      className={cn(
        'absolute z-30 w-56 rounded-xl glass-panel p-3 font-mono text-xs shadow-xl',
        STATS_POS[edge],
      )}
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
  side,
}: {
  sessionId: string | undefined;
  messages: SessionState['chat'];
  side: 'top' | 'bottom' | 'left' | 'right';
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
      side={side}
      className="w-80"
      trigger={
        <ToolbarButton label={t('session.chat')}>
          <MessageSquare aria-hidden />
        </ToolbarButton>
      }
    >
      <ul aria-live="polite" className="mb-3 flex max-h-64 min-h-24 flex-col gap-2 overflow-y-auto">
        {messages.length === 0 ? (
          <li className="m-auto text-xs text-muted">{t('session.chatEmpty')}</li>
        ) : (
          messages.map((msg) => (
            <m.li
              key={`${msg.at}-${msg.from}`}
              initial={d === 0 ? false : { opacity: 0, y: 4 }}
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
