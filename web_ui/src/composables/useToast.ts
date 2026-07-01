import { readonly, ref } from 'vue';
import type { Ref } from 'vue';

const TOAST_DURATION_MS = 5_000;

export type ToastType = 'info' | 'success' | 'warning' | 'error';

export interface ToastAction {
  label: string;
  onClick: () => void;
}

export interface Toast {
  id: number;
  message: string;
  type: ToastType;
  action?: ToastAction;
}

export interface ToastOptions {
  durationMs?: number;
  action?: ToastAction;
}

export type ToastApi = Record<ToastType, (message: string, options?: ToastOptions) => number>;

const toasts = ref<Toast[]>([]);
let nextToastId = 1;

export function useToast(): {
  toasts: Readonly<Ref<readonly Toast[]>>;
  dismissToast: (id: number) => void;
  toast: ToastApi;
} {
  return {
    toasts: readonly(toasts),
    dismissToast,
    toast,
  };
}

function dismissToast(id: number): void {
  const index = toasts.value.findIndex((toastItem) => toastItem.id === id);
  if (index !== -1) {
    toasts.value.splice(index, 1);
  }
}

function pushToast(type: ToastType, message: string, options?: ToastOptions): number {
  const id = nextToastId++;
  toasts.value.push({ id, type, message, action: options?.action });
  window.setTimeout(() => dismissToast(id), options?.durationMs ?? TOAST_DURATION_MS);
  return id;
}

const toast: ToastApi = {
  info: (message, options) => pushToast('info', message, options),
  success: (message, options) => pushToast('success', message, options),
  warning: (message, options) => pushToast('warning', message, options),
  error: (message, options) => pushToast('error', message, options),
};
