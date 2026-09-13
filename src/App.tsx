// M4 组装根:启动恢复 + 全局事件 + 关 tab 流程(M3 语义不变)。
// 布局在 app/Workbench;会话数据 sessionsStore,tab 视图 tabStore。
import { useCallback, useEffect, useRef, useState } from "react";

import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { Checkbox } from "@/components/ui/checkbox";
import { Label } from "@/components/ui/label";

import Toaster from "@/components/Toaster";

import Workbench from "./app/Workbench";
import {
  disposeEntry,
  resizeSession,
  setTerminalConfig,
  stopSession,
} from "./features/terminal/terminalManager";
import {
  configGet,
  gitCheck,
  sessionDispose,
  sessionList,
  worktreeRemove,
} from "./ipc/commands";
import {
  onSessionExitEvent,
  onSessionStateEvent,
  onWorktreeChanged,
} from "./ipc/events";
import type { GitCheckInfo } from "./ipc/types";
import { useProjects } from "./stores/projectStore";
import { useSessions } from "./stores/sessionsStore";
import { useTabs } from "./stores/tabStore";
import { toast } from "./stores/toastStore";

function App() {
  const hydrate = useSessions((s) => s.hydrate);
  // 确认框打开期间后台自行退出的观察依赖(effect 按 sessions 快照变化重跑)
  const sessions = useSessions((s) => s.sessions);

  // git 探测结果经 props 传 ProjectSide(左栏黄条)
  const [gitInfo, setGitInfo] = useState<GitCheckInfo | null>(null);
  // 启动流程只跑一次(StrictMode 下 effect 双执行)
  const bootedRef = useRef(false);
  // 关闭收尾在途的会话:确认框重复确认/收尾中再点不去重入 stop(仅 Running 可停)
  const closingRef = useRef(new Set<string>());

  // 关 tab 确认(运行中会话):附带的 worktree 清理选项
  const [confirmClose, setConfirmClose] = useState<{
    id: string;
    worktree?: { repoPath: string; name: string };
  } | null>(null);
  const [alsoRemoveWt, setAlsoRemoveWt] = useState(true);

  // 启动恢复:config 注入(先于任何终端实例创建)→ git 探测 → session_list(并重建 tab 视图)
  useEffect(() => {
    if (bootedRef.current) return;
    bootedRef.current = true;
    void (async () => {
      try {
        setTerminalConfig((await configGet()).terminal);
      } catch (e) {
        console.error("[app] config_get 失败,用内置默认终端配置", e);
      }
      try {
        setGitInfo(await gitCheck());
      } catch (e) {
        console.error("[app] git_check 失败", e);
      }
      try {
        // 项目树异步填充:不 await,不阻塞会话恢复
        void useProjects.getState().loadAll();
        const snaps = await sessionList();
        hydrate(snaps);
        useTabs.getState().rebuildFromSessions(snaps);
      } catch (e) {
        console.error("[app] session_list 失败,按空会话启动", e);
      }
    })();
  }, [hydrate]);

  // 全局事件 → store(worktree://changed 由 projectStore 接收)
  useEffect(() => {
    const unState = onSessionStateEvent((sc) =>
      useSessions.getState().onState(sc.sessionId, sc.next)
    );
    const unExit = onSessionExitEvent((ev) =>
      useSessions.getState().onExit(ev)
    );
    const unWt = onWorktreeChanged((ev) =>
      useProjects.getState().applyWorktreeChange(ev)
    );
    return () => {
      void unState.then((u) => u());
      void unExit.then((u) => u());
      void unWt.then((u) => u());
    };
  }, []);

  // pane fit 上报 → 后端 resize(80x24 创建的初始校正 + 窗口变化)
  const handleFitted = useCallback((id: string, cols: number, rows: number) => {
    resizeSession(id, cols, rows);
  }, []);

  // 关 tab 流程(M3 语义不变):
  // - 终态:sessionDispose(Rust 侧删条目)+ 本地移除(数据 + tab 视图)
  // - 运行中:AlertDialog 确认「停止并关闭」;绑了 worktree 的附选「同时删除
  //   worktree」→ stop(等 Exit)→ 可选 worktreeRemove → sessionDispose
  const handleClose = useCallback((sessionId: string) => {
    const snap = useSessions.getState().sessions[sessionId];
    if (!snap || closingRef.current.has(sessionId)) return;

    if (snap.state === "exited" || snap.state === "failed") {
      closingRef.current.add(sessionId);
      void sessionDispose(sessionId)
        .catch((e) => console.error("[app] dispose 失败", e))
        .finally(() => {
          closingRef.current.delete(sessionId);
          disposeEntry(sessionId);
          useSessions.getState().removeSession(sessionId);
          useTabs.getState().closeTab(`term-${sessionId}`);
        });
      return;
    }
    setConfirmClose({
      id: sessionId,
      worktree:
        snap.repoPath && snap.worktreeName
          ? { repoPath: snap.repoPath, name: snap.worktreeName }
          : undefined,
    });
  }, []);

  const confirmStopAndClose = useCallback(async () => {
    if (!confirmClose) return;
    const { id, worktree } = confirmClose;
    setConfirmClose(null);
    if (closingRef.current.has(id)) return;
    closingRef.current.add(id);
    const finish = (): void => {
      closingRef.current.delete(id);
      disposeEntry(id);
      useSessions.getState().removeSession(id);
      useTabs.getState().closeTab(`term-${id}`);
    };
    try {
      await stopSession(id);
    } catch {
      // 竞态兜底:进程已自行退出/已在停止中,后端会拒停——以 store 终态为准,
      // 仍活着则中止(不能把活会话从本地摘掉)
      const cur = useSessions.getState().sessions[id];
      if (cur && cur.state !== "exited" && cur.state !== "failed") {
        closingRef.current.delete(id);
        toast(`停止会话失败:当前状态 ${cur.state}`, "error");
        return;
      }
    }
    if (worktree && alsoRemoveWt) {
      await worktreeRemove(worktree.repoPath, worktree.name, true).catch((e) =>
        toast(`worktree 清理失败:${String(e)}`, "error")
      );
    }
    await sessionDispose(id).catch(() => {}); // stop 后已终态;失败不阻本地收尾
    finish();
  }, [confirmClose, alsoRemoveWt]);

  // 确认框打开期间会话自行退出:不再需要确认,直接收尾(M3 终审 Minor)
  useEffect(() => {
    if (!confirmClose) return;
    const snap = useSessions.getState().sessions[confirmClose.id];
    if (snap && (snap.state === "exited" || snap.state === "failed")) {
      const id = confirmClose.id;
      setConfirmClose(null);
      closingRef.current.add(id);
      void sessionDispose(id)
        .catch(() => {})
        .finally(() => {
          closingRef.current.delete(id);
          disposeEntry(id);
          useSessions.getState().removeSession(id);
          useTabs.getState().closeTab(`term-${id}`);
        });
    }
  }, [confirmClose, sessions]);

  return (
    <>
      <Workbench gitInfo={gitInfo} onCloseTab={handleClose} onFitted={handleFitted} />
      <Toaster />
      <AlertDialog
        open={confirmClose !== null}
        onOpenChange={(v) => {
          // 关闭即重置附选项(M-5):不残留上一次的勾选状态
          if (!v) {
            setConfirmClose(null);
            setAlsoRemoveWt(true);
          }
        }}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>停止并关闭会话?</AlertDialogTitle>
            <AlertDialogDescription>
              会话仍在运行:关闭会先优雅停止(Ctrl+C → 宽限 → 超时强杀),输出历史随之释放。
            </AlertDialogDescription>
          </AlertDialogHeader>
          {confirmClose?.worktree && (
            <div className="flex items-center gap-2">
              <Checkbox
                id="also-remove-wt"
                checked={alsoRemoveWt}
                onCheckedChange={(v) => setAlsoRemoveWt(v === true)}
              />
              <Label htmlFor="also-remove-wt">
                同时删除 worktree(分支与目录)
              </Label>
            </div>
          )}
          <AlertDialogFooter>
            <AlertDialogCancel>取消</AlertDialogCancel>
            <AlertDialogAction onClick={() => void confirmStopAndClose()}>
              停止并关闭
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </>
  );
}

export default App;
