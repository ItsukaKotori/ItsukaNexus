//! config::store 集成测试:默认生成 / save-load 往返 / 损坏自愈。
//! 跨平台无 cfg 门:只用 std::fs + std::env::temp_dir(不引 tempfile 依赖),
//! 每个测试用 uuid 唯一子目录隔离,末尾清理容忍失败。
use std::fs;
use std::path::PathBuf;

use nexus_core::config::model::{AgentProfile, AppConfig};
use nexus_core::config::{default_config, store};

/// 唯一临时目录:temp_dir/itsukanexus-test-<uuid>,已创建。
fn temp_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("itsukanexus-test-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&dir).expect("创建临时目录失败");
    dir
}

/// 空目录 load_or_create:返回默认配置且 config.json 已落盘。
#[test]
fn empty_dir_load_or_create_returns_default_and_writes_file() {
    let dir = temp_dir();
    let cfg = store::load_or_create(&dir);

    assert_eq!(cfg.version, 1);
    assert_eq!(cfg.terminal.font_family, None);
    assert_eq!(cfg.terminal.font_size, 13);
    assert_eq!(cfg.terminal.scrollback, 5000);
    // 内置一条 shell profile,command 取 default_shell 的当前值
    assert_eq!(cfg.agent_profiles.len(), 1);
    let shell = &cfg.agent_profiles[0];
    assert_eq!(shell.id, "shell");
    assert_eq!(shell.display_name, "Shell");
    assert_eq!(shell.command, nexus_core::agent::manager::default_shell());
    assert!(shell.args_template.is_empty());
    assert!(shell.env.is_empty());

    assert!(dir.join("config.json").exists());
    // 与 default_config() 全等
    assert_eq!(cfg, default_config());

    let _ = fs::remove_dir_all(&dir);
}

/// save 后重新 load_or_create:修改过的字段与新增 profile 原样往返。
#[test]
fn save_then_load_round_trip() {
    let dir = temp_dir();
    let mut cfg = store::load_or_create(&dir);
    cfg.terminal.font_size = 15;
    cfg.terminal.font_family = Some("JetBrains Mono".into());
    cfg.agent_profiles.push(AgentProfile {
        id: "claude".into(),
        display_name: "Claude Code".into(),
        command: "claude".into(),
        args_template: vec!["--verbose".into()],
        env: [("API_KEY".to_string(), "secret".to_string())]
            .into_iter()
            .collect(),
    });

    store::save(&dir, &cfg).expect("save 失败");

    let reloaded = store::load_or_create(&dir);
    assert_eq!(reloaded, cfg);

    let _ = fs::remove_dir_all(&dir);
}

/// 损坏自愈:坏 JSON → 返回默认、旧文件改名 .bak、新 config.json 可解析。
#[test]
fn corrupted_config_self_heals_via_bak() {
    let dir = temp_dir();
    fs::write(dir.join("config.json"), "{ not valid json").expect("写坏文件失败");

    let cfg = store::load_or_create(&dir);
    assert_eq!(cfg, default_config());

    assert!(dir.join("config.json.bak").exists());
    let bytes = fs::read(dir.join("config.json")).expect("新 config.json 不存在");
    let parsed: AppConfig = serde_json::from_slice(&bytes).expect("新 config.json 不可解析");
    assert_eq!(parsed, cfg);

    let _ = fs::remove_dir_all(&dir);
}
