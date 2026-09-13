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

    /// 入册:git 校验(仅 git 仓库,spec v1.2)→ 以 RepoInfo.root(canonical)
    /// 归一路径 → 追加 repo 本地 exclude(M3 必办#3)→ 持久化。
    /// 同一路径重复入册返回既有条目(幂等)。
    pub async fn add(&self, path: &str) -> Result<ProjectEntry, NexusError> {
        let info = self.ops.validate_repo(std::path::Path::new(path)).await?;
        let root = info.root;
        {
            let mut guard = self.entries.lock().expect("注册表锁被毒化");
            if let Some(hit) = guard.iter().find(|e| e.path == root) {
                return Ok(hit.clone());
            }
            let entry = ProjectEntry {
                id: ProjectId::new(),
                name: root
                    .file_name()
                    .map(|f| f.to_string_lossy().into_owned())
                    .unwrap_or_else(|| root.to_string_lossy().into_owned()),
                path: root.clone(),
                added_at_ms: now_ms(),
            };
            guard.push(entry.clone());
            guard.sort_by_key(|e| e.added_at_ms);
            if let Err(e) = store::save(&self.dir, &guard) {
                // 写盘失败回滚内存表:表与磁盘不分离
                guard.retain(|e| e.id != entry.id);
                return Err(e);
            }
            drop(guard);
            Self::append_local_exclude(&root);
            Ok(entry)
        }
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
