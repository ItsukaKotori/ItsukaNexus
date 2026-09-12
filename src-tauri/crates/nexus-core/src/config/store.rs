// 磁盘读写:`config.json` 的人读编辑 + 损坏自愈 + 原子写。
// 原子写 = 同目录 `config.json.tmp` 写入+flush 后 rename 覆盖:任何时刻磁盘上
// 都有一份完整配置,写一半崩溃也不会损坏旧文件。
// 错误通道:NexusError 目前(M1 形态)只有 io 来源的 Pty 变体可用——配置 IO
// 错误借道 `NexusError::Pty(io::Error)` 上抛,变体分层(spec 的 Config 变体)
// 留给 M4,避免本任务扩散改动 error.rs。
use std::io::Write;
use std::path::Path;

use crate::config::model::AppConfig;
use crate::error::NexusError;

const CONFIG_FILE: &str = "config.json";
const CONFIG_BAK: &str = "config.json.bak";
const CONFIG_TMP: &str = "config.json.tmp";

/// 读配置:文件不存在 → 写默认并返回;解析失败 → 旧文件改名 `.bak` 后写默认
/// (防损坏循环:坏文件不删,留档可查);其余读错误 → 照常写默认。
/// 任何写盘失败都不阻塞返回(返回内存默认值,下次启动再试)。
pub fn load_or_create(dir: &Path) -> AppConfig {
    let path = dir.join(CONFIG_FILE);
    match std::fs::read(&path) {
        Ok(bytes) => match serde_json::from_slice::<AppConfig>(&bytes) {
            Ok(cfg) => return cfg,
            Err(e) => {
                log::warn!("配置解析失败({e}),备份为 {CONFIG_BAK} 并重写默认");
                if let Err(e) = std::fs::rename(&path, dir.join(CONFIG_BAK)) {
                    log::warn!("配置备份失败: {e}");
                }
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => log::warn!("配置读取失败({e}),使用默认配置"),
    }
    let cfg = AppConfig::default();
    if let Err(e) = save(dir, &cfg) {
        log::warn!("默认配置写盘失败: {e}");
    }
    cfg
}

/// 写配置:serde_json pretty + tmp+rename 原子覆盖。
pub fn save(dir: &Path, cfg: &AppConfig) -> Result<(), NexusError> {
    std::fs::create_dir_all(dir)?;
    let json = serde_json::to_vec_pretty(cfg)
        .map_err(|e| std::io::Error::other(format!("配置序列化失败: {e}")))?;
    let tmp = dir.join(CONFIG_TMP);
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(&json)?;
        f.flush()?;
    } // 文件在此关闭,rename 前 drop 句柄
    std::fs::rename(&tmp, dir.join(CONFIG_FILE))?;
    Ok(())
}
