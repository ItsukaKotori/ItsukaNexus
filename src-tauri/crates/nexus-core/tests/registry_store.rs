// registry 域:projects.json 持久化(单条容错/原子写)+ ProjectRegistry 入册流程
// (工作区层级:D 自身或其 git 直接子目录各为一个项目行)。
use std::path::Path;
use std::process::Command;
use std::sync::Arc;

use nexus_core::gitx::cli::GitCliOps;
use nexus_core::registry::store;
use nexus_core::registry::{ProjectEntry, ProjectRegistry};
use nexus_core::NexusError;

fn init_repo_at(dir: &Path) {
    let run = |args: &[&str]| {
        let st = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .status()
            .unwrap();
        assert!(st.success(), "git {args:?} 失败");
    };
    run(&["init", "-q"]);
    run(&["config", "user.email", "test@nx.local"]);
    run(&["config", "user.name", "nx-test"]);
    std::fs::write(dir.join("README.md"), "# t\n").unwrap();
    run(&["add", "."]);
    run(&["commit", "-qm", "init"]);
}

fn entry(n: u32) -> ProjectEntry {
    ProjectEntry {
        id: format!("00000000-0000-0000-0000-00000000000{n}")
            .parse()
            .unwrap(),
        name: format!("p{n}"),
        path: format!("/tmp/p{n}").into(),
        workspace: format!("/tmp/p{n}").into(),
        added_at_ms: n as u64,
    }
}

fn registry() -> ProjectRegistry {
    ProjectRegistry::new(
        tempfile::tempdir().unwrap().path().to_path_buf(),
        Arc::new(GitCliOps::new()),
    )
}

#[test]
fn store_roundtrip_preserves_order() {
    let dir = tempfile::tempdir().unwrap();
    let items = vec![entry(1), entry(2)];
    store::save(dir.path(), &items).unwrap();
    assert_eq!(store::load(dir.path()), items);
}

#[test]
fn load_salvages_per_entry_and_falls_back_on_non_json() {
    let dir = tempfile::tempdir().unwrap();
    // 单条损坏(id 非法):只丢那一条
    std::fs::write(
        dir.path().join("projects.json"),
        r#"{"schemaVersion":1,"projects":[{"id":"not-a-uuid","name":"bad","path":"/b","addedAtMs":1},{"id":"00000000-0000-0000-0000-000000000002","name":"ok","path":"/ok","addedAtMs":2}]}"#,
    )
    .unwrap();
    let loaded = store::load(dir.path());
    assert_eq!(loaded.len(), 1, "坏条目丢弃,好条目保留");
    assert_eq!(loaded[0].name, "ok");
    // 整档非 JSON:回退空表
    std::fs::write(dir.path().join("projects.json"), "{{{").unwrap();
    assert!(store::load(dir.path()).is_empty());
    // 缺 schemaVersion/projects 字段:serde default 兜底为空表
    std::fs::write(dir.path().join("projects.json"), "{}").unwrap();
    assert!(store::load(dir.path()).is_empty());
}

#[tokio::test]
async fn add_validates_git_repo_and_writes_exclude() {
    let dir = tempfile::tempdir().unwrap();
    init_repo_at(dir.path());
    let reg = registry();
    // 非 git 目录(无 git 子目录)拒绝
    let notrepo = tempfile::tempdir().unwrap();
    assert!(reg.add(notrepo.path().to_str().unwrap()).await.is_err());

    // git 仓库入册(自身即工作区):path 归一到 canonical 根;exclude 被追加
    let added = reg.add(dir.path().to_str().unwrap()).await.unwrap();
    assert_eq!(added.len(), 1);
    assert_eq!(added[0].path, dir.path().canonicalize().unwrap());
    assert_eq!(
        added[0].name,
        dir.path().file_name().unwrap().to_string_lossy()
    );
    assert_eq!(added[0].workspace, dir.path().canonicalize().unwrap());
    let exclude = std::fs::read_to_string(dir.path().join(".git/info/exclude")).unwrap();
    assert!(
        exclude.lines().any(|l| l.trim() == ".nx-worktrees/"),
        "exclude 应含 .nx-worktrees/: {exclude}"
    );

    // 重复添加同一路径:返回既有条目,不重复入册、不重复追加 exclude
    let again = reg.add(dir.path().to_str().unwrap()).await.unwrap();
    assert_eq!(again.len(), 1);
    assert_eq!(again[0].id, added[0].id);
    assert_eq!(reg.list().len(), 1);
    let cnt = exclude_lines(&dir);
    assert_eq!(cnt, 1, "exclude 只应有一行 .nx-worktrees/");

    // remove:命中 true,再删 false;条目消失
    assert!(reg.remove(added[0].id));
    assert!(!reg.remove(added[0].id));
    assert!(reg.list().is_empty());
}

