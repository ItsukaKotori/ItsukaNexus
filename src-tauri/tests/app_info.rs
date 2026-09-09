// 集成测试：位于 tests/ 目录，编译成独立的测试 crate，
// 只能像外部使用者一样通过 lib 名导入公开 API（itsukanexus_lib）。
// 区别于模块内部的 #[cfg(test)] 单元测试——后者可以测私有函数。
use itsukanexus_lib::app::{app_info, AppInfo};

#[test]
fn app_info_returns_name_version_platform() {
    let info: AppInfo = app_info();
    assert_eq!(info.name, "itsukanexus");
    assert!(!info.version.is_empty());
    // 白名单断言而非绑定单一平台：测试需在三大桌面平台上都可通过
    assert!(["windows", "macos", "linux"].contains(&info.platform.as_str()));
}
