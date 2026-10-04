import { createFileRoute, redirect } from '@tanstack/react-router';

import { isScrinId } from '../lib/search';
import { ConnectPage } from '../screens/connect';

export const Route = createFileRoute('/connect/$id')({
  beforeLoad: ({ params }) => {
    // TanStack Router's redirect is a thrown Response-like object by design.
    // eslint-disable-next-line @typescript-eslint/only-throw-error
    if (!isScrinId(params.id)) throw redirect({ to: '/' });
  },
  component: ConnectRoute,
});

function ConnectRoute() {
  const { id } = Route.useParams();
  return <ConnectPage id={id} />;
}
