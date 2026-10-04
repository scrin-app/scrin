import { useTranslation, type TranslationKey } from '@scrin/i18n';
import { cn, Skeleton, Tab, Tabs, TabsList, TabsPanel } from '@scrin/ui';
import {
  Cog,
  Download,
  Info,
  Keyboard,
  Languages,
  Monitor,
  Network,
  Palette,
  ShieldCheck,
  Volume2,
  type LucideIcon,
} from 'lucide-react';
import { lazy, Suspense, useEffect, useRef, useState, type RefObject } from 'react';

import { PageHeader } from '../components/page-header';

const SECTION_IDS = [
  'general',
  'security',
  'network',
  'video',
  'audio',
  'input',
  'appearance',
  'language',
  'updates',
  'about',
] as const;

export type SectionId = (typeof SECTION_IDS)[number];

const SECTIONS: Record<SectionId, { label: TranslationKey; icon: LucideIcon }> = {
  general: { label: 'settings.general', icon: Cog },
  security: { label: 'settings.security', icon: ShieldCheck },
  network: { label: 'settings.network', icon: Network },
  video: { label: 'settings.video', icon: Monitor },
  audio: { label: 'settings.audio', icon: Volume2 },
  input: { label: 'settings.input', icon: Keyboard },
  appearance: { label: 'settings.appearance', icon: Palette },
  language: { label: 'settings.language', icon: Languages },
  updates: { label: 'settings.updates', icon: Download },
  about: { label: 'settings.about', icon: Info },
};

// Section bodies (forms, selects, the updater) are a separate chunk; the
// shell and navigation paint first with a skeleton.
const SettingsSection = lazy(() =>
  import('./settings-sections').then((m) => ({ default: m.SettingsSection })),
);

function isSection(v: string): v is SectionId {
  return (SECTION_IDS as readonly string[]).includes(v);
}

function initialSection(): SectionId {
  const hash = typeof location === 'undefined' ? '' : location.hash.slice(1);
  return isSection(hash) ? hash : 'general';
}

/** Wide when the settings container (not the window) is at least 48rem. */
function useWide(ref: RefObject<HTMLElement | null>): boolean {
  const [wide, setWide] = useState(false);
  useEffect(() => {
    const el = ref.current;
    if (!el || typeof ResizeObserver === 'undefined') return undefined;
    const ro = new ResizeObserver(([entry]) => {
      if (entry) setWide(entry.contentRect.width >= 768);
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, [ref]);
  return wide;
}

/**
 * D-005. One section at a time: a vertical sidebar when the container is
 * wide, a scrollable tab strip when it is narrow (360 px phones up to 32:9).
 * The active section is mirrored in the URL hash so links such as
 * `/settings#network` land on it.
 */
export function SettingsPage() {
  const { t } = useTranslation();
  const [section, setSection] = useState<SectionId>(initialSection);
  const rootRef = useRef<HTMLDivElement>(null);
  const wide = useWide(rootRef);

  const select = (v: unknown) => {
    if (typeof v !== 'string' || !isSection(v)) return;
    setSection(v);
    if (typeof history !== 'undefined') history.replaceState(history.state, '', `#${v}`);
  };

  return (
    <div ref={rootRef} className="@container">
      <PageHeader title={t('settings.title')} />
      <Tabs
        value={section}
        onValueChange={select}
        orientation={wide ? 'vertical' : 'horizontal'}
        className="grid gap-6 @3xl:grid-cols-[13rem_minmax(0,1fr)] @7xl:grid-cols-[15rem_minmax(0,60rem)]"
      >
        <TabsList
          aria-label={t('settings.sections')}
          indicator={false}
          className={cn(
            'max-w-full justify-start overflow-x-auto',
            '@3xl:sticky @3xl:top-24 @3xl:flex-col @3xl:items-stretch @3xl:self-start @3xl:overflow-visible',
          )}
        >
          {SECTION_IDS.map((id) => {
            const { label, icon: Icon } = SECTIONS[id];
            return (
              <Tab
                key={id}
                value={id}
                id={`settings-tab-${id}`}
                className="h-10 shrink-0 justify-start whitespace-nowrap data-active:bg-surface data-active:shadow-sm @3xl:w-full"
              >
                <Icon aria-hidden />
                {t(label)}
              </Tab>
            );
          })}
        </TabsList>
        {SECTION_IDS.map((id) => (
          <TabsPanel key={id} value={id} className="mt-0 min-w-0">
            {id === section ? (
              <Suspense fallback={<SectionSkeleton />}>
                <SettingsSection id={id} title={t(SECTIONS[id].label)} />
              </Suspense>
            ) : null}
          </TabsPanel>
        ))}
      </Tabs>
    </div>
  );
}

function SectionSkeleton() {
  const { t } = useTranslation();
  return (
    <output aria-label={t('common.loading')} className="block rounded-xl glass-panel p-6">
      <Skeleton className="mb-5 h-6 w-40" />
      <div className="flex flex-col gap-4">
        <Skeleton className="h-10 w-full" />
        <Skeleton className="h-10 w-5/6" />
        <Skeleton className="h-10 w-2/3" />
      </div>
    </output>
  );
}