/// 工作区层级主用例:非 git 父目录 + 两个 git 子目录 → 各为一个项目行,
/// workspace = 父目录 canonical,两个子目录 exclude 都写入,project_list 有序
#[tokio::test]
async fn add_parent_with_git_subdirs_groups_them_under_workspace() {
    let ws = tempfile::tempdir().unwrap();
    let alpha = ws.path().join("alpha");
    let beta = ws.path().join("beta");
    std::fs::create_dir_all(&alpha).unwrap();
    std::fs::create_dir_all(&beta).unwrap();
    init_repo_at(&alpha);
    init_repo_at(&beta);
    // 普通非 git 子目录应被忽略
    std::fs::create_dir_all(ws.path().join("plain")).unwrap();

    let reg = registry();
    let added = reg.add(ws.path().to_str().unwrap()).await.unwrap();
    let ws_canon = ws.path().canonicalize().unwrap();
    assert_eq!(added.len(), 2, "只有两个 git 子目录入册");
    assert_eq!(added[0].path, alpha.canonicalize().unwrap());
    assert_eq!(added[1].path, beta.canonicalize().unwrap());
    assert_eq!(added[0].name, "alpha");
    assert_eq!(added[0].workspace, ws_canon);
    assert_eq!(added[1].workspace, ws_canon);
    // 每个子目录的 exclude 都写入
    for sub in [&alpha, &beta] {
        let exclude = std::fs::read_to_string(sub.join(".git/info/exclude")).unwrap();
        assert!(
            exclude.lines().any(|l| l.trim() == ".nx-worktrees/"),
            "{sub:?} exclude 应含 .nx-worktrees/"
        );
    }
    // project_list 有序(按加入序 = 子目录名称序)
    let list = reg.list();
    assert_eq!(list.len(), 2);
    assert!(list[0].path < list[1].path);
}

/// git 目录(无 git 子目录)→ 单条,workspace = 自身
#[tokio::test]
async fn add_git_dir_without_git_subdirs_registers_itself() {
    let dir = tempfile::tempdir().unwrap();
    init_repo_at(dir.path());
    std::fs::create_dir_all(dir.path().join("plain")).unwrap();
    let reg = registry();
    let added = reg.add(dir.path().to_str().unwrap()).await.unwrap();
    assert_eq!(added.len(), 1);
    assert_eq!(added[0].path, dir.path().canonicalize().unwrap());
    assert_eq!(added[0].workspace, added[0].path);
    assert_eq!(reg.list().len(), 1);
}

/// 空普通目录(自身非 git、无 git 子目录)→ InvalidInput
#[tokio::test]
async fn add_plain_dir_without_any_repo_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let empty = dir.path().join("empty");
    std::fs::create_dir_all(&empty).unwrap();
    let reg = registry();
    let err = reg
        .add(empty.to_str().unwrap())
        .await
        .expect_err("空普通目录应被拒绝");
    assert!(matches!(err, NexusError::InvalidInput(_)), "实际: {err:?}");
    assert!(reg.list().is_empty());
}

/// legacy JSON 条目缺 workspace 字段 → load 后回填 workspace = path
#[test]
fn legacy_entry_without_workspace_backfills_to_path() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("projects.json"),
        r#"{"schema_version":1,"projects":[{"id":"00000000-0000-0000-0000-000000000003","name":"legacy","path":"/legacy/p","addedAtMs":3}]}"#,
    )
    .unwrap();
    let loaded = store::load(dir.path());
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].workspace, loaded[0].path);
    // 新档带 workspace 字段则原样保留
    let mut kept = entry(4);
    kept.workspace = "/ws".into();
    store::save(dir.path(), &[kept.clone()]).unwrap();
    assert_eq!(store::load(dir.path()), vec![kept]);
}

/// 重复 add 同一 D → 幂等(条目不重复)
#[tokio::test]
async fn readd_same_workspace_is_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    init_repo_at(dir.path());
    std::fs::create_dir_all(dir.path().join("child")).unwrap();
    init_repo_at(&dir.path().join("child"));
    let reg = registry();
    let first = reg.add(dir.path().to_str().unwrap()).await.unwrap();
    assert_eq!(first.len(), 2, "D 自身 + child");
    let second = reg.add(dir.path().to_str().unwrap()).await.unwrap();
    assert_eq!(second.len(), 2);
    assert_eq!(
        second.iter().map(|e| e.id).collect::<Vec<_>>(),
        first.iter().map(|e| e.id).collect::<Vec<_>>(),
        "幂等:复用既有条目"
    );
    assert_eq!(reg.list().len(), 2);
}

/// 已在别的工作区的 repo 再 add 到新 D → workspace 被更新为 D(重挂)
#[tokio::test]
async fn readd_repo_under_new_workspace_remounts_entry() {
    let ws = tempfile::tempdir().unwrap();
    let alpha = ws.path().join("alpha");
    std::fs::create_dir_all(&alpha).unwrap();
    init_repo_at(&alpha);
    let reg = registry();
    // 先单独入册 alpha:workspace = alpha 自身
    let direct = reg.add(alpha.to_str().unwrap()).await.unwrap();
    assert_eq!(direct.len(), 1);
    assert_eq!(direct[0].workspace, alpha.canonicalize().unwrap());
    // 再入册父目录:alpha 重挂到父工作区,条目 id 不变、不重复
    let remounted = reg.add(ws.path().to_str().unwrap()).await.unwrap();
    assert_eq!(remounted.len(), 1);
    assert_eq!(remounted[0].id, direct[0].id);
    assert_eq!(
        remounted[0].workspace,
        ws.path().canonicalize().unwrap(),
        "workspace 应被更新为父目录"
    );
    let list = reg.list();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].workspace, ws.path().canonicalize().unwrap());
}

fn exclude_lines(dir: &tempfile::TempDir) -> usize {
    std::fs::read_to_string(dir.path().join(".git/info/exclude"))
        .unwrap()
        .lines()
        .filter(|l| l.trim() == ".nx-worktrees/")
        .count()
}
