//! 誰がこの操作をしているかの判定。エージェント検出は std-env / @vercel/detect-agent の規約に従う。

use std::env;

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

/// claim の一意性を保証する識別子。セッション ID が取れなければ場所と PID から作る。
pub fn session_key() -> String {
    for key in ["CLAUDE_CODE_SESSION_ID", "CODEX_THREAD_ID"] {
        if let Ok(v) = env::var(key)
            && !v.is_empty()
        {
            return v;
        }
    }
    let cwd = env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| "?".to_string());
    format!("{cwd}#{}", std::process::id())
}

pub fn pid() -> i32 {
    std::process::id() as i32
}

/// そのプロセスがまだ生きているか。1 マシン前提なので PID で判定できる。
/// 権限が無い場合 (EPERM) も存在はしているので、生存とみなす。
pub fn process_alive(pid: i32) -> bool {
    if pid <= 0 {
        return false;
    }
    unsafe { libc::kill(pid, 0) == 0 || *libc::__error() == libc::EPERM }
}
