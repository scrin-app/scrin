import { createFileRoute, redirect } from '@tanstack/react-router';

import { isScrinId, sessionSearch } from '../lib/search';
import { SessionPage } from '../screens/session';

export const Route = createFileRoute('/session/$id')({
  validateSearch: sessionSearch,
  beforeLoad: ({ params }) => {
    // TanStack Router's redirect is a thrown Response-like object by design.
    // eslint-disable-next-line @typescript-eslint/only-throw-error
    if (!isScrinId(params.id)) throw redirect({ to: '/' });
  },
  component: SessionRoute,
});

function SessionRoute() {
  const { id } = Route.useParams();
  const { s } = Route.useSearch();
  return <SessionPage id={id} sessionId={s} />;
}
