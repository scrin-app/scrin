import { AlertDialog as BaseAlert } from '@base-ui/react/alert-dialog';
import { Dialog as BaseDialog } from '@base-ui/react/dialog';
import { useTranslation } from '@scrin/i18n';
import { X } from 'lucide-react';
import type { ReactElement, ReactNode } from 'react';

import { cn } from '../lib/cn';
import { Button, buttonVariants } from './button';

const backdropClass = cn(
  'fixed inset-0 z-50 bg-black/45 backdrop-blur-[2px]',
  'transition-opacity duration-(--scrin-dur) ease-scrin',
  'data-ending-style:opacity-0 data-starting-style:opacity-0',
);

const popupClass = cn(
  'fixed top-1/2 left-1/2 z-50 w-[min(32rem,calc(100vw-2rem))] -translate-x-1/2 -translate-y-1/2 glass-panel',
  'max-h-[calc(100dvh-2rem)] overflow-y-auto rounded-xl p-6 shadow-2xl outline-none',
  'transition-[opacity,transform] duration-(--scrin-dur) ease-scrin',
  'data-starting-style:scale-95 data-starting-style:opacity-0',
  'data-ending-style:scale-95 data-ending-style:opacity-0',
);

export interface DialogProps {
  open?: boolean;
  defaultOpen?: boolean;
  onOpenChange?: (open: boolean) => void;
  /** Element that opens the dialog, e.g. a <Button>. */
  trigger?: ReactElement;
  title: ReactNode;
  description?: ReactNode;
  children?: ReactNode;
  footer?: ReactNode;
  className?: string;
}

export function Dialog({
  open,
  defaultOpen,
  onOpenChange,
  trigger,
  title,
  description,
  children,
  footer,
  className,
}: DialogProps) {
  const { t } = useTranslation();
  return (
    <BaseDialog.Root
      {...(open !== undefined ? { open } : {})}
      {...(defaultOpen !== undefined ? { defaultOpen } : {})}
      {...(onOpenChange ? { onOpenChange: (o: boolean) => onOpenChange(o) } : {})}
    >
      {trigger ? <BaseDialog.Trigger render={trigger} /> : null}
      <BaseDialog.Portal>
        <BaseDialog.Backdrop className={backdropClass} />
        <BaseDialog.Popup className={cn(popupClass, className)}>
          <div className="flex items-start justify-between gap-4">
            <BaseDialog.Title className="text-lg font-semibold text-fg">{title}</BaseDialog.Title>
            <BaseDialog.Close
              aria-label={t('ui.dialogClose')}
              className={buttonVariants({ variant: 'ghost', size: 'icon-sm' })}
            >
              <X aria-hidden />
            </BaseDialog.Close>
          </div>
          {description ? (
            <BaseDialog.Description className="mt-1 text-sm text-muted">
              {description}
            </BaseDialog.Description>
          ) : null}
          {children ? <div className="mt-4">{children}</div> : null}
          {footer ? <div className="mt-6 flex flex-wrap justify-end gap-2">{footer}</div> : null}
        </BaseDialog.Popup>
      </BaseDialog.Portal>
    </BaseDialog.Root>
  );
}

export interface AlertDialogProps {
  open?: boolean;
  onOpenChange?: (open: boolean) => void;
  trigger?: ReactElement;
  title: ReactNode;
  description: ReactNode;
  confirmLabel: ReactNode;
  cancelLabel?: ReactNode;
  destructive?: boolean;
  onConfirm: () => void;
}

/** Confirmation that cannot be dismissed by clicking outside. */
export function AlertDialog({
  open,
  onOpenChange,
  trigger,
  title,
  description,
  confirmLabel,
  cancelLabel,
  destructive = false,
  onConfirm,
}: AlertDialogProps) {
  const { t } = useTranslation();
  return (
    <BaseAlert.Root
      {...(open !== undefined ? { open } : {})}
      {...(onOpenChange ? { onOpenChange: (o: boolean) => onOpenChange(o) } : {})}
    >
      {trigger ? <BaseAlert.Trigger render={trigger} /> : null}
      <BaseAlert.Portal>
        <BaseAlert.Backdrop className={backdropClass} />
        <BaseAlert.Popup className={cn(popupClass, 'w-[min(26rem,calc(100vw-2rem))]')}>
          <BaseAlert.Title className="text-lg font-semibold text-fg">{title}</BaseAlert.Title>
          <BaseAlert.Description className="mt-2 text-sm text-muted">
            {description}
          </BaseAlert.Description>
          <div className="mt-6 flex flex-wrap justify-end gap-2">
            <BaseAlert.Close render={<Button variant="ghost" />}>
              {cancelLabel ?? t('common.cancel')}
            </BaseAlert.Close>
            <BaseAlert.Close
              render={<Button variant={destructive ? 'danger' : 'primary'} />}
              onClick={onConfirm}
            >
              {confirmLabel}
            </BaseAlert.Close>
          </div>
        </BaseAlert.Popup>
      </BaseAlert.Portal>
    </BaseAlert.Root>
  );
}
