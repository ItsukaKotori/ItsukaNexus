// 配置模型:全部 camelCase 序列化(前端契约),字段与 spec M2 Task 7 一致。
// Default 不是全零:AppConfig 带 version=1 + shell profile,TerminalConfig
// 带 13pt/5000 行——"缺省即可用"。
use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::agent::manager::default_shell;

/// 应用配置根。`version` 预留给后续 schema 迁移。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppConfig {
    pub version: u32,
    pub terminal: TerminalConfig,
    pub agent_profiles: Vec<AgentProfile>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            version: 1,
            terminal: TerminalConfig::default(),
            agent_profiles: vec![AgentProfile::default_shell_profile()],
        }
    }
}

/// 终端外观与行为(xterm.js 侧消费)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalConfig {
    /// None = 前端用内置默认字体栈
    pub font_family: Option<String>,
    pub font_size: u16,
    /// 每会话回滚行数
    pub scrollback: u32,
}

impl Default for TerminalConfig {
    fn default() -> Self {
        Self {
            font_family: None,
            font_size: 13,
            scrollback: 5000,
        }
    }
}

/// Agent 启动档案:M2 只建模,不驱动 spawn(M4 接管,届时替换 default_shell)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AgentProfile {
    pub id: String,
    pub display_name: String,
    pub command: String,
    /// 占位符模板(M4 定义替换规则),M2 原样存储
    pub args_template: Vec<String>,
    pub env: HashMap<String, String>,
}

impl AgentProfile {
    /// 默认配置内置的那条:command 取 default_shell 的当前值(本机 $SHELL)。
    fn default_shell_profile() -> Self {
        Self {
            id: "shell".into(),
            display_name: "Shell".into(),
            command: default_shell(),
            args_template: Vec::new(),
            env: HashMap::new(),
        }
    }
}
