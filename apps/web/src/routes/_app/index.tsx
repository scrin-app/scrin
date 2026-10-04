import { createFileRoute } from '@tanstack/react-router';

import { homeSearch } from '../../lib/search';
import { HomePage } from '../../screens/home';

export const Route = createFileRoute('/_app/')({
  /** `?id=` prefills the partner ID (command palette, devices list, share link). */
  validateSearch: homeSearch,
  component: HomeRoute,
});

function HomeRoute() {
  const { id } = Route.useSearch();
  return <HomePage prefillId={id} />;
}
