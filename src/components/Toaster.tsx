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
        // 原型 .toast:border-strong 描边 + float 阴影 + rise 入场(8px 上浮渐显)
        <div
          key={t.id}
          className="toast-rise flex min-w-[260px] max-w-[400px] items-center gap-2.5 rounded-xl border border-border-strong bg-card px-3 py-2.5 text-[12.5px] shadow-[0_10px_24px_oklch(0.2_0_0_/_0.18)]"
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
