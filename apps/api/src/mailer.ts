import type { Logger } from './logger.ts';

export interface MailMessage {
  to: string;
  subject: string;
  text: string;
  html?: string;
}

/** Transactional email. The official instance sends through brivio (ADR-0007), never Resend. */
export interface Mailer {
  readonly enabled: boolean;
  send(message: MailMessage): Promise<void>;
}

interface BrivioConfig {
  apiUrl: string;
  apiKey: string;
  from: string;
  logger: Logger;
  fetch?: typeof fetch;
  timeoutMs?: number;
}

export class BrivioMailer implements Mailer {
  readonly enabled = true;
  readonly #cfg: BrivioConfig;

  constructor(cfg: BrivioConfig) {
    this.#cfg = cfg;
  }

  async send(message: MailMessage): Promise<void> {
    const { apiUrl, apiKey, from, logger, timeoutMs = 10_000 } = this.#cfg;
    const doFetch = this.#cfg.fetch ?? fetch;
    const url = `${apiUrl.replace(/\/+$/, '')}/v1/email/send`;
    try {
      const res = await doFetch(url, {
        method: 'POST',
        headers: { 'content-type': 'application/json', authorization: `Bearer ${apiKey}` },
        body: JSON.stringify({ from, ...message }),
        signal: AbortSignal.timeout(timeoutMs),
      });
      if (!res.ok) {
        logger.error({ status: res.status, subject: message.subject }, 'brivio email rejected');
        return;
      }
      logger.info({ subject: message.subject }, 'email sent via brivio');
    } catch (err) {
      // Never surface mail failures to the caller: that would reveal account existence.
      logger.error({ err, subject: message.subject }, 'brivio email failed');
    }
  }
}

/** Email not configured (self-host without brivio): log the intent, send nothing. */
export class DisabledMailer implements Mailer {
  readonly enabled = false;
  readonly #logger: Logger;

  constructor(logger: Logger) {
    this.#logger = logger;
  }

  send(message: MailMessage): Promise<void> {
    this.#logger.warn({ subject: message.subject }, 'email disabled (BRIVIO_API_URL/KEY unset)');
    return Promise.resolve();
  }
}

export function createMailer(opts: {
  apiUrl: string | undefined;
  apiKey: string | undefined;
  from: string;
  logger: Logger;
}): Mailer {
  if (opts.apiUrl === undefined || opts.apiKey === undefined)
    return new DisabledMailer(opts.logger);
  return new BrivioMailer({
    apiUrl: opts.apiUrl,
    apiKey: opts.apiKey,
    from: opts.from,
    logger: opts.logger,
  });
}
