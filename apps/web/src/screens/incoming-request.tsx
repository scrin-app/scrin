import { useTranslation } from '@scrin/i18n';
import {
  dismissIncomingRequest,
  getHostRoleActions,
  loadHostExtras,
  RequestDialog,
  toast,
  useHost,
  useIncomingRequest,
  type PermissionName,
} from '@scrin/ui';
import { useEffect } from 'react';

/**
 * D-002: shows the anti-scam request dialog whenever the host engine reports
 * an incoming connection. Mounted once at the root on hosts that can be
 * controlled; the dialog itself decides nothing — the answer goes back to the
 * engine, which enforces the policy again.
 */
export function IncomingRequestHost() {
  const { t } = useTranslation();
  const host = useHost();
  const request = useIncomingRequest();

  useEffect(() => {
    // Starts the desktop host-role feed (events in, accept/reject out).
    void loadHostExtras(host);
  }, [host]);

  useEffect(() => {
    if (request) void host.notify(t('request.title'), t('request.scamTitle'));
  }, [request, host, t]);

  if (!request) return null;

  const finish = async (run: () => Promise<void> | undefined, ok: string) => {
    try {
      await run();
      toast(ok);
    } catch {
      toast.error(t('request.answerFailed'));
    } finally {
      dismissIncomingRequest(request.session);
    }
  };

  const accept = (permissions: PermissionName[]) =>
    void finish(
      () => getHostRoleActions()?.accept(request.session, permissions),
      t('request.accepted'),
    );

  const deny = (reason: 'user' | 'expired') =>
    void finish(
      () => getHostRoleActions()?.reject(request.session),
      reason === 'expired' ? t('request.expired') : t('request.denied'),
    );

  return <RequestDialog key={request.session} request={request} onAccept={accept} onDeny={deny} />;
}
