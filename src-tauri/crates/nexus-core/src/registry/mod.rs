// 项目注册表(spec §1.3):多项目目录的持久化与入册校验。
// 存储在 store.rs;本文件是领域服务:git 校验 + exclude 追加 + 内存表。
pub mod store;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use crate::error::NexusError;
use crate::gitx::ops::GitOps;
use crate::ids::ProjectId;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectEntry {
    pub id: ProjectId,
    pub name: String,
    pub path: PathBuf,
    /// 所属工作区(添加时选择的父目录,canonical)。
    /// legacy 档缺此字段:serde default 空路径,store::load 回填 = path(零损迁移)。
    #[serde(default)]
    pub workspace: PathBuf,
    pub added_at_ms: u64,
}

pub struct ProjectRegistry {
    dir: PathBuf,
    ops: Arc<dyn GitOps>,
    entries: Mutex<Vec<ProjectEntry>>,
}

impl ProjectRegistry {
    pub fn new(dir: PathBuf, ops: Arc<dyn GitOps>) -> Self {
        let entries = store::load(&dir);
        Self {
            dir,
            ops,
            entries: Mutex::new(entries),
        }
    }

    /// 入册工作区(用户验收反馈:项目工作区层级):
    /// D 本身是 git 仓库 → D 也作为项目行;D 的每个**直接子目录**是 git 仓库
    /// → 各作为一个项目行(子目录按名称排序;入册 path 用 RepoInfo.root 归一)。
    /// 候选全空 → InvalidInput。同一路径已在册 → 仅把其 workspace 重挂为 D
    /// (复用条目,幂等);新 repo → 新条目。每 repo 追加本地 exclude(best-effort),
    /// 一次落盘;失败回滚内存表。返回该工作区全部条目(按 path 排序)。
    pub async fn add(&self, path: &str) -> Result<Vec<ProjectEntry>, NexusError> {
        let dir = std::path::Path::new(path);
        let workspace = std::fs::canonicalize(dir)
            .map_err(|_| NexusError::InvalidInput(format!("目录不存在: {path}")))?;
        // 候选 repo 根:D 自身 + 直接子目录(只看一层;普通子目录 validate_repo 失败即跳过)
        let mut roots: Vec<PathBuf> = Vec::new();
        if let Ok(info) = self.ops.validate_repo(&workspace).await {
            roots.push(info.root);
        }
        let mut subdirs: Vec<PathBuf> = match std::fs::read_dir(&workspace) {
            Ok(rd) => rd
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.is_dir())
                .collect(),
            Err(_) => Vec::new(), // 非目录(D 是文件):只剩 D 自身候选,走下方空判定
        };
        subdirs.sort();
        for sub in subdirs {
            if let Ok(info) = self.ops.validate_repo(&sub).await {
                if !roots.contains(&info.root) {
                    roots.push(info.root);
                }
            }
        }
        if roots.is_empty() {
            return Err(NexusError::InvalidInput(format!(
                "{} 不是 git 仓库,其下也没有 git 子目录",
                workspace.display()
            )));
        }
        let mut now = now_ms();
        let workspace_entries = {
            let mut guard = self.entries.lock().expect("注册表锁被毒化");
            let snapshot = guard.clone();
            for root in &roots {
                if let Some(hit) = guard.iter_mut().find(|e| &e.path == root) {
                    // 已在册(可能挂在别的工作区)→ 重挂工作区到 D,条目/时间戳不变
                    hit.workspace = workspace.clone();
                } else {
                    guard.push(ProjectEntry {
                        id: ProjectId::new(),
                        name: root
                            .file_name()
                            .map(|f| f.to_string_lossy().into_owned())
                            .unwrap_or_else(|| root.to_string_lossy().into_owned()),
                        path: root.clone(),
                        workspace: workspace.clone(),
                        added_at_ms: now,
                    });
                    // 同批多个新条目错开毫秒:added_at_ms 升序即入册序(D 先、子目录按名)
                    now += 1;
                }
            }
            // 稳定序:addedAtMs 升序,同毫秒按 path(同批 D 是子目录的路径前缀,天然在前)
            guard.sort_by(|a, b| (a.added_at_ms, &a.path).cmp(&(b.added_at_ms, &b.path)));
            if let Err(e) = store::save(&self.dir, &guard) {
                // 写盘失败回滚内存表(含重挂):表与磁盘不分离
                *guard = snapshot;
                return Err(e);
            }
            let mut ws: Vec<ProjectEntry> = guard
                .iter()
                .filter(|e| e.workspace == workspace)
                .cloned()
                .collect();
            drop(guard);
            for root in &roots {
                Self::append_local_exclude(root);
            }
            ws.sort_by(|a, b| a.path.cmp(&b.path));
            ws
        };
        Ok(workspace_entries)
    }

    pub fn list(&self) -> Vec<ProjectEntry> {
        self.entries.lock().expect("注册表锁被毒化").clone()
    }

    pub fn remove(&self, id: ProjectId) -> bool {
        let mut guard = self.entries.lock().expect("注册表锁被毒化");
        let before = guard.len();
        guard.retain(|e| e.id != id);
        if guard.len() == before {
            return false;
        }
        if let Err(e) = store::save(&self.dir, &guard) {
            log::warn!("projects.json 写盘失败: {e}");
        }
        true
    }

    /// repo 本地忽略(不进版本库):追加 `.nx-worktrees/`,已有则跳过。
    /// best-effort:失败只 warn,不阻塞入册。
    fn append_local_exclude(root: &std::path::Path) {
        let info = root.join(".git").join("info");
        let file = info.join("exclude");
        if let Ok(existing) = std::fs::read_to_string(&file) {
            if existing.lines().any(|l| l.trim() == ".nx-worktrees/") {
                return;
            }
        }
        if let Err(e) = std::fs::create_dir_all(&info) {
            log::warn!(".git/info 创建失败({e}),跳过 exclude 追加");
            return;
        }
        let mut next = std::fs::read_to_string(&file).unwrap_or_default();
        if !next.is_empty() && !next.ends_with('\n') {
            next.push('\n');
        }
        next.push_str(".nx-worktrees/\n");
        if let Err(e) = std::fs::write(&file, next) {
            log::warn!("exclude 追加失败({e}),主检出会显示 .nx-worktrees/ 为未跟踪");
        }
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
