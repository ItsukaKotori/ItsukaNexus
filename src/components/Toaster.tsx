// toast 渲染层:右下角堆叠(原型 #toasts 位),不挡标签栏。
import { Check, Zap } from "lucide-react";

import { useToasts } from "../stores/toastStore";

export default function Toaster() {
  const toasts = useToasts((s) => s.toasts);
  if (toasts.length === 0) return null;
  return (
    <div
      aria-live="polite"
      className="fixed right-4 bottom-4 z-50 flex flex-col gap-2"
    >
      {toasts.map((t) => (
        <div
          key={t.id}
          className="flex min-w-[240px] max-w-[400px] items-center gap-2.5 rounded-lg border border-border bg-card px-3 py-2.5 text-xs shadow-lg"
        >
          {t.kind === "error" ? (
            <Zap className="size-4 shrink-0 text-destructive" />
          ) : (
            <Check className="size-4 shrink-0 text-status-ok" />
          )}
          <span className="min-w-0 break-all">{t.msg}</span>
        </div>
      ))}
    </div>
  );
}
