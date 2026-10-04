import { formatRelative, formatScrinId, useLocale, useTranslation } from '@scrin/i18n';
import {
  Badge,
  Button,
  buttonVariants,
  Card,
  cn,
  EmptyState,
  Input,
  Segmented,
  Select,
  StatusDot,
} from '@scrin/ui';
import { Link } from '@tanstack/react-router';
import { m } from 'motion/react';
import {
  ArrowRight,
  LayoutGrid,
  List,
  Monitor,
  MonitorSmartphone,
  Search,
  Smartphone,
} from 'lucide-react';
import { useMemo } from 'react';

import { PageHeader } from '../components/page-header';
import { DEVICE_GROUPS, DEVICES, type Device } from '../lib/mock-data';
import { useStagger } from '../lib/use-stagger';

type View = 'grid' | 'list';

export interface DevicesPageProps {
  query: string;
  group: string;
  view: View;
  devices?: readonly Device[];
  onChange: (patch: { q?: string | undefined; group?: string | undefined; view?: View }) => void;
}

export function DevicesPage({ query, group, view, devices = DEVICES, onChange }: DevicesPageProps) {
  const { t } = useTranslation();
  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    const digits = q.replace(/\s/g, '');
    return devices.filter(
      (d) =>
        (group === 'all' || d.group === group) &&
        (q === '' ||
          d.name.toLowerCase().includes(q) ||
          (digits !== '' && d.id.includes(digits)) ||
          d.tags.some((tag) => tag.includes(q))),
    );
  }, [devices, query, group]);

  const hasFilters = query !== '' || group !== 'all';

  return (
    <>
      <PageHeader
        title={t('devices.title')}
        subtitle={`${t('devices.subtitle')} · ${t('devices.count', { count: filtered.length })}`}
      />
      <div className="mb-4 flex flex-wrap items-center gap-2">
        <div className="relative min-w-0 flex-1 basis-64">
          <Search
            aria-hidden
            className="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-muted"
          />
          <Input
            type="search"
            aria-label={t('devices.search')}
            placeholder={t('devices.searchPlaceholder')}
            value={query}
            onChange={(e) => onChange({ q: e.currentTarget.value || undefined })}
            className="pl-9"
          />
        </div>
        <Select
          label={t('devices.groups')}
          value={group}
          onValueChange={(g) => onChange({ group: g === 'all' ? undefined : g })}
          options={[
            { value: 'all', label: t('devices.allGroups') },
            ...DEVICE_GROUPS.map((g) => ({ value: g, label: g })),
          ]}
          className="w-44"
        />
        <Segmented<View>
          label={t('devices.view')}
          value={view}
          onValueChange={(v) => onChange({ view: v })}
          options={[
            {
              value: 'grid',
              label: <span className="sr-only">{t('devices.viewGrid')}</span>,
              icon: <LayoutGrid aria-hidden />,
            },
            {
              value: 'list',
              label: <span className="sr-only">{t('devices.viewList')}</span>,
              icon: <List aria-hidden />,
            },
          ]}
        />
      </div>

      {devices.length === 0 ? (
        <EmptyState
          icon={<MonitorSmartphone aria-hidden />}
          title={t('devices.empty')}
          description={t('devices.emptyHint')}
        />
      ) : filtered.length === 0 ? (
        <EmptyState
          icon={<Search aria-hidden />}
          title={t('devices.noMatch', { query: query || group })}
          action={
            hasFilters ? (
              <Button
                variant="outline"
                onClick={() => onChange({ q: undefined, group: undefined })}
              >
                {t('devices.clearFilters')}
              </Button>
            ) : null
          }
        />
      ) : view === 'grid' ? (
        <ul className="grid grid-cols-1 gap-3 @lg:grid-cols-2 @4xl:grid-cols-3 @7xl:grid-cols-4 @[110rem]:grid-cols-5">
          {filtered.map((d, i) => (
            <DeviceCard key={d.id} device={d} index={i} />
          ))}
        </ul>
      ) : (
        <Card className="p-0 sm:p-0">
          <ul className="divide-y divide-outline">
            {filtered.map((d) => (
              <DeviceRow key={d.id} device={d} />
            ))}
          </ul>
        </Card>
      )}
    </>
  );
}

function OsIcon({ os }: { os: Device['os'] }) {
  return os === 'android' ? <Smartphone aria-hidden /> : <Monitor aria-hidden />;
}

function DeviceCard({ device: d, index }: { device: Device; index: number }) {
  const { t } = useTranslation();
  const locale = useLocale();
  const stagger = useStagger();
  return (
    <m.li
      {...stagger(Math.min(index, 8))}
      className="@container flex flex-col gap-3 rounded-xl glass-panel p-4"
    >
      <div className="flex items-start gap-3">
        <span className="grid size-10 shrink-0 place-items-center rounded-lg bg-surface-2 text-muted [&_svg]:size-5">
          <OsIcon os={d.os} />
        </span>
        <div className="min-w-0 flex-1">
          <h2 className="truncate text-sm font-semibold">{d.name}</h2>
          <p className="font-mono text-xs text-muted">{formatScrinId(d.id)}</p>
        </div>
        <StatusDot online={d.online} label={d.online ? t('common.online') : t('common.offline')} />
      </div>
      <div className="flex min-h-5 flex-wrap gap-1">
        <Badge>{d.group}</Badge>
        {d.tags.map((tag) => (
          <Badge key={tag} tone="accent">
            {tag}
          </Badge>
        ))}
      </div>
      <div className="mt-auto flex items-center justify-between gap-2">
        <span className="truncate text-xs text-muted">
          {t('devices.lastSeen', { time: formatRelative(locale, d.lastSeen) })}
        </span>
        <Link
          to="/"
          search={{ id: d.id }}
          className={buttonVariants({ size: 'sm', variant: d.online ? 'primary' : 'secondary' })}
          aria-label={`${t('devices.connect')}: ${d.name}`}
        >
          {t('devices.connect')}
        </Link>
      </div>
    </m.li>
  );
}

function DeviceRow({ device: d }: { device: Device }) {
  const { t } = useTranslation();
  const locale = useLocale();
  return (
    <li className="flex items-center gap-3 px-4 py-3">
      <span className="grid size-9 shrink-0 place-items-center rounded-lg bg-surface-2 text-muted [&_svg]:size-4">
        <OsIcon os={d.os} />
      </span>
      <div className="min-w-0 flex-1">
        <h2 className="truncate text-sm font-medium">{d.name}</h2>
        <p className="truncate text-xs text-muted">
          <span className="font-mono">{formatScrinId(d.id)}</span> · {d.group} ·{' '}
          {t('devices.lastSeen', { time: formatRelative(locale, d.lastSeen) })}
        </p>
      </div>
      <span className="hidden sm:inline">
        <StatusDot online={d.online} label={d.online ? t('common.online') : t('common.offline')} />
      </span>
      <Link
        to="/"
        search={{ id: d.id }}
        className={cn(buttonVariants({ size: 'icon-sm', variant: 'ghost' }))}
        aria-label={`${t('devices.connect')}: ${d.name}`}
      >
        <ArrowRight aria-hidden />
      </Link>
    </li>
  );
}
