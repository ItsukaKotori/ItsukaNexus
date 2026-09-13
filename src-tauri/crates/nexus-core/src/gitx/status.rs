// porcelain v2 -z 解析(spec §1.3):机器格式,变更记录以 NUL 分隔、rename 是
// path\0origPath\0 两段。头记录的分隔随 git 版本有二态(实测 2.50.1:全记录
// NUL 结尾;旧版:头记录 \n 结尾)——两态都认。只认行首关键词,未知记录跳过。
use std::path::PathBuf;

use super::ops::{FileStatus, GitStatus, GitStatusEntry};

/// XY 单字符 → FileStatus;'.' → None;未知字符 → None(git 新增状态码不炸)
fn map_char(c: char) -> Option<FileStatus> {
    match c {
        'M' => Some(FileStatus::Modified),
        'A' => Some(FileStatus::Added),
        'D' => Some(FileStatus::Deleted),
        'R' => Some(FileStatus::Renamed),
        'C' => Some(FileStatus::Copied),
        'U' => Some(FileStatus::Unmerged),
        '.' => None,
        other => {
            log::debug!("porcelain v2 未知状态码 {other:?},按无变化处理");
            None
        }
    }
}

/// 解析 `git status --porcelain=v2 --branch -z` 输出。cap 为条目上限,
/// 超过即停并置 truncated=true。
pub fn parse_status_porcelain_v2(out: &str, cap: usize) -> GitStatus {
    let mut status = GitStatus::default();
    let mut expect_orig: Option<GitStatusEntry> = None; // rename 等待 origPath 段

    // -z 语义:首个 NUL 段 = 全部头行 + 第一条变更;其后每段一条变更
    // (rename 的 origPath 独占一段)
    let mut segments = out.split('\0');
    let first = segments.next().unwrap_or("");
    let mut first_record: Option<String> = None;
    for line in first.lines() {
        if let Some(rest) = line.strip_prefix("# ") {
            parse_header(rest, &mut status);
        } else if !line.is_empty() {
            first_record = Some(line.to_string());
        }
    }
    let mut pending: Vec<String> = first_record.into_iter().collect();
    pending.extend(segments.filter(|s| !s.is_empty()).map(str::to_string));

    for line in pending {
        // 上一条是 rename:本段是它的 origPath
        if let Some(mut entry) = expect_orig.take() {
            entry.orig_path = Some(PathBuf::from(&line));
            push_entry(&mut status, entry, cap);
            continue;
        }
        // 头记录也可能 NUL 分隔(实测 git 2.50.1 -z),不能只在首段认
        if let Some(rest) = line.strip_prefix("# ") {
            parse_header(rest, &mut status);
        } else if let Some(path) = line.strip_prefix("? ") {
            push_entry(
                &mut status,
                GitStatusEntry {
                    path: PathBuf::from(path),
                    index: None,
                    worktree: Some(FileStatus::Untracked),
                    orig_path: None,
                },
                cap,
            );
        } else if let Some(rest) = line.strip_prefix("1 ") {
            // 1 <XY> <sub> <mH> <mI> <mW> <hH> <hI> <path>:path 可含空格,splitn 第 8 段起全归路径
            if let Some(entry) = parse_xy_path(rest, 8) {
                push_entry(&mut status, entry, cap);
            }
        } else if let Some(rest) = line.strip_prefix("2 ") {
            // 2 ... <path>\0<origPath>:origPath 在下一段
            if let Some(mut entry) = parse_xy_path(rest, 9) {
                entry.orig_path = Some(PathBuf::new()); // 占位,下一段覆盖
                expect_orig = Some(entry);
            }
        } else if line.starts_with("u ") {
            // 冲突:双侧 Unmerged;path 在第 11 段
            if let Some(path) = line.splitn(11, ' ').nth(10) {
                push_entry(
                    &mut status,
                    GitStatusEntry {
                        path: PathBuf::from(path),
                        index: Some(FileStatus::Unmerged),
                        worktree: Some(FileStatus::Unmerged),
                        orig_path: None,
                    },
                    cap,
                );
            }
        }
        // '#' 以外的未知行:跳过(git 新版本加字段不炸)
    }
    // 截断场景:expect_orig 悬空无害(最后一条 rename 已截断)
    status
}

