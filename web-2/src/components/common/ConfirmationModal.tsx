import React from 'react';
import { AlertTriangle, AlertCircle, Info, Loader2 } from 'lucide-react';
import { Modal } from './Modal';
import { Button } from './Button';

export type ConfirmationVariant = 'danger' | 'warning' | 'neutral' | 'primary';

export interface ConfirmationModalProps {
  isOpen: boolean;
  onClose: () => void;
  onConfirm: () => void | Promise<void>;
  title: string;
  message: React.ReactNode;
  confirmLabel?: string;
  cancelLabel?: string;
  variant?: ConfirmationVariant;
  isProcessing?: boolean;
  icon?: React.ReactNode;
}

export const ConfirmationModal: React.FC<ConfirmationModalProps> = ({
  isOpen,
  onClose,
  onConfirm,
  title,
  message,
  confirmLabel = 'Confirm',
  cancelLabel = 'Cancel',
  variant = 'danger',
  isProcessing = false,
  icon
}) => {
  const variantConfig = {
    danger: {
      icon: <AlertTriangle className="w-5 h-5 text-rose-500" />,
      iconBg: 'bg-rose-500/10 text-rose-500 ring-rose-500/20',
      confirmButtonVariant: 'outline' as const,
      confirmButtonClass:
        'bg-rose-600 hover:bg-rose-500 text-white border-transparent focus:ring-rose-500 shadow-rose-950/20'
    },
    warning: {
      icon: <AlertTriangle className="w-5 h-5 text-amber-500" />,
      iconBg: 'bg-amber-500/10 text-amber-500 ring-amber-500/20',
      confirmButtonVariant: 'outline' as const,
      confirmButtonClass:
        'bg-amber-600 hover:bg-amber-500 text-white border-transparent focus:ring-amber-500 shadow-amber-950/20'
    },
    neutral: {
      icon: <Info className="w-5 h-5 text-neutral-400" />,
      iconBg: 'bg-neutral-500/10 text-neutral-400 ring-neutral-500/20',
      confirmButtonVariant: 'secondary' as const,
      confirmButtonClass: ''
    },
    primary: {
      icon: <AlertCircle className="w-5 h-5 text-teal-500" />,
      iconBg: 'bg-teal-500/10 text-teal-500 ring-teal-500/20',
      confirmButtonVariant: 'primary' as const,
      confirmButtonClass: ''
    }
  }[variant];

  return (
    <Modal
      isOpen={isOpen}
      onClose={isProcessing ? () => {} : onClose}
      maxWidth="sm"
      className="p-5 sm:p-6"
    >
      <div className="flex flex-col items-center text-center space-y-4">
        <div
          className={`w-11 h-11 rounded-2xl flex items-center justify-center ring-1 ${variantConfig.iconBg} shrink-0`}
        >
          {icon || variantConfig.icon}
        </div>

        <div className="space-y-1.5 max-w-xs">
          <h3 className="text-base font-bold text-neutral-900 dark:text-neutral-100">
            {title}
          </h3>
          <div className="text-xs text-neutral-500 dark:text-neutral-400 leading-relaxed">
            {message}
          </div>
        </div>

        <div className="flex items-center gap-2.5 w-full pt-2">
          <Button
            variant="outline"
            size="sm"
            onClick={onClose}
            disabled={isProcessing}
            className="flex-1 justify-center"
          >
            {cancelLabel}
          </Button>

          <Button
            variant={variantConfig.confirmButtonVariant}
            size="sm"
            onClick={onConfirm}
            disabled={isProcessing}
            className={`flex-1 justify-center font-semibold ${variantConfig.confirmButtonClass}`}
          >
            {isProcessing ? (
              <span className="flex items-center gap-1.5">
                <Loader2 className="w-3.5 h-3.5 animate-spin" />
                <span>Processing...</span>
              </span>
            ) : (
              confirmLabel
            )}
          </Button>
        </div>
      </div>
    </Modal>
  );
};
