import { useTranslation } from '@scrin/i18n';
import { buttonVariants } from '@scrin/ui';
import { Link } from '@tanstack/react-router';
import { Compass } from 'lucide-react';

export function NotFound() {
  const { t } = useTranslation();
  return (
    <main id="main" className="grid min-h-dvh place-items-center bg-bg px-6 text-center text-fg">
      <div className="flex max-w-md flex-col items-center gap-4">
        <span className="grid size-20 place-items-center rounded-3xl bg-accent/12 text-accent">
          <Compass aria-hidden className="size-10" />
        </span>
        <p className="font-mono text-sm font-semibold tracking-widest text-muted">404</p>
        <h1 className="text-3xl font-semibold tracking-tight">{t('notFound.title')}</h1>
        <p className="text-muted">{t('notFound.body')}</p>
        <Link to="/" className={buttonVariants({ size: 'lg' })}>
          {t('notFound.home')}
        </Link>
      </div>
    </main>
  );
}
