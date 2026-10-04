import { House, MonitorSmartphone, Settings } from 'lucide-react';

export const NAV_ITEMS = [
  { to: '/', labelKey: 'nav.home', icon: House },
  { to: '/devices', labelKey: 'nav.devices', icon: MonitorSmartphone },
  { to: '/settings', labelKey: 'nav.settings', icon: Settings },
] as const;
