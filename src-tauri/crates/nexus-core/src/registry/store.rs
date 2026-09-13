// projects.json 磁盘读写:单条容错(坏一条丢一条)+ tmp+rename 原子写。
// 容错策略(orca zod-salvage 哲学):整档解析成 serde_json::Value,
// 逐条 from_value——单条字段坏只丢该条;整档非 JSON/缺字段 → 空表。
use std::io::Write;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::ProjectEntry;
use crate::error::NexusError;

const FILE: &str = "projects.json";
const TMP: &str = "projects.json.tmp";

#[derive(Serialize, Deserialize)]
struct Schema {
    schema_version: u32,
    projects: Vec<ProjectEntry>,
}

pub fn load(dir: &Path) -> Vec<ProjectEntry> {
    let bytes = match std::fs::read(dir.join(FILE)) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
        Err(e) => {
            log::warn!("projects.json 读取失败({e}),按空表启动");
            return Vec::new();
        }
    };
    let root: serde_json::Value = match serde_json::from_slice(&bytes) {
        Ok(v) => v,
        Err(e) => {
            log::warn!("projects.json 非 JSON({e}),按空表启动(文件保留现场)");
            return Vec::new();
        }
    };
    let Some(raws) = root.get("projects").and_then(|v| v.as_array()) else {
        return Vec::new();
    };
    raws.iter()
        .filter_map(
            |r| match serde_json::from_value::<ProjectEntry>(r.clone()) {
                Ok(e) => Some(e),
                Err(e) => {
                    log::warn!("一条项目记录损坏,丢弃: {e}");
                    None
                }
            },
        )
        .collect()
}

pub fn save(dir: &Path, entries: &[ProjectEntry]) -> Result<(), NexusError> {
    let schema = Schema {
        schema_version: 1,
        projects: entries.to_vec(),
    };
    std::fs::create_dir_all(dir)?;
    let json = serde_json::to_vec_pretty(&schema)
        .map_err(|e| NexusError::Pty(std::io::Error::other(format!("注册表序列化失败: {e}"))))?;
    let tmp = dir.join(TMP);
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(&json)?;
        f.flush()?;
    }
    std::fs::rename(&tmp, dir.join(FILE))?;
    Ok(())
}
