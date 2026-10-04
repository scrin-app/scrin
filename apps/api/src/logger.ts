import { pino, type Logger } from 'pino';

export type { Logger };

export function createLogger(level: string): Logger {
  return pino({
    level,
    base: { service: 'scrin-api' },
    timestamp: pino.stdTimeFunctions.isoTime,
    // Defence in depth: these must never be logged, but if an object slips in, censor it.
    redact: {
      paths: [
        'authorization',
        'cookie',
        '*.authorization',
        '*.cookie',
        '*.password',
        '*.token',
        '*.secret',
        '*.apiKey',
        '*.signature',
      ],
      censor: '[redacted]',
    },
  });
}
