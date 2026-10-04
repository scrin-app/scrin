import { createFileRoute } from '@tanstack/react-router';

import { devicesSearch } from '../../lib/search';
import { DevicesPage } from '../../screens/devices';

export const Route = createFileRoute('/_app/devices')({
  validateSearch: devicesSearch,
  component: DevicesRoute,
});

function DevicesRoute() {
  const search = Route.useSearch();
  const navigate = Route.useNavigate();
  return (
    <DevicesPage
      query={search.q ?? ''}
      group={search.group ?? 'all'}
      view={search.view ?? 'grid'}
      onChange={(patch) => {
        void navigate({
          search: (prev) => ({ ...prev, ...patch }),
          replace: true,
        });
      }}
    />
  );
}
