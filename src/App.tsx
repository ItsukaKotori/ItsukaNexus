// M3 AppShell:顶栏(应用信息 + git 状态 + 新建)+ 会话 tab 栏 + 常驻终端区。
// 所有会话的 pane 恒久挂载,display 切换可见性——输出直达 term.write,
// 状态经全局事件驱动 store,React 只负责壳(spec §1.5)。
// M3:新建走 NewSessionDialog(repo/worktree);关 tab 即删(dispose + 可选
// 清 worktree);顶栏展示 git_check 探测结果(worktree 功能可用性引导)。
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
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Label } from "@/components/ui/label";

import NewSessionDialog from "./features/launch/NewSessionDialog";
import TerminalPane from "./features/terminal/TerminalPane";
import TerminalTabs from "./features/terminal/TerminalTabs";
import {
  disposeEntry,
  resizeSession,
  setTerminalConfig,
  stopSession,
} from "./features/terminal/terminalManager";
import {
  configGet,
  getAppInfo,
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
import type { AppInfo, GitCheckInfo } from "./ipc/types";
import { useSessions } from "./stores/sessionsStore";
import { useWorktrees } from "./stores/worktreeStore";

function App() {
  const sessions = useSessions((s) => s.sessions);
  const activeId = useSessions((s) => s.activeId);
  const setActive = useSessions((s) => s.setActive);
  const hydrate = useSessions((s) => s.hydrate);
  const closeTab = useSessions((s) => s.closeTab);

  const [info, setInfo] = useState<AppInfo | null>(null);
  const [gitInfo, setGitInfo] = useState<GitCheckInfo | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [newDialogOpen, setNewDialogOpen] = useState(false);
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

  // 启动恢复:app_info → config 注入(先于任何终端实例创建)→ git 探测 → session_list
  useEffect(() => {
    if (bootedRef.current) return;
    bootedRef.current = true;
    void (async () => {
      try {
        setInfo(await getAppInfo());
      } catch (e) {
        console.error("[app] app_info 失败", e);
      }
      try {
        setTerminalConfig((await configGet()).terminal);
      } catch (e) {
        console.error("[app] config_get 失败,用内置默认终端配置", e);
      }
      try {
        setGitInfo(await gitCheck());
      } catch (e) {
        console.error("[app] git_check 失败,顶栏不展示 git 状态", e);
      }
      try {
        hydrate(await sessionList());
      } catch (e) {
        console.error("[app] session_list 失败,按空会话启动", e);
      }
    })();
  }, []);

  // 全局事件 → store(订阅随组件生命周期;StrictMode 重挂会重订,事件幂等)
  useEffect(() => {
    const unState = onSessionStateEvent((sc) =>
      useSessions.getState().onState(sc.sessionId, sc.next)
    );
    const unExit = onSessionExitEvent((ev) =>
      useSessions.getState().onExit(ev)
    );
    const unWt = onWorktreeChanged((ev) =>
      useWorktrees.getState().applyChange(ev)
    );
    return () => {
      void unState.then((u) => u());
      void unExit.then((u) => u());
      void unWt.then((u) => u());
    };
  }, []);

  // 新建:打开对话框(repo 选择 + git 校验 + 可选 worktree 都在里面完成)
  const handleNew = useCallback(() => {
    setError(null);
    setNewDialogOpen(true);
  }, []);

  // pane fit 上报 → 后端 resize(80x24 创建的初始校正 + 窗口变化)
  const handleFitted = useCallback((id: string, cols: number, rows: number) => {
    resizeSession(id, cols, rows);
  }, []);

  // 关 tab(关 tab 即删):
  // - 终态:sessionDispose(Rust 侧删条目)+ 本地移除
  // - 运行中:AlertDialog 确认「停止并关闭」;绑了 worktree 的附选「同时删除
  //   worktree」→ stop(等 Exit)→ 可选 worktreeRemove → sessionDispose
  const handleClose = useCallback(
    (id: string) => {
      const snap = useSessions.getState().sessions[id];
      if (!snap || closingRef.current.has(id)) return;

      if (snap.state === "exited" || snap.state === "failed") {
        closingRef.current.add(id);
        void sessionDispose(id)
          .catch((e) => console.error("[app] dispose 失败", e))
          .finally(() => {
            closingRef.current.delete(id);
            disposeEntry(id);
            closeTab(id);
          });
        return;
      }
      setConfirmClose({
        id,
        worktree:
          snap.repoPath && snap.worktreeName
            ? { repoPath: snap.repoPath, name: snap.worktreeName }
            : undefined,
      });
    },
    [closeTab]
  );

  const confirmStopAndClose = useCallback(async () => {
    if (!confirmClose) return;
    const { id, worktree } = confirmClose;
    setConfirmClose(null);
    if (closingRef.current.has(id)) return;
    closingRef.current.add(id);
    const finish = (): void => {
      closingRef.current.delete(id);
      disposeEntry(id);
      closeTab(id);
    };
    try {
      await stopSession(id);
    } catch {
      // 竞态兜底:进程已自行退出/已在停止中,后端会拒停——以 store 终态为准,
      // 仍活着则中止(不能把活会话从本地摘掉)
      const cur = useSessions.getState().sessions[id];
      if (cur && cur.state !== "exited" && cur.state !== "failed") {
        closingRef.current.delete(id);
        setError(`停止会话失败:当前状态 ${cur.state}`);
        return;
      }
    }
    if (worktree && alsoRemoveWt) {
      await worktreeRemove(worktree.repoPath, worktree.name, true).catch((e) =>
        setError(`worktree 清理失败:${String(e)}`)
      );
    }
    await sessionDispose(id).catch(() => {}); // stop 后已终态;失败不阻本地收尾
    finish();
  }, [confirmClose, alsoRemoveWt, closeTab]);

  const sessionIds = Object.keys(sessions);

  return (
    <main className="flex h-screen flex-col">
      <header className="flex items-center gap-3 border-b px-4 py-2">
        <strong>ItsukaNexus</strong>
        {info && (
          <span className="text-[13px] text-muted-foreground">
            v{info.version} · {info.platform}
          </span>
        )}
        <span className="flex-1" />
        {gitInfo && !gitInfo.available && (
          <span className="rounded-md bg-amber-500/15 px-2.5 py-1 text-xs text-amber-400">
            未检测到 git——worktree 功能不可用(请安装 git ≥ 2.20)
          </span>
        )}
        {gitInfo && gitInfo.available && !gitInfo.worktreeSupported && (
          <span className="rounded-md bg-amber-500/15 px-2.5 py-1 text-xs text-amber-400">
            git {gitInfo.version ?? ""} 版本过低——worktree 功能需要 git ≥ 2.20,请升级
          </span>
        )}
        {gitInfo && gitInfo.available && gitInfo.worktreeSupported && (
          <span className="text-xs text-muted-foreground">
            git {gitInfo.version ?? "可用"}
          </span>
        )}
        <Button size="sm" onClick={handleNew}>
          新建会话
        </Button>
      </header>

      <TerminalTabs
        sessions={sessions}
        activeId={activeId}
        onSelect={setActive}
        onClose={handleClose}
      />

      <div className="min-h-0 flex-1 p-1">
        {sessionIds.map((id) => (
          <div
            key={id}
            className={id === activeId ? "block h-full w-full" : "hidden"}
          >
            <TerminalPane sessionId={id} onFitted={handleFitted} />
          </div>
        ))}
        {sessionIds.length === 0 && (
          <div className="flex h-full items-center justify-center text-sm text-muted-foreground">
            暂无会话——点右上角「新建会话」开始
          </div>
        )}
      </div>

      {error && (
        <footer className="bg-destructive/10 px-4 py-1 text-sm text-red-400">
          {error}
        </footer>
      )}

      <NewSessionDialog
        open={newDialogOpen}
        onOpenChange={setNewDialogOpen}
        onError={setError}
      />

      <AlertDialog
        open={confirmClose !== null}
        onOpenChange={(v) => {
          if (!v) setConfirmClose(null);
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
    </main>
  );
}

export default App;
