// 应用元信息领域模块：纯逻辑，不依赖 Tauri 运行时状态，
// 因此可以在不启动整个应用的情况下做集成测试。
// 未来命令变多时，演进为 commands/ 目录下的一个文件。
use serde::Serialize;

/// 应用元信息（M0 第一个跨 IPC 结构体）。
/// name/version 来自编译期 env 宏（Cargo.toml），platform 来自目标 OS 常量。
///
/// 学习点：`env!("CARGO_PKG_*")` 是编译期宏——cargo 编译时把 Cargo.toml 的
/// 值直接织入代码，等同于写字面量，零运行时开销且不可被环境变量篡改；
/// 而 `std::env::consts::OS` 是目标平台常量（编译时确定，但属于 std 运行库）。
/// 运行期的 `std::env::var()` 才是读进程环境变量的那个，注意区分。
///
/// `#[derive(Serialize)]` 让此结构体能被序列化为 JSON——Tauri 命令返回值
/// 跨 IPC 传给前端时正是走 serde 序列化。rename_all = "camelCase" 是为
/// 未来出现多词字段时与前端 TS 命名习惯对齐（当前三个字段都是单词，无影响）。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub name: String,
    pub version: String,
    pub platform: String,
}

/// 返回应用元信息。纯函数，无副作用。
pub fn app_info() -> AppInfo {
    AppInfo {
        name: env!("CARGO_PKG_NAME").to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        platform: std::env::consts::OS.to_string(),
    }
}
