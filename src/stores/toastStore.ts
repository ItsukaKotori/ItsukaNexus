// 全局 toast(原型 toasts):错误/提示的统一出口;2.8s 自动消失,同屏至多 4 条。
import { create } from "zustand";

export interface ToastItem {
  id: number;
  msg: string;
  kind: "info" | "error";
}

let nextId = 1;

interface ToastState {
  toasts: ToastItem[];
  push: (msg: string, kind?: "info" | "error") => void;
}

export const useToasts = create<ToastState>((set) => ({
  toasts: [],
  push: (msg, kind = "info") => {
    const id = nextId++;
    set((st) => ({ toasts: [...st.toasts, { id, msg, kind }].slice(-4) }));
    setTimeout(() => {
      set((st) => ({ toasts: st.toasts.filter((t) => t.id !== id) }));
    }, 2800);
  },
}));

/** React 外便捷口(terminalManager/事件回调等非组件上下文) */
export function toast(msg: string, kind?: "info" | "error"): void {
  useToasts.getState().push(msg, kind);
}