/// 头行:`branch.head main` / `branch.ab +2 -1` / 其余忽略
fn parse_header(rest: &str, status: &mut GitStatus) {
    if let Some(h) = rest.strip_prefix("branch.head ") {
        let h = h.trim();
        status.branch = if h == "(detached)" {
            None
        } else {
            Some(h.to_string())
        };
    } else if let Some(ab) = rest.strip_prefix("branch.ab ") {
        let mut it = ab.split_whitespace();
        if let Some(a) = it.next() {
            status.ahead = a.trim_start_matches('+').parse().unwrap_or(0);
        }
        if let Some(b) = it.next() {
            status.behind = b.trim_start_matches('-').parse().unwrap_or(0);
        }
    }
}

/// `1`/`2` 记录公共段:前两字符 XY,路径在 splitn(fields) 段的最后
fn parse_xy_path(rest: &str, fields: usize) -> Option<GitStatusEntry> {
    let mut parts = rest.splitn(fields, ' ');
    let xy = parts.next()?;
    let mut chars = xy.chars();
    let x = chars.next()?;
    let y = chars.next()?;
    let path = parts.last()?.trim_end();
    Some(GitStatusEntry {
        path: PathBuf::from(path),
        index: map_char(x),
        worktree: map_char(y),
        orig_path: None,
    })
}

fn push_entry(status: &mut GitStatus, entry: GitStatusEntry, cap: usize) {
    if status.entries.len() >= cap {
        status.truncated = true;
        return;
    }
    status.entries.push(entry);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gitx::ops::FileStatus;

    /// -z 快照:头行们 + 修改 + 未跟踪 + rename(注意头行后紧跟第一条记录,NUL 分隔;
    /// rename 的 XY 用 '.' 表无变化、并带 R100 评分段——实测 git 2.50.1 真实形态)
    #[test]
    fn parses_z_stream_with_headers_and_rename() {
        let out = "# branch.oid 6cf78bbf16892e48ecef12a92d66d98c7c938453\n\
                   # branch.head main\n\
                   # branch.upstream origin/main\n\
                   # branch.ab +2 -1\n\
                   1 .M N... 100644 100644 100644 6cf78 6cf78 a.txt\0\
                   ? 新 未跟踪.txt\0\
                   2 R. N... 100644 100644 100644 6cf78 11111 R100 b-renamed.txt\0b.txt\0";
        let st = parse_status_porcelain_v2(out, 100);
        assert_eq!(st.branch.as_deref(), Some("main"));
        assert_eq!((st.ahead, st.behind), (2, 1));
        assert_eq!(st.entries.len(), 3);
        assert_eq!(st.entries[0].worktree, Some(FileStatus::Modified));
        assert_eq!(st.entries[0].index, None);
        assert_eq!(st.entries[1].worktree, Some(FileStatus::Untracked));
        assert_eq!(st.entries[2].index, Some(FileStatus::Renamed));
        assert_eq!(
            st.entries[2].orig_path.as_deref(),
            Some(std::path::Path::new("b.txt"))
        );
        assert!(!st.truncated);
    }

    /// 实测 git 2.50.1 -z:头记录也是 NUL 结尾(并非 \n)——branch/ahead/behind 仍须解析到
    #[test]
    fn parses_nul_terminated_headers_modern_git() {
        let out = "# branch.oid 682966763463b2cce67f1b6f40ad80b58213dd08\0\
                   # branch.head main\0\
                   1 .M N... 100644 100644 100644 7898 7898 a.txt\0\
                   2 R. N... 100644 100644 100644 6178 6178 R100 b-renamed.txt\0b.txt\0\
                   ? 新 文件.txt\0";
        let st = parse_status_porcelain_v2(out, 100);
        assert_eq!(st.branch.as_deref(), Some("main"));
        assert_eq!(st.entries.len(), 3);
        assert_eq!(st.entries[0].worktree, Some(FileStatus::Modified));
        assert_eq!(st.entries[1].index, Some(FileStatus::Renamed));
        assert_eq!(
            st.entries[1].orig_path.as_deref(),
            Some(std::path::Path::new("b.txt"))
        );
        assert_eq!(st.entries[2].worktree, Some(FileStatus::Untracked));
    }

    #[test]
    fn cap_truncates_and_flags() {
        let out = "1 .M N... 100644 100644 100644 6cf78 6cf78 a\x001 .M N... 100644 100644 100644 6cf78 6cf78 b\0";
        let st = parse_status_porcelain_v2(out, 1);
        assert_eq!(st.entries.len(), 1);
        assert!(st.truncated);
    }

    #[test]
    fn detached_branch_is_none_and_unknown_records_skipped() {
        let out = "# branch.head (detached)\n!ignored-record\0";
        let st = parse_status_porcelain_v2(out, 100);
        assert!(st.branch.is_none());
        assert!(st.entries.is_empty());
    }
}
