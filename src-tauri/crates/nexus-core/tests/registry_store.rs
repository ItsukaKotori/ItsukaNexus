// registry 域:projects.json 持久化(单条容错/原子写)+ ProjectRegistry 入册流程。
use std::path::Path;
use std::process::Command;
use std::sync::Arc;

use nexus_core::gitx::cli::GitCliOps;
use nexus_core::registry::store;
use nexus_core::registry::{ProjectEntry, ProjectRegistry};

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
        added_at_ms: n as u64,
    }
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
    let reg = ProjectRegistry::new(
        tempfile::tempdir().unwrap().path().to_path_buf(),
        Arc::new(GitCliOps::new()),
    );
    // 非 git 目录拒绝(NotARepo 透传)
    let notrepo = tempfile::tempdir().unwrap();
    assert!(reg.add(notrepo.path().to_str().unwrap()).await.is_err());

    // git 仓库入册:path 归一到 canonical 根;exclude 被追加
    let added = reg.add(dir.path().to_str().unwrap()).await.unwrap();
    assert_eq!(added.path, dir.path().canonicalize().unwrap());
    assert_eq!(
        added.name,
        dir.path().file_name().unwrap().to_string_lossy()
    );
    let exclude = std::fs::read_to_string(dir.path().join(".git/info/exclude")).unwrap();
    assert!(
        exclude.lines().any(|l| l.trim() == ".nx-worktrees/"),
        "exclude 应含 .nx-worktrees/: {exclude}"
    );

    // 重复添加同一路径:返回既有条目,不重复入册、不重复追加 exclude
    let again = reg.add(dir.path().to_str().unwrap()).await.unwrap();
    assert_eq!(again.id, added.id);
    assert_eq!(reg.list().len(), 1);
    let cnt = exclude_lines(&dir);
    assert_eq!(cnt, 1, "exclude 只应有一行 .nx-worktrees/");

    // remove:命中 true,再删 false;条目消失
    assert!(reg.remove(added.id));
    assert!(!reg.remove(added.id));
    assert!(reg.list().is_empty());
}

fn exclude_lines(dir: &tempfile::TempDir) -> usize {
    std::fs::read_to_string(dir.path().join(".git/info/exclude"))
        .unwrap()
        .lines()
        .filter(|l| l.trim() == ".nx-worktrees/")
        .count()
}
