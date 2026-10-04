import { formatScrinId, useTranslation } from '@scrin/i18n';
import { AlertDialog, Badge, cn, Skeleton, Spinner, useHost } from '@scrin/ui';
import { useNavigate } from '@tanstack/react-router';
import { Circle } from 'lucide-react';
import { lazy, Suspense, useEffect, useRef, useState } from 'react';

import { PHRASE_TARGET, useSessionEvents } from '../lib/engine';

// The toolbar (menus, chat, stats, drag) is its own chunk: the canvas paints first.
const SessionToolbar = lazy(() =>
  import('./session-toolbar').then((mod) => ({ default: mod.SessionToolbar })),
);

export function SessionPage({ id, sessionId }: { id: string; sessionId: string | undefined }) {
  const { t } = useTranslation();
  const label = id === PHRASE_TARGET ? t('home.phraseTitle') : formatScrinId(id);
  const host = useHost();
  const navigate = useNavigate();
  const session = useSessionEvents(sessionId);
  const rootRef = useRef<HTMLDivElement>(null);
  const [fullscreen, setFullscreen] = useState(false);
  const [recording, setRecording] = useState(false);
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

  const end = async () => {
    if (sessionId) await host.engine.endSession(sessionId);
    else void navigate({ to: '/' });
  };

  return (
    <div ref={rootRef} className="relative h-dvh w-full overflow-hidden bg-black text-fg">
      <h1 className="sr-only">{t('session.title', { id: label })}</h1>
      <main id="main" className="absolute inset-0 grid place-items-center">
        <RemoteCanvas label={label} sessionId={sessionId} hasStats={session.stats !== null} />
      </main>

      <Suspense
        fallback={
          <div className="absolute inset-x-0 top-0 flex justify-center pt-3">
            <Skeleton className="h-11 w-72 rounded-2xl" />
          </div>
        }
      >
        <SessionToolbar
          sessionId={sessionId}
          session={session}
          fullscreen={fullscreen}
          onToggleFullscreen={() => void toggleFullscreen()}
          recording={recording}
          onToggleRecording={() => setRecording((r) => !r)}
          onEnd={() => setConfirmEnd(true)}
        />
      </Suspense>

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
 * The remote screen. With a real browser session (WebHost + server) the
 * stream is decoded and drawn into the canvas; the session client is loaded
 * lazily so neither it nor the wasm module is in the initial bundle. In demo
 * mode (mock engine) and on desktop (native surface on top) the canvas stays
 * unbound and the placeholder shows.
 */
function RemoteCanvas({
  label,
  sessionId,
  hasStats,
}: {
  label: string;
  sessionId: string | undefined;
  hasStats: boolean;
}) {
  const { t } = useTranslation();
  const host = useHost();
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const [live, setLive] = useState(false);
  const [firstFrame, setFirstFrame] = useState(false);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (host.platform.kind !== 'web' || !sessionId || !canvas) return undefined;
    let detach: (() => void) | null = null;
    let cancelled = false;
    void import('@scrin/ui/web-client').then(({ attachCanvas }) => {
      if (cancelled) return;
      const attached = attachCanvas(sessionId, canvas, {
        onLive: () => setLive(true),
        onFirstFrame: () => setFirstFrame(true),
      });
      detach = () => {
        attached.detach();
      };
    });
    return () => {
      cancelled = true;
      detach?.();
    };
  }, [host.platform.kind, sessionId]);

  const hasVideo = live ? firstFrame : hasStats;
  return (
    <figure
      data-scrin-video=""
      className="relative m-0 aspect-[43/18] max-h-full w-full max-w-full overflow-hidden"
    >
      <figcaption className="sr-only">{t('session.canvasLabel', { id: label })}</figcaption>
      {live ? null : (
        <>
          {/* Demo-mode placeholder "desktop" (mock engine, no stream). */}
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
        </>
      )}
      {/* Focusable so keyboard input reaches the remote; input is captured by the session client. */}
      <canvas
        ref={canvasRef}
        tabIndex={live ? 0 : -1}
        aria-label={t('session.canvasLabel', { id: label })}
        className={cn(
          'absolute inset-0 h-full w-full touch-none object-contain outline-none focus-visible:ring-2 focus-visible:ring-accent',
          !live && 'pointer-events-none',
        )}
      />
      {hasVideo ? null : (
        <div className="pointer-events-none absolute inset-0 grid place-items-center bg-black/55 text-white">
          <span className="flex items-center gap-3 text-sm">
            <Spinner label={false} /> {t('session.waitingVideo')}
          </span>
        </div>
      )}
    </figure>
  );
}
