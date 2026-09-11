//! 誰がこの操作をしているかの判定。エージェント検出は std-env / @vercel/detect-agent の規約に従う。

use std::env;
use std::io;
use std::process::Command;

/// 表示用のラベル。
pub fn actor() -> String {
    if let Ok(v) = env::var("AXON_ACTOR")
        && !v.is_empty()
    {
        return v;
    }
    // サポート対象は簡潔な名前を出す。AI_AGENT の値は
    // "claude-code_2-1-251_agent" のように冗長なことがあるため、個別検出を先に見る。
    if env::var_os("CLAUDECODE").is_some() || env::var_os("CLAUDE_CODE").is_some() {
        return "claude-code".to_string();
    }
    if env::var_os("CODEX_SANDBOX").is_some() || env::var_os("CODEX_THREAD_ID").is_some() {
        return "codex".to_string();
    }
    // サポート外のエージェントは自己申告をそのまま使う
    if let Ok(v) = env::var("AI_AGENT")
        && !v.is_empty()
    {
        return v;
    }
    let user = env::var("USER").unwrap_or_else(|_| "unknown".to_string());
    match env::current_dir()
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
    {
        Some(dir) => format!("{user}@{dir}"),
        None => user,
    }
}

pub fn worktree() -> io::Result<String> {
    let current_dir = env::current_dir()?;
    if let Ok(output) = Command::new("git")
        .current_dir(&current_dir)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        && output.status.success()
    {
        let root = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !root.is_empty() {
            return Ok(root);
        }
    }
    Ok(current_dir.to_string_lossy().into_owned())
}
