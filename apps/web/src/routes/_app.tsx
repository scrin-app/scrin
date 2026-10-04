import { createFileRoute } from '@tanstack/react-router';

import { AppShell } from '../components/app-shell';

/** Pathless layout: every route under it gets the sidebar / tab bar shell. */
export const Route = createFileRoute('/_app')({
  component: AppShell,
});
