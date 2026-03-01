use anyhow::{Context, Result, bail};
use chrono::{Datelike, Duration, Local, NaiveDate, NaiveDateTime, NaiveTime};
use clap::{Parser, Subcommand};
use globset::{Glob, GlobSetBuilder};
use path_clean::PathClean;
use rusqlite::{Connection, params, params_from_iter};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, Stdio};
use std::time::UNIX_EPOCH;
use walkdir::WalkDir;

const TEMPLATE_IDENTITY: &str = include_str!("templates/agent/IDENTITY.md");
const TEMPLATE_SOUL: &str = include_str!("templates/agent/SOUL.md");
const TEMPLATE_OWNER_PROFILE: &str = include_str!("templates/owner/profile.md");
const TEMPLATE_OWNER_PERSONALITY: &str = include_str!("templates/owner/personality.md");
const TEMPLATE_OWNER_PREFERENCES: &str = include_str!("templates/owner/preferences.md");
const TEMPLATE_OWNER_INTERESTS: &str = include_str!("templates/owner/interests.md");

#[derive(Debug, Parser)]
#[command(
    name = "amem",
    version,
    about = "Local memory CLI for assistant workflows"
)]
pub struct Cli {
    #[arg(long, global = true)]
    memory_dir: Option<PathBuf>,
    #[arg(long, global = true, default_value_t = false)]
    json: bool,
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    Init,
    Search {
        query: String,
        #[arg(short = 'k', long, default_value_t = 8)]
        top_k: usize,
        #[arg(long, default_value_t = false)]
        lexical_only: bool,
        #[arg(long, default_value_t = false)]
        semantic_only: bool,
    },
    Remember {
        #[arg(long)]
        query: Option<String>,
    },
    #[command(visible_alias = "ls")]
    List {
        #[arg(long)]
        path: Option<String>,
        #[arg(long)]
        kind: Option<String>,
        #[arg(long)]
        date: Option<String>,
        #[arg(long)]
        limit: Option<usize>,
    },
    Today {
        #[arg(long)]
        date: Option<String>,
    },
    Keep {
        text: String,
        #[arg(long, default_value = "activity")]
        kind: String,
        #[arg(long)]
        date: Option<String>,
        #[arg(long, default_value = "manual")]
        source: String,
    },
    Which,
    Index {
        #[arg(long, default_value_t = false)]
        rebuild: bool,
    },
    Watch,
    Capture {
        #[arg(long)]
        kind: String,
        #[arg(long)]
        text: String,
        #[arg(long)]
        date: Option<String>,
        #[arg(long, default_value = "manual")]
        source: String,
    },
    Context {
        #[arg(long)]
        task: String,
        #[arg(long)]
        date: Option<String>,
    },
    Get {
        #[command(subcommand)]
        target: GetTarget,
    },
    Set {
        #[command(subcommand)]
        target: SetTarget,
    },
    Triage {
        #[command(subcommand)]
        target: TriageTarget,
    },
    Owner {
        target: Option<String>,
    },
    Agent {
        target: Option<String>,
    },
    Codex {
        #[arg(long, default_value_t = false)]
        resume_only: bool,
        #[arg(long)]
        prompt: Option<String>,
    },
    Gemini {
        #[arg(long, default_value_t = false)]
        resume_only: bool,
        #[arg(long)]
        prompt: Option<String>,
    },
    Claude {
        #[arg(long, default_value_t = false)]
        resume_only: bool,
        #[arg(long)]
        prompt: Option<String>,
    },
    Copilot {
        #[arg(long, default_value_t = false)]
        resume_only: bool,
        #[arg(long)]
        prompt: Option<String>,
    },
    Opencode {
        #[arg(long, default_value_t = false)]
        resume_only: bool,
        #[arg(long)]
        prompt: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
pub enum GetTarget {
    Owner {
        target: Option<String>,
    },
    Agent {
        target: Option<String>,
    },
    #[command(visible_alias = "diaries")]
    Diary {
        period: Option<String>,
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long, default_value_t = false)]
        detail: bool,
        #[arg(long, default_value_t = false)]
        all: bool,
    },
    #[command(visible_alias = "activity", visible_alias = "activities")]
    Acts {
        period: Option<String>,
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long, default_value_t = false)]
        detail: bool,
        #[arg(long, default_value_t = false)]
        all: bool,
    },
    #[command(visible_alias = "task", visible_alias = "todo")]
    Tasks {
        period: Option<String>,
        #[arg(long)]
        limit: Option<usize>,
    },
}

#[derive(Debug, Subcommand)]
pub enum SetTarget {
    Diary {
        text: String,
        #[arg(long)]
        date: Option<String>,
        #[arg(long)]
        time: Option<String>,
    },
    Owner {
        target: Option<String>,
        #[arg(value_name = "VALUE", trailing_var_arg = true)]
        value: Vec<String>,
    },
    #[command(visible_alias = "activity", visible_alias = "activities")]
    Acts {
        #[arg(value_name = "TEXT", required = true, num_args = 1.., trailing_var_arg = true)]
        text: Vec<String>,
        #[arg(long)]
        date: Option<String>,
        #[arg(long, default_value = "manual")]
        source: String,
    },
    #[command(visible_alias = "task", visible_alias = "todo")]
    Tasks {
        #[arg(value_name = "ARG", required = true, num_args = 1.., trailing_var_arg = true)]
        args: Vec<String>,
    },
    Memory {
        text: String,
        #[arg(long)]
        filename: String,
        #[arg(long, default_value = "P3")]
        priority: String,
    },
}

#[derive(Debug, Subcommand)]
pub enum TriageTarget {
    Memory { filename: String, priority: String },
}

#[derive(Debug, Serialize)]
struct SearchHit {
    path: String,
    score: f64,
    snippet: String,
}

#[derive(Debug, Serialize)]
struct TodayJson {
    date: String,
    agent_identity: String,
    agent_identity_path: String,
    agent_soul: String,
    agent_soul_path: String,
    owner_profile: String,
    owner_profile_path: String,
    owner_preferences: String,
    owner_preferences_path: String,
    owner_diary: String,
    owner_diary_path: String,
    owner_diary_paths: Vec<String>,
    owner_diary_recent: Vec<RecentDailySection>,
    open_tasks: String,
    open_tasks_paths: Vec<String>,
    activity: String,
    activity_paths: Vec<String>,
    activity_recent: Vec<RecentDailySection>,
    agent_memories: String,
    agent_memories_paths: Vec<String>,
}

#[derive(Debug, Serialize)]
struct RecentDailySection {
    date: String,
    paths: Vec<String>,
    content: String,
}

#[derive(Debug, Serialize)]
struct KeepJson {
    path: String,
    source: String,
}

#[derive(Debug, Serialize)]
struct InitJson {
    memory_dir: String,
    created: Vec<String>,
}

pub fn run_cli() -> Result<()> {
    let cli = Cli::parse();
    run_with(
        cli,
        &std::env::current_dir().context("failed to resolve current directory")?,
    )
}

fn run_with(cli: Cli, cwd: &Path) -> Result<()> {
    let memory_dir = resolve_memory_dir(cwd, cli.memory_dir);
    match cli.command {
        None => cmd_today(&memory_dir, None, cli.json),
        Some(Commands::Init) => cmd_init(&memory_dir, cli.json),
        Some(Commands::Search {
            query,
            top_k,
            lexical_only,
            semantic_only,
        }) => cmd_search(
            &memory_dir,
            &query,
            top_k,
            lexical_only,
            semantic_only,
            cli.json,
        ),
        Some(Commands::Remember { query }) => cmd_remember(&memory_dir, query, cli.json),
        Some(Commands::List {
            path,
            kind,
            date,
            limit,
        }) => cmd_list(&memory_dir, path, kind, date, limit, cli.json),
        Some(Commands::Today { date }) => cmd_today(&memory_dir, date, cli.json),
        Some(Commands::Keep {
            text,
            kind,
            date,
            source,
        }) => cmd_keep(&memory_dir, &text, &kind, date, &source, cli.json),
        Some(Commands::Which) => cmd_which(&memory_dir, cli.json),
        Some(Commands::Index { rebuild }) => cmd_index(&memory_dir, rebuild, cli.json),
        Some(Commands::Watch) => cmd_watch(&memory_dir),
        Some(Commands::Capture {
            kind,
            text,
            date,
            source,
        }) => cmd_keep(&memory_dir, &text, &kind, date, &source, cli.json),
        Some(Commands::Context { task, date }) => cmd_context(&memory_dir, &task, date, cli.json),
        Some(Commands::Get { target }) => cmd_get(&memory_dir, target, cli.json),
        Some(Commands::Set { target }) => cmd_set(&memory_dir, target, cli.json),
        Some(Commands::Triage { target }) => cmd_triage(&memory_dir, target, cli.json),
        Some(Commands::Owner { target }) => cmd_get_owner(&memory_dir, target, cli.json),
        Some(Commands::Agent { target }) => cmd_get_agent(&memory_dir, target, cli.json),
        Some(Commands::Codex {
            resume_only,
            prompt,
        }) => cmd_codex(&memory_dir, cwd, resume_only, prompt),
        Some(Commands::Gemini {
            resume_only,
            prompt,
        }) => cmd_gemini(&memory_dir, cwd, resume_only, prompt),
        Some(Commands::Claude {
            resume_only,
            prompt,
        }) => cmd_claude(&memory_dir, cwd, resume_only, prompt),
        Some(Commands::Copilot {
            resume_only,
            prompt,
        }) => cmd_copilot(&memory_dir, cwd, resume_only, prompt),
        Some(Commands::Opencode {
            resume_only,
            prompt,
        }) => cmd_opencode(&memory_dir, cwd, resume_only, prompt),
    }
}

fn resolve_memory_dir(cwd: &Path, input: Option<PathBuf>) -> PathBuf {
    let base = input
        .or_else(|| std::env::var_os("AMEM_DIR").map(PathBuf::from))
        .unwrap_or_else(default_memory_dir);
    let path = if base.is_absolute() {
        base
    } else {
        cwd.join(base)
    };
    PathBuf::from(path.clean())
}

fn default_memory_dir() -> PathBuf {
    if let Some(root) = std::env::var_os("AMEM_ROOT").filter(|v| !v.is_empty()) {
        return PathBuf::from(root);
    }
    home_dir_from_env()
        .map(|home| home.join(".amem"))
        .unwrap_or_else(|| PathBuf::from(".amem"))
}

fn home_dir_from_env() -> Option<PathBuf> {
    if let Some(home) = std::env::var_os("HOME").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(home));
    }

    #[cfg(windows)]
    {
        if let Some(profile) = std::env::var_os("USERPROFILE").filter(|v| !v.is_empty()) {
            return Some(PathBuf::from(profile));
        }
        let drive = std::env::var_os("HOMEDRIVE").filter(|v| !v.is_empty());
        let path = std::env::var_os("HOMEPATH").filter(|v| !v.is_empty());
        if let (Some(drive), Some(path)) = (drive, path) {
            return Some(PathBuf::from(drive).join(path));
        }
    }

    None
}

fn cmd_init(memory_dir: &Path, json: bool) -> Result<()> {
    let created = init_memory_scaffold(memory_dir)?;

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&InitJson {
                memory_dir: memory_dir.to_string_lossy().to_string(),
                created,
            })?
        );
    } else {
        println!("{}", memory_dir.to_string_lossy());
    }
    Ok(())
}

fn init_memory_scaffold(memory_dir: &Path) -> Result<Vec<String>> {
    fs::create_dir_all(memory_dir)
        .with_context(|| format!("failed to create {}", memory_dir.to_string_lossy()))?;

    let directories = [
        memory_dir.join("owner"),
        memory_dir.join("owner").join("diary"),
        memory_dir.join("agent"),
        memory_dir.join("agent").join("tasks"),
        memory_dir.join("agent").join("inbox"),
        memory_dir.join("agent").join("activity"),
        memory_dir.join("agent").join("memory"),
        memory_dir.join("agent").join("memory").join("P0"),
        memory_dir.join("agent").join("memory").join("P1"),
        memory_dir.join("agent").join("memory").join("P2"),
        memory_dir.join("agent").join("memory").join("P3"),
    ];
    for dir in directories {
        fs::create_dir_all(&dir)
            .with_context(|| format!("failed to create {}", dir.to_string_lossy()))?;
    }

    let files = [
        (
            memory_dir.join("agent").join("IDENTITY.md"),
            TEMPLATE_IDENTITY,
        ),
        (memory_dir.join("agent").join("SOUL.md"), TEMPLATE_SOUL),
        (
            memory_dir.join("owner").join("profile.md"),
            TEMPLATE_OWNER_PROFILE,
        ),
        (
            memory_dir.join("owner").join("personality.md"),
            TEMPLATE_OWNER_PERSONALITY,
        ),
        (
            memory_dir.join("owner").join("preferences.md"),
            TEMPLATE_OWNER_PREFERENCES,
        ),
        (
            memory_dir.join("owner").join("interests.md"),
            TEMPLATE_OWNER_INTERESTS,
        ),
        (
            memory_dir.join("agent").join("tasks").join("open.md"),
            "# Open Tasks\n\n",
        ),
        (
            memory_dir.join("agent").join("tasks").join("done.md"),
            "# Done Tasks\n\n",
        ),
        (
            memory_dir.join("agent").join("inbox").join("captured.md"),
            "# Captured Notes\n\n",
        ),
    ];

    let mut created = Vec::new();
    for (path, content) in files {
        if !path.exists() {
            fs::write(&path, content)
                .with_context(|| format!("failed to write {}", path.to_string_lossy()))?;
            created.push(rel_or_abs(memory_dir, &path));
        }
    }
    Ok(created)
}

fn cmd_which(memory_dir: &Path, json: bool) -> Result<()> {
    if json {
        println!(
            "{}",
            serde_json::json!({ "memory_dir": memory_dir.to_string_lossy() })
        );
    } else {
        println!("{}", memory_dir.to_string_lossy());
    }
    Ok(())
}

fn cmd_keep(
    memory_dir: &Path,
    text: &str,
    kind: &str,
    date: Option<String>,
    source: &str,
    json: bool,
) -> Result<()> {
    let target_date = parse_or_today(date.as_deref())?;
    let now = Local::now();
    let target = match kind {
        "activity" => {
            let p = activity_path(memory_dir, target_date);
            ensure_parent(&p)?;
            p
        }
        "inbox" => {
            let p = agent_inbox_captured_path(memory_dir);
            ensure_parent(&p)?;
            p
        }
        "task-note" => {
            let p = agent_tasks_open_path(memory_dir);
            ensure_parent(&p)?;
            p
        }
        other => bail!("unsupported kind: {other}"),
    };
    let line = format!("- {} [{}] {}\n", now.format("%H:%M"), source, text.trim());
    if kind == "activity" {
        append_daily_line_with_frontmatter(&target, target_date, line.trim_end())?;
    } else {
        append_markdown_line(&target, line.trim_end())?;
    }

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&KeepJson {
                path: rel_or_abs(memory_dir, &target),
                source: source.to_string(),
            })?
        );
    } else {
        println!("{}", rel_or_abs(memory_dir, &target));
    }
    notify_discord_via_acomm_for_keep(text);
    Ok(())
}

fn notify_discord_via_acomm_for_keep(text: &str) {
    let text = text.trim();
    if text.is_empty() {
        return;
    }

    let Some(discord_bot_token) = resolve_discord_env_value_for_keep("DISCORD_BOT_TOKEN") else {
        return;
    };
    let Some(discord_notify_channel_id) =
        resolve_discord_env_value_for_keep("DISCORD_NOTIFY_CHANNEL_ID")
    else {
        return;
    };

    let mut cmd = ProcessCommand::new("acomm");
    cmd.arg("--discord")
        .arg("--agent")
        .arg(text)
        .env("DISCORD_BOT_TOKEN", discord_bot_token)
        .env("DISCORD_NOTIFY_CHANNEL_ID", discord_notify_channel_id)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    let _ = cmd.spawn();
}

fn resolve_discord_env_value_for_keep(key: &str) -> Option<String> {
    if let Ok(value) = std::env::var(key) {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }

    let env_path = std::env::var_os("HOME")
        .map(PathBuf::from)?
        .join(".config")
        .join("yuiclaw")
        .join(".env");
    read_simple_env_file_value(&env_path, key)
}

fn read_simple_env_file_value(path: &Path, key: &str) -> Option<String> {
    let content = fs::read_to_string(path).ok()?;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let trimmed = trimmed.strip_prefix("export ").unwrap_or(trimmed);
        let (raw_key, raw_value) = trimmed.split_once('=')?;
        if raw_key.trim() != key {
            continue;
        }
        let value = raw_value.trim();
        let value = if value.starts_with('"') && value.ends_with('"') && value.len() >= 2 {
            &value[1..value.len() - 1]
        } else if value.starts_with('\'') && value.ends_with('\'') && value.len() >= 2 {
            &value[1..value.len() - 1]
        } else {
            value
        };
        let value = value.trim();
        if !value.is_empty() {
            return Some(value.to_string());
        }
    }
    None
}

fn cmd_list(
    memory_dir: &Path,
    path: Option<String>,
    kind: Option<String>,
    date: Option<String>,
    limit: Option<usize>,
    json: bool,
) -> Result<()> {
    let mut entries = memory_files(memory_dir)?;
    entries.sort();

    let path_filter = if let Some(pattern) = path {
        let mut builder = GlobSetBuilder::new();
        builder.add(Glob::new(&pattern).with_context(|| format!("invalid glob: {pattern}"))?);
        Some(builder.build()?)
    } else {
        None
    };

    let kind = kind.as_deref();
    let date = date.as_deref();
    let mut out: Vec<String> = entries
        .into_iter()
        .filter(|p| {
            let s = p.to_string_lossy();
            if let Some(k) = kind {
                let ok = match k {
                    "owner" => s.starts_with("owner/"),
                    "activity" => s.starts_with("agent/activity/") || s.starts_with("activity/"),
                    "tasks" => s.starts_with("agent/tasks/") || s.starts_with("tasks/"),
                    "inbox" => s.starts_with("agent/inbox/") || s.starts_with("inbox/"),
                    _ => false,
                };
                if !ok {
                    return false;
                }
            }
            if let Some(d) = date {
                if !s.contains(d) {
                    return false;
                }
            }
            if let Some(glob) = &path_filter {
                if !glob.is_match(&*s) {
                    return false;
                }
            }
            true
        })
        .map(|p| p.to_string_lossy().to_string())
        .collect();

    if let Some(n) = limit {
        out.truncate(n);
    }

    if json {
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else {
        for e in out {
            println!("{e}");
        }
    }
    Ok(())
}

fn cmd_search(
    memory_dir: &Path,
    query: &str,
    top_k: usize,
    _lexical_only: bool,
    semantic_only: bool,
    json: bool,
) -> Result<()> {
    if semantic_only {
        if json {
            println!("[]");
        }
        return Ok(());
    }
    let hits = search_hits(memory_dir, query, top_k)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&hits)?);
    } else {
        for hit in hits {
            println!("{:.3}\t{}\t{}", hit.score, hit.path, hit.snippet);
        }
    }
    Ok(())
}

fn cmd_remember(memory_dir: &Path, query: Option<String>, json: bool) -> Result<()> {
    let mut memories = Vec::new();
    for p in ["P0", "P1", "P2", "P3"] {
        let dir = memory_dir.join("agent").join("memory").join(p);
        if !dir.exists() {
            continue;
        }
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }
            let content = fs::read_to_string(&path)?;
            let (_, body) = parse_daily_frontmatter_and_body(&content);
            memories.push(serde_json::json!({
                "priority": p,
                "path": rel_or_abs(memory_dir, &path),
                "filename": path.file_name().unwrap_or_default().to_string_lossy(),
                "content": body.trim(),
            }));
        }
    }

    if let Some(q) = query {
        let q = q.to_lowercase();
        memories.retain(|m| {
            m["content"]
                .as_str()
                .unwrap_or_default()
                .to_lowercase()
                .contains(&q)
                || m["filename"]
                    .as_str()
                    .unwrap_or_default()
                    .to_lowercase()
                    .contains(&q)
        });
    }

    if json {
        println!("{}", serde_json::to_string_pretty(&memories)?);
    } else {
        for m in memories {
            println!(
                "== {} ({}) ==\n[{}]\n{}\n",
                m["priority"].as_str().unwrap_or_default(),
                m["filename"].as_str().unwrap_or_default(),
                m["path"].as_str().unwrap_or_default(),
                m["content"].as_str().unwrap_or_default()
            );
        }
    }
    Ok(())
}

fn cmd_set_memory(
    memory_dir: &Path,
    text: &str,
    filename: &str,
    priority: &str,
    json: bool,
) -> Result<()> {
    let p = normalize_priority(priority)?;
    let mut fname = filename.to_string();
    if !fname.ends_with(".md") {
        fname.push_str(".md");
    }

    if let Some(existing_path) = find_memory_file(memory_dir, &fname) {
        bail!(
            "memory file already exists at: {}",
            rel_or_abs(memory_dir, &existing_path)
        );
    }

    let target_path = memory_dir.join("agent").join("memory").join(p).join(&fname);
    ensure_parent(&target_path)?;
    fs::write(&target_path, text)?;

    if json {
        println!(
            "{}",
            serde_json::json!({
                "path": rel_or_abs(memory_dir, &target_path),
                "priority": p,
                "filename": fname,
            })
        );
    } else {
        println!("{}", rel_or_abs(memory_dir, &target_path));
    }
    Ok(())
}

fn cmd_triage_memory(
    memory_dir: &Path,
    filename: &str,
    new_priority: &str,
    json: bool,
) -> Result<()> {
    let new_p = normalize_priority(new_priority)?;
    let mut fname = filename.to_string();
    if !fname.ends_with(".md") {
        fname.push_str(".md");
    }

    let source_path = find_memory_file(memory_dir, &fname)
        .ok_or_else(|| anyhow::anyhow!("memory file not found: {fname}"))?;
    let target_path = memory_dir
        .join("agent")
        .join("memory")
        .join(new_p)
        .join(&fname);

    if source_path == target_path {
        bail!("memory is already at priority {new_p}");
    }

    ensure_parent(&target_path)?;
    fs::rename(&source_path, &target_path)?;

    if json {
        println!(
            "{}",
            serde_json::json!({
                "from": rel_or_abs(memory_dir, &source_path),
                "to": rel_or_abs(memory_dir, &target_path),
                "priority": new_p,
            })
        );
    } else {
        println!("{}", rel_or_abs(memory_dir, &target_path));
    }
    Ok(())
}

fn find_memory_file(memory_dir: &Path, filename: &str) -> Option<PathBuf> {
    for p in ["P0", "P1", "P2", "P3"] {
        let path = memory_dir
            .join("agent")
            .join("memory")
            .join(p)
            .join(filename);
        if path.exists() {
            return Some(path);
        }
    }
    None
}

fn normalize_priority(raw: &str) -> Result<&'static str> {
    match raw.trim().to_uppercase().as_str() {
        "P0" => Ok("P0"),
        "P1" => Ok("P1"),
        "P2" => Ok("P2"),
        "P3" => Ok("P3"),
        _ => bail!("invalid priority: {raw}. use P0, P1, P2, or P3"),
    }
}

fn cmd_today(memory_dir: &Path, date: Option<String>, json: bool) -> Result<()> {
    let d = parse_or_today(date.as_deref())?;
    let today = load_today(memory_dir, d);

    if json {
        println!("{}", serde_json::to_string_pretty(&today)?);
        return Ok(());
    }

    println!("{}", render_today_snapshot(&today));
    Ok(())
}

fn cmd_context(memory_dir: &Path, task: &str, date: Option<String>, json: bool) -> Result<()> {
    let d = parse_or_today(date.as_deref())?;
    let today = load_today(memory_dir, d);
    let mut hits = search_hits(memory_dir, task, 5)?;

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "task": task,
                "today": today,
                "related": hits,
            }))?
        );
        return Ok(());
    }

    println!("Task Context: {task}");
    println!(
        "\n== Today Snapshot ==\nAgent Tasks:\n{}",
        empty_as_na(&today.open_tasks)
    );
    println!(
        "\nAgent Activities:\n{}",
        render_recent_daily_sections(&today.activity_recent)
    );
    println!("\n== Related Memory ==");
    if hits.is_empty() {
        println!("(none)");
    } else {
        for h in hits.drain(..) {
            println!("{:.3}\t{}\t{}", h.score, h.path, h.snippet);
        }
    }
    Ok(())
}

fn cmd_get(memory_dir: &Path, target: GetTarget, json: bool) -> Result<()> {
    init_memory_scaffold(memory_dir)?;
    match target {
        GetTarget::Owner { target } => cmd_get_owner(memory_dir, target, json),
        GetTarget::Agent { target } => cmd_get_agent(memory_dir, target, json),
        GetTarget::Diary {
            period,
            limit,
            detail,
            all,
        } => cmd_get_diary(memory_dir, period, limit, detail, all, json),
        GetTarget::Acts {
            period,
            limit,
            detail,
            all,
        } => cmd_get_acts(memory_dir, period, limit, detail, all, json),
        GetTarget::Tasks { period, limit } => cmd_get_tasks(memory_dir, period, limit, json),
    }
}

fn cmd_set(memory_dir: &Path, target: SetTarget, json: bool) -> Result<()> {
    init_memory_scaffold(memory_dir)?;
    match target {
        SetTarget::Diary { text, date, time } => cmd_set_diary(memory_dir, &text, date, time, json),
        SetTarget::Owner { target, value } => cmd_set_owner(memory_dir, target, value, json),
        SetTarget::Acts { text, date, source } => {
            let joined = text.join(" ");
            cmd_keep(memory_dir, joined.trim(), "activity", date, &source, json)
        }
        SetTarget::Tasks { args } => cmd_set_tasks(memory_dir, args, json),
        SetTarget::Memory {
            text,
            filename,
            priority,
        } => cmd_set_memory(memory_dir, &text, &filename, &priority, json),
    }
}

fn cmd_triage(memory_dir: &Path, target: TriageTarget, json: bool) -> Result<()> {
    init_memory_scaffold(memory_dir)?;
    match target {
        TriageTarget::Memory { filename, priority } => {
            cmd_triage_memory(memory_dir, &filename, &priority, json)
        }
    }
}

fn cmd_set_diary(
    memory_dir: &Path,
    text: &str,
    date: Option<String>,
    time: Option<String>,
    json: bool,
) -> Result<()> {
    let entry = text.trim();
    if entry.is_empty() {
        bail!("missing diary text. use: amem set diary <text> [--date yyyy-mm-dd] [--time HH:MM]");
    }

    let target_date = parse_or_today(date.as_deref())?;
    let target_time = parse_or_now_time(time.as_deref())?;
    let path = owner_diary_path(memory_dir, target_date);
    append_daily_line_with_frontmatter(
        &path,
        target_date,
        &format!("- {} {}", target_time, entry),
    )?;

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "path": rel_or_abs(memory_dir, &path),
                "date": target_date.to_string(),
                "time": target_time,
            }))?
        );
    } else {
        println!("{}", rel_or_abs(memory_dir, &path));
    }
    Ok(())
}

fn cmd_get_owner(memory_dir: &Path, target: Option<String>, json: bool) -> Result<()> {
    init_memory_scaffold(memory_dir)?;
    let profile_path = memory_dir.join("owner").join("profile.md");
    let preferences_path = memory_dir.join("owner").join("preferences.md");

    match target.as_deref().map(|s| s.trim().to_lowercase()) {
        None => {
            let content = read_or_empty(profile_path.clone());
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "path": rel_or_abs(memory_dir, &profile_path),
                        "content": content,
                    }))?
                );
            } else {
                println!("{}", content);
            }
            Ok(())
        }
        Some(t) if t == "preference" || t == "preferences" => {
            let content = read_or_empty(preferences_path.clone());
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "path": rel_or_abs(memory_dir, &preferences_path),
                        "content": content,
                    }))?
                );
            } else {
                println!("{}", content);
            }
            Ok(())
        }
        Some(t) => {
            let key = canonical_owner_key(&t).ok_or_else(|| {
                anyhow::anyhow!(
                    "unsupported owner key: {t}. supported: name, github_username(github), email, location, occupation(job), native_language(lang), birthday"
                )
            })?;
            let content = read_or_empty(profile_path);
            let value = owner_profile_value(&content, key).unwrap_or_default();
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "key": key,
                        "value": value,
                    }))?
                );
            } else {
                println!("{value}");
            }
            Ok(())
        }
    }
}

fn cmd_get_agent(memory_dir: &Path, target: Option<String>, json: bool) -> Result<()> {
    init_memory_scaffold(memory_dir)?;
    let identity_path = memory_dir.join("agent").join("IDENTITY.md");
    let soul_path = memory_dir.join("agent").join("SOUL.md");
    let identity_content = read_body_or_empty(identity_path.clone());
    let soul_content = read_body_or_empty(soul_path.clone());
    let (memories_content, memories_paths) = read_agent_memories(memory_dir);

    match target.as_deref().map(|s| s.trim().to_lowercase()) {
        None => {
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "identity": {
                            "path": rel_or_abs(memory_dir, &identity_path),
                            "content": identity_content,
                        },
                        "soul": {
                            "path": rel_or_abs(memory_dir, &soul_path),
                            "content": soul_content,
                        },
                        "memories": {
                            "paths": memories_paths
                                .iter()
                                .map(|p| rel_or_abs(memory_dir, Path::new(p)))
                                .collect::<Vec<_>>(),
                            "content": memories_content,
                        },
                    }))?
                );
            } else {
                println!(
                    "{}",
                    render_agent_sections(
                        memory_dir,
                        &identity_path,
                        &identity_content,
                        &soul_path,
                        &soul_content,
                        &memories_paths,
                        &memories_content,
                    )
                );
            }
            Ok(())
        }
        Some(t) if t == "identity" => {
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "path": rel_or_abs(memory_dir, &identity_path),
                        "content": identity_content,
                    }))?
                );
            } else {
                println!("{identity_content}");
            }
            Ok(())
        }
        Some(t) if t == "soul" => {
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "path": rel_or_abs(memory_dir, &soul_path),
                        "content": soul_content,
                    }))?
                );
            } else {
                println!("{soul_content}");
            }
            Ok(())
        }
        Some(t) if t == "memory" || t == "memories" => {
            let rel_paths = memories_paths
                .iter()
                .map(|p| rel_or_abs(memory_dir, Path::new(p)))
                .collect::<Vec<_>>();
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "paths": rel_paths,
                        "content": memories_content,
                    }))?
                );
            } else {
                let paths = rel_paths
                    .into_iter()
                    .map(|p| format!("[{p}]"))
                    .collect::<Vec<_>>()
                    .join("\n");
                if paths.is_empty() {
                    println!("{}", empty_as_na(&memories_content));
                } else {
                    println!("{}\n{}", paths, empty_as_na(&memories_content));
                }
            }
            Ok(())
        }
        Some(t) => {
            bail!("unsupported agent key: {t}. supported: identity, soul, memory(memories)")
        }
    }
}

fn render_agent_sections(
    memory_dir: &Path,
    identity_path: &Path,
    identity_content: &str,
    soul_path: &Path,
    soul_content: &str,
    memories_paths: &[String],
    memories_content: &str,
) -> String {
    let mut sections = Vec::new();
    sections.push(format!(
        "== Agent Identity ==\n[{}]\n{}",
        rel_or_abs(memory_dir, identity_path),
        empty_as_na(identity_content)
    ));
    sections.push(format!(
        "== Agent Soul ==\n[{}]\n{}",
        rel_or_abs(memory_dir, soul_path),
        empty_as_na(soul_content)
    ));

    let rel_paths = memories_paths
        .iter()
        .map(|p| rel_or_abs(memory_dir, Path::new(p)))
        .collect::<Vec<_>>();
    let paths = rel_paths
        .iter()
        .map(|p| format!("[{p}]"))
        .collect::<Vec<_>>()
        .join("\n");
    sections.push(format!(
        "== Agent Memories ==\n{}\n{}",
        if paths.is_empty() {
            String::new()
        } else {
            format!("{}\n", paths)
        },
        empty_as_na(memories_content)
    ));

    sections.join("\n\n")
}

fn cmd_set_owner(
    memory_dir: &Path,
    target: Option<String>,
    value_parts: Vec<String>,
    json: bool,
) -> Result<()> {
    init_memory_scaffold(memory_dir)?;
    let Some(target_raw) = target.map(|s| s.trim().to_lowercase()) else {
        bail!(
            "missing target. use: amem set owner <key> <value>. keys: name, github_username(github), email, location, occupation(job), native_language(lang), birthday, preference"
        );
    };
    let value = value_parts.join(" ").trim().to_string();

    if target_raw == "preference" || target_raw == "preferences" {
        if value.is_empty() {
            bail!("missing key:value. use: amem set owner preference <key:value>");
        }
        let Some((raw_key, raw_val)) = value.split_once(':') else {
            bail!("invalid preference format. use key:value");
        };
        let key = raw_key.trim();
        let val = raw_val.trim();
        if key.is_empty() || val.is_empty() {
            bail!("invalid preference format. use key:value");
        }
        let now = Local::now();
        let line = format!("- [{}] {}: {}", now.format("%Y-%m-%d %H:%M"), key, val);
        let path = memory_dir.join("owner").join("preferences.md");
        append_markdown_line(&path, &line)?;

        if json {
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "path": rel_or_abs(memory_dir, &path),
                    "key": key,
                    "value": val,
                    "recorded_at": now.format("%Y-%m-%d %H:%M").to_string(),
                }))?
            );
        } else {
            println!("{}", rel_or_abs(memory_dir, &path));
        }
        return Ok(());
    }

    let key = canonical_owner_key(&target_raw).ok_or_else(|| {
        anyhow::anyhow!(
            "unsupported owner key: {target_raw}. supported: name, github_username(github), email, location, occupation(job), native_language(lang), birthday, preference"
        )
    })?;
    if value.is_empty() {
        bail!("missing value. use: amem set owner {key} <value>");
    }

    let path = memory_dir.join("owner").join("profile.md");
    let mut lines: Vec<String> = fs::read_to_string(&path)
        .unwrap_or_default()
        .lines()
        .map(|s| s.to_string())
        .collect();

    let mut replaced = false;
    for line in &mut lines {
        if let Some(existing_val) = owner_profile_value(line, key) {
            if let Some(val_pos) = line.rfind(&existing_val) {
                *line = format!("{} {}", &line[..val_pos].trim_end(), value);
                replaced = true;
                break;
            }
        }
    }
    if !replaced {
        if !lines.last().map(|s| s.trim().is_empty()).unwrap_or(false) {
            lines.push(String::new());
        }
        lines.push(format!("{key}: {value}"));
    }

    let mut out = lines.join("\n");
    if !out.ends_with('\n') {
        out.push('\n');
    }
    fs::write(&path, out).with_context(|| format!("failed to write {}", path.to_string_lossy()))?;

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "path": rel_or_abs(memory_dir, &path),
                "key": key,
                "value": value,
            }))?
        );
    } else {
        println!("{}", rel_or_abs(memory_dir, &path));
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
struct ActivityEntry {
    timestamp: String,
    source: Option<String>,
    text: String,
    path: String,
}

#[derive(Debug, Clone, Serialize)]
struct DiaryEntry {
    timestamp: String,
    text: String,
    path: String,
}

#[derive(Debug, Clone)]
struct DailySummaryRow {
    date: String,
    summary: String,
}

fn cmd_get_diary(
    memory_dir: &Path,
    period: Option<String>,
    limit: Option<usize>,
    detail: bool,
    all: bool,
    json: bool,
) -> Result<()> {
    init_memory_scaffold(memory_dir)?;
    let mut entries = collect_diary_entries(memory_dir)?;
    if let Some(period_raw) = period.as_deref() {
        validate_period(period_raw)?;
        let mut filtered = Vec::new();
        for entry in entries {
            if diary_entry_matches_period(&entry, period_raw)? {
                filtered.push(entry);
            }
        }
        entries = filtered;
    }

    let period_norm = period.as_deref().map(|s| s.trim().to_ascii_lowercase());
    let summary_mode =
        !json && !detail && !all && matches!(period_norm.as_deref(), Some("week" | "month"));
    if summary_mode {
        let summary_period = period_norm.as_deref().unwrap_or("week");
        let summaries = collect_diary_daily_summaries(memory_dir, summary_period, limit)?;
        println!("Owner Diary:");
        if summaries.is_empty() {
            println!("(none)");
        }
        for row in summaries {
            println!("- [{}] {}", row.date, row.summary);
        }
        return Ok(());
    }

    let effective_limit = if all {
        usize::MAX
    } else {
        limit.unwrap_or_else(|| if period.is_some() { usize::MAX } else { 10 })
    };
    entries.truncate(effective_limit);

    if json {
        println!("{}", serde_json::to_string_pretty(&entries)?);
    } else {
        println!("Owner Diary:");
        if entries.is_empty() {
            println!("(none)");
        }
        for entry in entries {
            println!("- [{}] {}", entry.timestamp, entry.text);
        }
    }
    Ok(())
}

fn collect_diary_daily_summaries(
    memory_dir: &Path,
    period: &str,
    limit: Option<usize>,
) -> Result<Vec<DailySummaryRow>> {
    validate_period(period)?;
    let today = Local::now().date_naive();
    let mut per_date: HashMap<NaiveDate, String> = HashMap::new();
    for rel in memory_files(memory_dir)? {
        let rel_text = rel.to_string_lossy();
        if !rel_text.starts_with("owner/diary/") {
            continue;
        }
        let Some(date) = activity_date_from_rel(&rel) else {
            continue;
        };
        if !date_matches_period(date, period)? {
            continue;
        }
        let path = memory_dir.join(&rel);
        let content = fs::read_to_string(path).unwrap_or_default();
        let (summary, body) = parse_daily_frontmatter_and_body(&content);
        let resolved = resolve_daily_summary(summary.as_deref(), &body, date, today);
        if resolved.is_empty() {
            continue;
        }
        per_date.entry(date).or_insert(resolved);
    }

    let mut rows: Vec<(NaiveDate, String)> = per_date.into_iter().collect();
    rows.sort_by(|a, b| b.0.cmp(&a.0));
    rows.truncate(limit.unwrap_or_else(|| default_summary_limit_for_period(period)));
    Ok(rows
        .into_iter()
        .map(|(date, summary)| DailySummaryRow {
            date: date.format("%Y-%m-%d").to_string(),
            summary,
        })
        .collect())
}

fn collect_diary_entries(memory_dir: &Path) -> Result<Vec<DiaryEntry>> {
    let mut out = Vec::new();
    for rel in memory_files(memory_dir)? {
        let rel_text = rel.to_string_lossy();
        if !rel_text.starts_with("owner/diary/") {
            continue;
        }
        let Some(date) = activity_date_from_rel(&rel) else {
            continue;
        };
        let path = memory_dir.join(&rel);
        let content = fs::read_to_string(&path).unwrap_or_default();
        let (_, body) = parse_daily_frontmatter_and_body(&content);
        for line in body.lines() {
            if let Some(entry) = parse_diary_line(&date, line, &rel_text) {
                out.push(entry);
            }
        }
    }
    out.sort_by(|a, b| {
        b.timestamp
            .cmp(&a.timestamp)
            .then_with(|| a.path.cmp(&b.path))
    });
    Ok(out)
}

fn parse_diary_line(date: &NaiveDate, line: &str, path: &str) -> Option<DiaryEntry> {
    let body = line.strip_prefix("- ")?.trim();
    if body.is_empty() {
        return None;
    }

    let mut time = "00:00".to_string();
    let mut text = body;
    if body.len() >= 5 {
        let candidate = &body[..5];
        if is_hhmm(candidate) {
            time = candidate.to_string();
            text = body[5..].trim_start();
        }
    }
    let text = text.trim();
    if text.is_empty() {
        return None;
    }

    Some(DiaryEntry {
        timestamp: format!("{} {}", date.format("%Y-%m-%d"), time),
        text: text.to_string(),
        path: path.to_string(),
    })
}

fn diary_entry_matches_period(entry: &DiaryEntry, period: &str) -> Result<bool> {
    if entry.timestamp.len() < 10 {
        return Ok(false);
    }
    let date = NaiveDate::parse_from_str(&entry.timestamp[..10], "%Y-%m-%d")
        .with_context(|| format!("invalid diary timestamp: {}", entry.timestamp))?;
    date_matches_period(date, period)
}

fn cmd_get_acts(
    memory_dir: &Path,
    period: Option<String>,
    limit: Option<usize>,
    detail: bool,
    all: bool,
    json: bool,
) -> Result<()> {
    init_memory_scaffold(memory_dir)?;
    let mut entries = collect_activity_entries(memory_dir)?;
    if let Some(period_raw) = period.as_deref() {
        validate_period(period_raw)?;
        let mut filtered = Vec::new();
        for entry in entries {
            if activity_entry_matches_period(&entry, period_raw)? {
                filtered.push(entry);
            }
        }
        entries = filtered;
    }

    let period_norm = period.as_deref().map(|s| s.trim().to_ascii_lowercase());
    let summary_mode =
        !json && !detail && !all && matches!(period_norm.as_deref(), Some("week" | "month"));
    if summary_mode {
        let summary_period = period_norm.as_deref().unwrap_or("week");
        let summaries = collect_activity_daily_summaries(memory_dir, summary_period, limit)?;
        println!("Agent Activities:");
        if summaries.is_empty() {
            println!("(none)");
        }
        for row in summaries {
            println!("- [{}] {}", row.date, row.summary);
        }
        return Ok(());
    }

    let effective_limit = if all {
        usize::MAX
    } else {
        limit.unwrap_or_else(|| if period.is_some() { usize::MAX } else { 10 })
    };
    entries.truncate(effective_limit);

    if json {
        println!("{}", serde_json::to_string_pretty(&entries)?);
    } else {
        println!("Agent Activities:");
        if entries.is_empty() {
            println!("(none)");
        }
        for entry in entries {
            if let Some(source) = entry.source {
                println!("- [{}] [{}] {}", entry.timestamp, source, entry.text);
            } else {
                println!("- [{}] {}", entry.timestamp, entry.text);
            }
        }
    }
    Ok(())
}

fn collect_activity_daily_summaries(
    memory_dir: &Path,
    period: &str,
    limit: Option<usize>,
) -> Result<Vec<DailySummaryRow>> {
    validate_period(period)?;
    let today = Local::now().date_naive();
    let mut per_date: HashMap<NaiveDate, (u8, String)> = HashMap::new();
    for rel in memory_files(memory_dir)? {
        let rel_text = rel.to_string_lossy();
        if !rel_text.starts_with("agent/activity/") && !rel_text.starts_with("activity/") {
            continue;
        }
        let Some(date) = activity_date_from_rel(&rel) else {
            continue;
        };
        if !date_matches_period(date, period)? {
            continue;
        }
        let path = memory_dir.join(&rel);
        let content = fs::read_to_string(path).unwrap_or_default();
        let (summary, body) = parse_daily_frontmatter_and_body(&content);
        let resolved = resolve_daily_summary(summary.as_deref(), &body, date, today);
        if resolved.is_empty() {
            continue;
        }

        let priority = if rel_text.starts_with("agent/activity/") {
            0
        } else {
            1
        };
        match per_date.get(&date) {
            Some((existing_priority, _)) if *existing_priority <= priority => {}
            _ => {
                per_date.insert(date, (priority, resolved));
            }
        }
    }

    let mut rows: Vec<(NaiveDate, String)> = per_date
        .into_iter()
        .map(|(date, (_, summary))| (date, summary))
        .collect();
    rows.sort_by(|a, b| b.0.cmp(&a.0));
    rows.truncate(limit.unwrap_or_else(|| default_summary_limit_for_period(period)));
    Ok(rows
        .into_iter()
        .map(|(date, summary)| DailySummaryRow {
            date: date.format("%Y-%m-%d").to_string(),
            summary,
        })
        .collect())
}

fn collect_activity_entries(memory_dir: &Path) -> Result<Vec<ActivityEntry>> {
    let mut out = Vec::new();
    for rel in memory_files(memory_dir)? {
        let rel_text = rel.to_string_lossy();
        if !rel_text.starts_with("agent/activity/") && !rel_text.starts_with("activity/") {
            continue;
        }
        let Some(date) = activity_date_from_rel(&rel) else {
            continue;
        };
        let path = memory_dir.join(&rel);
        let content = fs::read_to_string(&path).unwrap_or_default();
        let (_, body) = parse_daily_frontmatter_and_body(&content);
        for line in body.lines() {
            if let Some(entry) = parse_activity_line(&date, line, &rel_text) {
                out.push(entry);
            }
        }
    }
    out.sort_by(|a, b| {
        b.timestamp
            .cmp(&a.timestamp)
            .then_with(|| a.path.cmp(&b.path))
    });
    Ok(out)
}

fn activity_date_from_rel(rel: &Path) -> Option<NaiveDate> {
    let file = rel.file_name()?.to_str()?;
    if file.len() < 10 {
        return None;
    }
    NaiveDate::parse_from_str(&file[..10], "%Y-%m-%d").ok()
}

fn parse_activity_line(date: &NaiveDate, line: &str, path: &str) -> Option<ActivityEntry> {
    let body = line.strip_prefix("- ")?.trim();
    if body.is_empty() {
        return None;
    }

    let mut time = "00:00".to_string();
    let mut rest = body;
    if body.len() >= 5 {
        let candidate = &body[..5];
        if is_hhmm(candidate) {
            time = candidate.to_string();
            rest = body[5..].trim_start();
        }
    }

    let (source, text) = if let Some(after_open) = rest.strip_prefix('[') {
        if let Some(end) = after_open.find(']') {
            let source = after_open[..end].trim().to_string();
            let text = after_open[end + 1..].trim().to_string();
            (
                if source.is_empty() {
                    None
                } else {
                    Some(source)
                },
                text,
            )
        } else {
            (None, rest.trim().to_string())
        }
    } else {
        (None, rest.trim().to_string())
    };
    if text.is_empty() {
        return None;
    }

    Some(ActivityEntry {
        timestamp: format!("{} {}", date.format("%Y-%m-%d"), time),
        source,
        text,
        path: path.to_string(),
    })
}

fn activity_entry_matches_period(entry: &ActivityEntry, period: &str) -> Result<bool> {
    if entry.timestamp.len() < 10 {
        return Ok(false);
    }
    let date = NaiveDate::parse_from_str(&entry.timestamp[..10], "%Y-%m-%d")
        .with_context(|| format!("invalid activity timestamp: {}", entry.timestamp))?;
    date_matches_period(date, period)
}

fn date_matches_period(date: NaiveDate, period_raw: &str) -> Result<bool> {
    let period = period_raw.trim().to_lowercase();
    let today = Local::now().date_naive();
    match period.as_str() {
        "today" => Ok(date == today),
        "yesterday" => Ok(date == today - Duration::days(1)),
        "week" => {
            let start = today - Duration::days(6);
            Ok(date >= start && date <= today)
        }
        "month" => Ok(date.year() == today.year() && date.month() == today.month()),
        _ => {
            let specific = NaiveDate::parse_from_str(&period, "%Y-%m-%d").with_context(|| {
                format!(
                    "unsupported period: {period_raw}. use today|yesterday|week|month|yyyy-mm-dd"
                )
            })?;
            Ok(date == specific)
        }
    }
}

fn validate_period(period_raw: &str) -> Result<()> {
    let period = period_raw.trim().to_lowercase();
    match period.as_str() {
        "today" | "yesterday" | "week" | "month" => Ok(()),
        _ => {
            NaiveDate::parse_from_str(&period, "%Y-%m-%d").with_context(|| {
                format!(
                    "unsupported period: {period_raw}. use today|yesterday|week|month|yyyy-mm-dd"
                )
            })?;
            Ok(())
        }
    }
}

fn default_summary_limit_for_period(period_raw: &str) -> usize {
    match period_raw.trim().to_ascii_lowercase().as_str() {
        "month" => 31,
        _ => 7,
    }
}

#[derive(Debug, Clone, Serialize)]
struct TaskEntry {
    status: String,
    timestamp: Option<String>,
    hash: Option<String>,
    text: String,
    #[serde(skip_serializing)]
    raw_line: String,
    #[serde(skip_serializing)]
    line_index: usize,
    #[serde(skip_serializing)]
    source_path: PathBuf,
}

fn cmd_get_tasks(
    memory_dir: &Path,
    period: Option<String>,
    limit: Option<usize>,
    json: bool,
) -> Result<()> {
    init_memory_scaffold(memory_dir)?;
    let mut entries = Vec::new();
    for path in open_task_paths(memory_dir) {
        entries.extend(load_task_entries(&path, "open")?);
    }
    for path in done_task_paths(memory_dir) {
        entries.extend(load_task_entries(&path, "done")?);
    }

    if let Some(period_raw) = period.as_deref() {
        validate_period(period_raw)?;
        let mut filtered = Vec::new();
        for entry in entries {
            let Some(ts) = entry.timestamp.as_deref() else {
                continue;
            };
            if ts.len() < 10 {
                continue;
            }
            let date = NaiveDate::parse_from_str(&ts[..10], "%Y-%m-%d")
                .with_context(|| format!("invalid task timestamp: {ts}"))?;
            if date_matches_period(date, period_raw)? {
                filtered.push(entry);
            }
        }
        entries = filtered;
    }

    entries.sort_by(|a, b| {
        b.timestamp
            .cmp(&a.timestamp)
            .then_with(|| a.status.cmp(&b.status))
            .then_with(|| a.text.cmp(&b.text))
    });
    let effective_limit = limit.unwrap_or_else(|| if period.is_some() { usize::MAX } else { 10 });
    entries.truncate(effective_limit);

    if json {
        println!("{}", serde_json::to_string_pretty(&entries)?);
    } else {
        println!("Agent Tasks:");
        if entries.is_empty() {
            println!("(none)");
        }
        for entry in entries {
            let ts = entry.timestamp.unwrap_or_else(|| "unknown".to_string());
            if let Some(hash) = entry.hash {
                println!("- [{}] [{}] [{}] {}", ts, entry.status, hash, entry.text);
            } else {
                println!("- [{}] [{}] {}", ts, entry.status, entry.text);
            }
        }
    }
    Ok(())
}

fn cmd_set_tasks(memory_dir: &Path, args: Vec<String>, json: bool) -> Result<()> {
    init_memory_scaffold(memory_dir)?;
    if args.is_empty() {
        bail!("missing task args. use: amem set tasks <task> | amem set tasks done <hash|text>");
    }
    if args[0].eq_ignore_ascii_case("done") {
        if args.len() < 2 {
            bail!("missing task selector. use: amem set tasks done <hash|text>");
        }
        return cmd_set_tasks_done(memory_dir, args[1..].join(" "), json);
    }
    cmd_set_tasks_add(memory_dir, args.join(" "), json)
}

fn cmd_set_tasks_add(memory_dir: &Path, raw_text: String, json: bool) -> Result<()> {
    let text = raw_text.trim().to_string();
    if text.is_empty() {
        bail!("missing task text. use: amem set tasks <task>");
    }

    let open_path = agent_tasks_open_path(memory_dir);
    let mut existing = Vec::new();
    for path in open_task_paths(memory_dir) {
        existing.extend(load_task_entries(&path, "open")?);
    }
    for path in done_task_paths(memory_dir) {
        existing.extend(load_task_entries(&path, "done")?);
    }
    if let Some(found) = existing.into_iter().find(|e| e.text == text) {
        let hash = found.hash.unwrap_or_else(|| short_task_hash(&text));
        bail!("task already exists: [{hash}] {text}");
    }

    let hash = short_task_hash(&text);
    let now = Local::now().format("%Y-%m-%d %H:%M").to_string();
    append_markdown_line(&open_path, &format!("- [{now}] [{hash}] {text}"))?;

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "path": rel_or_abs(memory_dir, &open_path),
                "hash": hash,
                "status": "added",
            }))?
        );
    } else {
        println!("{hash}");
    }
    Ok(())
}

fn cmd_set_tasks_done(memory_dir: &Path, selector_raw: String, json: bool) -> Result<()> {
    let selector = selector_raw.trim().to_string();
    if selector.is_empty() {
        bail!("missing task selector. use: amem set tasks done <hash|text>");
    }

    let done_path = agent_tasks_done_path(memory_dir);
    let mut entries = Vec::new();
    for path in open_task_paths(memory_dir) {
        entries.extend(load_task_entries(&path, "open")?);
    }
    let matches: Vec<TaskEntry> = entries
        .into_iter()
        .filter(|entry| task_selector_matches(entry, &selector))
        .collect();

    if matches.is_empty() {
        bail!("task not found: {selector}");
    }
    if matches.len() > 1 {
        bail!("multiple tasks matched selector: {selector}");
    }

    let target = matches[0].clone();
    let open_content = fs::read_to_string(&target.source_path).unwrap_or_default();
    let mut lines: Vec<String> = open_content.lines().map(|s| s.to_string()).collect();
    if target.line_index < lines.len() {
        lines.remove(target.line_index);
    }
    let mut rewritten = lines.join("\n");
    if !rewritten.ends_with('\n') {
        rewritten.push('\n');
    }
    fs::write(&target.source_path, rewritten)
        .with_context(|| format!("failed to write {}", target.source_path.to_string_lossy()))?;
    append_markdown_line(&done_path, &target.raw_line)?;

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "from": rel_or_abs(memory_dir, &target.source_path),
                "to": rel_or_abs(memory_dir, &done_path),
                "hash": target.hash,
                "status": "done",
            }))?
        );
    } else if let Some(hash) = target.hash {
        println!("{hash}");
    } else {
        println!("{}", target.text);
    }
    Ok(())
}

fn task_selector_matches(entry: &TaskEntry, selector: &str) -> bool {
    let query = selector.trim();
    if query.is_empty() {
        return false;
    }
    if query.chars().all(|c| c.is_ascii_hexdigit()) && query.len() <= 7 {
        return entry
            .hash
            .as_deref()
            .map(|h| h.starts_with(query))
            .unwrap_or(false);
    }
    entry.text == query
}

fn load_task_entries(path: &Path, status: &str) -> Result<Vec<TaskEntry>> {
    let content = fs::read_to_string(path).unwrap_or_default();
    let mut out = Vec::new();
    for (idx, line) in content.lines().enumerate() {
        let Some(parsed) = parse_task_line(line) else {
            continue;
        };
        out.push(TaskEntry {
            status: status.to_string(),
            timestamp: parsed.timestamp,
            hash: parsed.hash,
            text: parsed.text,
            raw_line: line.to_string(),
            line_index: idx,
            source_path: path.to_path_buf(),
        });
    }
    Ok(out)
}

#[derive(Debug, Clone)]
struct ParsedTaskLine {
    timestamp: Option<String>,
    hash: Option<String>,
    text: String,
}

fn parse_task_line(line: &str) -> Option<ParsedTaskLine> {
    let body = line.strip_prefix("- ")?.trim();
    if body.is_empty() {
        return None;
    }

    let mut rest = body;
    let mut timestamp = None;
    let mut hash = None;

    if let Some((token, after_token)) = take_bracket_token(rest) {
        if NaiveDateTime::parse_from_str(&token, "%Y-%m-%d %H:%M").is_ok() {
            timestamp = Some(token);
            rest = after_token;
            if let Some((hash_token, after_hash)) = take_bracket_token(rest) {
                if hash_token.chars().all(|c| c.is_ascii_hexdigit()) {
                    hash = Some(hash_token.to_lowercase());
                    rest = after_hash;
                }
            }
        }
    }

    let text = rest.trim().to_string();
    if text.is_empty() {
        return None;
    }
    Some(ParsedTaskLine {
        timestamp,
        hash,
        text,
    })
}

fn take_bracket_token(input: &str) -> Option<(String, &str)> {
    let trimmed = input.trim_start();
    let after_open = trimmed.strip_prefix('[')?;
    let end = after_open.find(']')?;
    let token = after_open[..end].trim().to_string();
    let rest = after_open[end + 1..].trim_start();
    Some((token, rest))
}

fn append_markdown_line(path: &Path, line: &str) -> Result<()> {
    ensure_parent(path)?;

    let needs_newline = fs::read(path)
        .map(|bytes| !bytes.is_empty() && !bytes.ends_with(b"\n"))
        .unwrap_or(false);
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("failed to open {}", path.to_string_lossy()))?;
    if needs_newline {
        file.write_all(b"\n")
            .with_context(|| format!("failed to write {}", path.to_string_lossy()))?;
    }
    file.write_all(line.as_bytes())
        .with_context(|| format!("failed to write {}", path.to_string_lossy()))?;
    file.write_all(b"\n")
        .with_context(|| format!("failed to write {}", path.to_string_lossy()))?;
    Ok(())
}

fn append_daily_line_with_frontmatter(
    path: &Path,
    target_date: NaiveDate,
    line: &str,
) -> Result<()> {
    ensure_parent(path)?;
    let content = fs::read_to_string(path).unwrap_or_default();
    let (summary, mut body) = parse_daily_frontmatter_and_body(&content);

    if !body.trim().is_empty() && !body.ends_with('\n') {
        body.push('\n');
    }
    body.push_str(line.trim_end());
    body.push('\n');

    let today = Local::now().date_naive();
    let resolved_summary = if target_date < today {
        resolve_daily_summary(summary.as_deref(), &body, target_date, today)
    } else {
        summary.unwrap_or_default()
    };
    let rendered = render_daily_markdown_with_frontmatter(&resolved_summary, &body);
    fs::write(path, rendered)
        .with_context(|| format!("failed to write {}", path.to_string_lossy()))?;
    Ok(())
}

fn parse_daily_frontmatter_and_body(content: &str) -> (Option<String>, String) {
    let normalized = content.replace("\r\n", "\n");
    let lines: Vec<&str> = normalized.split('\n').collect();
    if lines.first().copied() != Some("---") {
        return (None, normalized);
    }

    let mut summary = None;
    for idx in 1..lines.len() {
        let line = lines[idx];
        if line == "---" {
            let body = lines[idx + 1..].join("\n");
            return (summary, body);
        }
        if let Some(raw) = line.trim().strip_prefix("summary:") {
            summary = Some(parse_simple_yaml_scalar(raw.trim()));
        }
    }
    (None, normalized)
}

fn parse_simple_yaml_scalar(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if trimmed.len() >= 2 && trimmed.starts_with('\'') && trimmed.ends_with('\'') {
        return trimmed[1..trimmed.len() - 1].replace("''", "'");
    }
    if trimmed.len() >= 2 && trimmed.starts_with('"') && trimmed.ends_with('"') {
        let inner = &trimmed[1..trimmed.len() - 1];
        let mut out = String::new();
        let mut escaped = false;
        for ch in inner.chars() {
            if escaped {
                out.push(match ch {
                    'n' => '\n',
                    't' => '\t',
                    '"' => '"',
                    '\\' => '\\',
                    other => other,
                });
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else {
                out.push(ch);
            }
        }
        if escaped {
            out.push('\\');
        }
        return out;
    }
    trimmed.to_string()
}

fn render_daily_markdown_with_frontmatter(summary: &str, body: &str) -> String {
    let normalized_summary = collapse_inline_whitespace(summary);
    let encoded_summary = normalized_summary
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    let mut out = format!("---\nsummary: \"{}\"\n---\n", encoded_summary);
    if !body.is_empty() {
        out.push_str(body);
        if !out.ends_with('\n') {
            out.push('\n');
        }
    }
    out
}

fn resolve_daily_summary(
    frontmatter_summary: Option<&str>,
    body: &str,
    date: NaiveDate,
    today: NaiveDate,
) -> String {
    let raw = frontmatter_summary.unwrap_or("").trim();
    if !raw.is_empty() {
        return raw.to_string();
    }
    if date < today {
        return derive_summary_from_body(body);
    }
    String::new()
}

fn derive_summary_from_body(body: &str) -> String {
    let mut parts = Vec::new();
    for line in body.lines() {
        let Some(text) = extract_summary_text_from_bullet_line(line) else {
            continue;
        };
        if parts.contains(&text) {
            continue;
        }
        parts.push(text);
        if parts.len() >= 3 {
            break;
        }
    }
    let mut summary = match parts.len() {
        0 => String::new(),
        1 => parts[0].clone(),
        2 => format!("{} / {}", parts[0], parts[1]),
        _ => format!("{} / {} など", parts[0], parts[1]),
    };

    if summary.chars().count() > 90 {
        summary = format!("{}...", summary.chars().take(87).collect::<String>());
    }
    summary
}

fn extract_summary_text_from_bullet_line(line: &str) -> Option<String> {
    let body = line.trim().strip_prefix("- ")?.trim();
    if body.is_empty() {
        return None;
    }

    let mut rest = body;
    if rest.len() >= 5 && is_hhmm(&rest[..5]) {
        rest = rest[5..].trim_start();
    }
    if let Some(after_open) = rest.strip_prefix('[') {
        if let Some(end) = after_open.find(']') {
            rest = after_open[end + 1..].trim_start();
        }
    }

    let text = collapse_inline_whitespace(rest);
    if text.is_empty() { None } else { Some(text) }
}

fn collapse_inline_whitespace(raw: &str) -> String {
    raw.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn short_task_hash(text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    let digest = format!("{:x}", hasher.finalize());
    digest[..7].to_string()
}

fn is_hhmm(raw: &str) -> bool {
    if raw.len() != 5 {
        return false;
    }
    let bytes = raw.as_bytes();
    bytes[0].is_ascii_digit()
        && bytes[1].is_ascii_digit()
        && bytes[2] == b':'
        && bytes[3].is_ascii_digit()
        && bytes[4].is_ascii_digit()
}

fn canonical_owner_key(raw: &str) -> Option<&'static str> {
    match raw.trim().to_lowercase().as_str() {
        "name" => Some("name"),
        "what_to_call_them" | "call" | "nickname" => Some("what_to_call_them"),
        "pronouns" => Some("pronouns"),
        "timezone" | "tz" => Some("timezone"),
        "language" | "native_language" | "lang" => Some("native_language"),
        "github_username" | "github" | "github_handle" => Some("github_username"),
        "email" => Some("email"),
        "location" => Some("location"),
        "occupation" | "job" => Some("occupation"),
        "birthday" => Some("birthday"),
        _ => None,
    }
}

fn owner_profile_value(content: &str, key: &str) -> Option<String> {
    let mut aliases = vec![key.to_string()];
    match key {
        "name" => {
            aliases.push("Name".to_string());
            aliases.push("**Name**".to_string());
            aliases.push("**Name:**".to_string());
        }
        "what_to_call_them" => {
            aliases.push("What to call them".to_string());
            aliases.push("**What to call them**".to_string());
            aliases.push("**What to call them:**".to_string());
        }
        "pronouns" => {
            aliases.push("Pronouns".to_string());
            aliases.push("**Pronouns**".to_string());
            aliases.push("**Pronouns:**".to_string());
        }
        "timezone" => {
            aliases.push("Timezone".to_string());
            aliases.push("**Timezone**".to_string());
            aliases.push("**Timezone:**".to_string());
        }
        "native_language" => {
            aliases.push("Language".to_string());
            aliases.push("**Language**".to_string());
            aliases.push("**Language:**".to_string());
            aliases.push("native_language".to_string());
        }
        "github_username" => {
            aliases.push("github_handle".to_string());
        }
        _ => {}
    }
    aliases.sort_by_key(|b| std::cmp::Reverse(b.len()));

    for line in content.lines() {
        let l = line.trim();
        if l.is_empty() || l.starts_with('#') {
            continue;
        }
        for alias in &aliases {
            if let Some(pos) = l.find(alias) {
                let rest = l[pos + alias.len()..].trim();
                let mut res = if let Some(val) = rest.strip_prefix(':') {
                    val.trim().to_string()
                } else if alias.ends_with(':') && !rest.is_empty() {
                    rest.to_string()
                } else {
                    continue;
                };

                // Clean up markdown bold markers if any
                res = res.trim_matches('*').trim().to_string();
                if !res.is_empty() {
                    return Some(res);
                }
            }
        }
    }
    None
}

fn cmd_index(memory_dir: &Path, rebuild: bool, json: bool) -> Result<()> {
    let index_dir = memory_dir.join(".index");
    fs::create_dir_all(&index_dir).with_context(|| {
        format!(
            "failed to create index directory {}",
            index_dir.to_string_lossy()
        )
    })?;
    let index_db = index_dir.join("index.db");
    if rebuild && index_db.exists() {
        fs::remove_file(&index_db)
            .with_context(|| format!("failed to remove {}", index_db.to_string_lossy()))?;
    }

    let mut conn = Connection::open(&index_db)
        .with_context(|| format!("failed to open {}", index_db.to_string_lossy()))?;
    conn.execute_batch(
        r#"
        PRAGMA journal_mode=WAL;
        CREATE TABLE IF NOT EXISTS files(
            path TEXT PRIMARY KEY,
            content_hash TEXT NOT NULL,
            mtime INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS chunks(
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            path TEXT NOT NULL,
            chunk_text TEXT NOT NULL,
            line_start INTEGER NOT NULL,
            line_end INTEGER NOT NULL,
            updated_at INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS postings(
            token TEXT NOT NULL,
            chunk_id INTEGER NOT NULL,
            tf INTEGER NOT NULL,
            PRIMARY KEY(token, chunk_id),
            FOREIGN KEY(chunk_id) REFERENCES chunks(id) ON DELETE CASCADE
        );
        CREATE TABLE IF NOT EXISTS token_stats(
            token TEXT PRIMARY KEY,
            df INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS embedding_cache(
            cache_key TEXT PRIMARY KEY,
            vector BLOB,
            created_at INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_postings_token ON postings(token);
        CREATE INDEX IF NOT EXISTS idx_chunks_path ON chunks(path);
        "#,
    )?;

    let docs = load_docs(memory_dir)?;
    let tx = conn.transaction()?;
    tx.execute("DELETE FROM files", [])?;
    tx.execute("DELETE FROM chunks", [])?;
    tx.execute("DELETE FROM postings", [])?;
    tx.execute("DELETE FROM token_stats", [])?;

    for (path, content) in docs {
        let abs = memory_dir.join(&path);
        let mtime = fs::metadata(&abs)
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        let mut hasher = Sha256::new();
        hasher.update(content.as_bytes());
        let hash = format!("{:x}", hasher.finalize());

        tx.execute(
            "INSERT INTO files(path, content_hash, mtime) VALUES (?1, ?2, ?3)",
            params![path.to_string_lossy().to_string(), hash, mtime],
        )?;

        for (i, para) in content
            .split("\n\n")
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .enumerate()
        {
            tx.execute(
                "INSERT INTO chunks(path, chunk_text, line_start, line_end, updated_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    path.to_string_lossy().to_string(),
                    para,
                    i as i64 + 1,
                    i as i64 + 1,
                    Local::now().timestamp()
                ],
            )?;
            let chunk_id = tx.last_insert_rowid();
            for (token, tf) in unigram_freqs(para) {
                tx.execute(
                    "INSERT INTO postings(token, chunk_id, tf) VALUES (?1, ?2, ?3)",
                    params![token, chunk_id, tf],
                )?;
            }
        }
    }

    tx.execute(
        "INSERT INTO token_stats(token, df) SELECT token, COUNT(*) FROM postings GROUP BY token",
        [],
    )?;
    tx.commit()?;

    if json {
        println!(
            "{}",
            serde_json::json!({
                "index_db": index_db.to_string_lossy(),
                "status": "ok"
            })
        );
    } else {
        println!("{}", index_db.to_string_lossy());
    }
    Ok(())
}

fn cmd_watch(memory_dir: &Path) -> Result<()> {
    let _ = memory_dir;
    println!("watch mode is not implemented yet. use `amem index` periodically.");
    Ok(())
}

fn cmd_codex(
    memory_dir: &Path,
    cwd: &Path,
    resume_only: bool,
    prompt: Option<String>,
) -> Result<()> {
    init_memory_scaffold(memory_dir)?;

    let codex_bin = std::env::var("AMEM_CODEX_BIN").unwrap_or_else(|_| "codex".to_string());
    let mut seed_thread_id: Option<String> = None;
    if !resume_only {
        let bootstrap = codex_bootstrap_prompt(memory_dir)?;
        let output = ProcessCommand::new(&codex_bin)
            .arg("exec")
            .arg("--json")
            .arg("--dangerously-bypass-approvals-and-sandbox")
            .arg("--skip-git-repo-check")
            .arg("--cd")
            .arg(cwd)
            .arg(bootstrap)
            .output()
            .with_context(|| format!("failed to run `{codex_bin} exec`"))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            bail!(
                "`{codex_bin} exec` failed (status: {}): {}{}",
                output
                    .status
                    .code()
                    .map(|n| n.to_string())
                    .unwrap_or_else(|| "signal".to_string()),
                stderr.trim(),
                if stderr.trim().is_empty() {
                    format!("\n{}", stdout.trim())
                } else {
                    String::new()
                }
            );
        }
        seed_thread_id = extract_codex_thread_id(&output.stdout);
        if seed_thread_id.is_none() {
            bail!(
                "seed session was created but thread_id was not found in `codex exec --json` output; refusing to fallback to `resume --last`"
            );
        }
    }

    let mut resume = ProcessCommand::new(&codex_bin);
    resume.arg("resume");
    resume.arg("--dangerously-bypass-approvals-and-sandbox");
    if resume_only {
        resume.arg("--last");
    } else if let Some(thread_id) = seed_thread_id {
        resume.arg(thread_id);
    } else {
        bail!("internal error: missing seed thread id");
    }
    resume.arg("--cd").arg(cwd);
    if let Some(p) = prompt {
        resume.arg(p);
    }
    let status = resume
        .status()
        .with_context(|| format!("failed to run `{codex_bin} resume`"))?;
    if !status.success() {
        bail!(
            "`{codex_bin} resume` failed (status: {})",
            status
                .code()
                .map(|n| n.to_string())
                .unwrap_or_else(|| "signal".to_string())
        );
    }
    Ok(())
}

fn cmd_gemini(
    memory_dir: &Path,
    cwd: &Path,
    resume_only: bool,
    prompt: Option<String>,
) -> Result<()> {
    init_memory_scaffold(memory_dir)?;

    let gemini_bin = std::env::var("AMEM_GEMINI_BIN").unwrap_or_else(|_| "gemini".to_string());
    let mut seed_session_id: Option<String> = None;
    if !resume_only {
        let bootstrap = gemini_bootstrap_prompt(memory_dir)?;
        let output = ProcessCommand::new(&gemini_bin)
            .current_dir(cwd)
            .arg("--approval-mode")
            .arg("yolo")
            .arg("--output-format")
            .arg("json")
            .arg("-p")
            .arg(bootstrap)
            .output()
            .with_context(|| format!("failed to run `{gemini_bin}` seed prompt"))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            bail!(
                "`{gemini_bin}` seed failed (status: {}): {}{}",
                output
                    .status
                    .code()
                    .map(|n| n.to_string())
                    .unwrap_or_else(|| "signal".to_string()),
                stderr.trim(),
                if stderr.trim().is_empty() {
                    format!("\n{}", stdout.trim())
                } else {
                    String::new()
                }
            );
        }
        seed_session_id = extract_gemini_session_id(&output.stdout);
        if seed_session_id.is_none() {
            bail!(
                "seed session was created but session_id was not found in Gemini JSON output; refusing to fallback to `--resume latest`"
            );
        }
    }

    let mut resume = ProcessCommand::new(&gemini_bin);
    resume
        .current_dir(cwd)
        .arg("--approval-mode")
        .arg("yolo")
        .arg("--resume");
    if resume_only {
        resume.arg("latest");
    } else if let Some(session_id) = seed_session_id {
        resume.arg(session_id);
    } else {
        bail!("internal error: missing Gemini seed session id");
    }
    if let Some(p) = prompt {
        resume.arg("--prompt-interactive").arg(p);
    }
    let status = resume
        .status()
        .with_context(|| format!("failed to run `{gemini_bin} --resume`"))?;
    if !status.success() {
        bail!(
            "`{gemini_bin} --resume` failed (status: {})",
            status
                .code()
                .map(|n| n.to_string())
                .unwrap_or_else(|| "signal".to_string())
        );
    }
    Ok(())
}

fn cmd_claude(
    memory_dir: &Path,
    cwd: &Path,
    resume_only: bool,
    prompt: Option<String>,
) -> Result<()> {
    init_memory_scaffold(memory_dir)?;

    let claude_bin = resolve_claude_bin();
    let mut seed_session_id: Option<String> = None;
    if !resume_only {
        let bootstrap = claude_bootstrap_prompt(memory_dir)?;
        let output = ProcessCommand::new(&claude_bin)
            .current_dir(cwd)
            .arg("--dangerously-skip-permissions")
            .arg("--print")
            .arg("--output-format")
            .arg("json")
            .arg(bootstrap)
            .output()
            .with_context(|| format!("failed to run `{claude_bin}` seed prompt"))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            bail!(
                "`{claude_bin}` seed failed (status: {}): {}{}",
                output
                    .status
                    .code()
                    .map(|n| n.to_string())
                    .unwrap_or_else(|| "signal".to_string()),
                stderr.trim(),
                if stderr.trim().is_empty() {
                    format!("\n{}", stdout.trim())
                } else {
                    String::new()
                }
            );
        }
        seed_session_id = extract_claude_session_id(&output.stdout);
        if seed_session_id.is_none() {
            bail!(
                "seed session was created but session_id was not found in Claude JSON output; refusing to fallback to `--continue`"
            );
        }
    }

    let mut resume = ProcessCommand::new(&claude_bin);
    resume
        .current_dir(cwd)
        .arg("--dangerously-skip-permissions");
    if resume_only {
        resume.arg("--continue");
    } else if let Some(session_id) = seed_session_id {
        resume.arg("--resume").arg(session_id);
    } else {
        bail!("internal error: missing Claude seed session id");
    }
    if let Some(p) = prompt {
        resume.arg(p);
    }
    let status = resume
        .status()
        .with_context(|| format!("failed to run `{claude_bin}` resume command"))?;
    if !status.success() {
        bail!(
            "`{claude_bin}` resume command failed (status: {})",
            status
                .code()
                .map(|n| n.to_string())
                .unwrap_or_else(|| "signal".to_string())
        );
    }
    Ok(())
}

fn cmd_copilot(
    memory_dir: &Path,
    cwd: &Path,
    resume_only: bool,
    prompt: Option<String>,
) -> Result<()> {
    init_memory_scaffold(memory_dir)?;

    let copilot_bin = std::env::var("AMEM_COPILOT_BIN").unwrap_or_else(|_| "copilot".to_string());
    let mut seed_session_id: Option<String> = None;
    if !resume_only {
        let previous_share_files: HashSet<PathBuf> =
            collect_copilot_share_files(cwd)?.into_iter().collect();
        let bootstrap = copilot_bootstrap_prompt(memory_dir)?;
        let output = ProcessCommand::new(&copilot_bin)
            .current_dir(cwd)
            .arg("-p")
            .arg(bootstrap)
            .arg("--allow-all")
            .arg("--share")
            .output()
            .with_context(|| format!("failed to run `{copilot_bin}` seed prompt"))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            bail!(
                "`{copilot_bin}` seed failed (status: {}): {}{}",
                output
                    .status
                    .code()
                    .map(|n| n.to_string())
                    .unwrap_or_else(|| "signal".to_string()),
                stderr.trim(),
                if stderr.trim().is_empty() {
                    format!("\n{}", stdout.trim())
                } else {
                    String::new()
                }
            );
        }

        seed_session_id = extract_copilot_session_id_from_output(&output.stdout, &output.stderr);

        let new_share_files: Vec<PathBuf> = collect_copilot_share_files(cwd)?
            .into_iter()
            .filter(|p| !previous_share_files.contains(p))
            .collect();

        if seed_session_id.is_none() {
            for path in &new_share_files {
                if let Some(id) = extract_copilot_session_id_from_share_path(path) {
                    seed_session_id = Some(id);
                    break;
                }
            }
        }

        for path in new_share_files {
            let _ = fs::remove_file(path);
        }

        if seed_session_id.is_none() {
            bail!(
                "seed session was created but session_id was not found in Copilot output or share path; refusing to fallback to `--continue`"
            );
        }
    }

    let mut resume = ProcessCommand::new(&copilot_bin);
    resume.current_dir(cwd).arg("--allow-all");
    if resume_only {
        resume.arg("--continue");
    } else if let Some(session_id) = seed_session_id {
        resume.arg("--resume").arg(session_id);
    } else {
        bail!("internal error: missing Copilot seed session id");
    }
    if let Some(p) = prompt {
        resume.arg("-i").arg(p);
    }
    let status = resume
        .status()
        .with_context(|| format!("failed to run `{copilot_bin}` resume command"))?;
    if !status.success() {
        bail!(
            "`{copilot_bin}` resume command failed (status: {})",
            status
                .code()
                .map(|n| n.to_string())
                .unwrap_or_else(|| "signal".to_string())
        );
    }
    Ok(())
}

fn cmd_opencode(
    memory_dir: &Path,
    cwd: &Path,
    resume_only: bool,
    prompt: Option<String>,
) -> Result<()> {
    const DEFAULT_OPENCODE_PERMISSION: &str = r#"{"*":"allow"}"#;

    init_memory_scaffold(memory_dir)?;

    let opencode_bin =
        std::env::var("AMEM_OPENCODE_BIN").unwrap_or_else(|_| "opencode".to_string());
    let opencode_agent =
        std::env::var("AMEM_OPENCODE_AGENT").unwrap_or_else(|_| "build".to_string());
    let opencode_permission = std::env::var("AMEM_OPENCODE_PERMISSION")
        .ok()
        .or_else(|| std::env::var("OPENCODE_PERMISSION").ok())
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_OPENCODE_PERMISSION.to_string());
    let default_opencode_config_content = serde_json::json!({
        "agent": {
            opencode_agent.clone(): {
                "permission": {
                    "*": "allow"
                }
            }
        }
    })
    .to_string();
    let opencode_config_content = std::env::var("AMEM_OPENCODE_CONFIG_CONTENT")
        .ok()
        .or_else(|| std::env::var("OPENCODE_CONFIG_CONTENT").ok())
        .filter(|v| !v.trim().is_empty())
        .unwrap_or(default_opencode_config_content);
    let mut seed_session_id: Option<String> = None;
    if !resume_only {
        let bootstrap = opencode_bootstrap_prompt(memory_dir)?;
        let output = ProcessCommand::new(&opencode_bin)
            .current_dir(cwd)
            .env("OPENCODE_PERMISSION", &opencode_permission)
            .env("OPENCODE_CONFIG_CONTENT", &opencode_config_content)
            .arg("run")
            .arg("--agent")
            .arg(&opencode_agent)
            .arg("--format")
            .arg("json")
            .arg(bootstrap)
            .output()
            .with_context(|| format!("failed to run `{opencode_bin} run` seed prompt"))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            bail!(
                "`{opencode_bin} run` seed failed (status: {}): {}{}",
                output
                    .status
                    .code()
                    .map(|n| n.to_string())
                    .unwrap_or_else(|| "signal".to_string()),
                stderr.trim(),
                if stderr.trim().is_empty() {
                    format!("\n{}", stdout.trim())
                } else {
                    String::new()
                }
            );
        }

        seed_session_id = extract_opencode_session_id(&output.stdout, &output.stderr);
        if seed_session_id.is_none() {
            bail!(
                "seed session was created but sessionID was not found in OpenCode JSON output; refusing to fallback to `--continue`"
            );
        }
    }

    let mut resume = ProcessCommand::new(&opencode_bin);
    resume
        .current_dir(cwd)
        .env("OPENCODE_PERMISSION", &opencode_permission)
        .env("OPENCODE_CONFIG_CONTENT", &opencode_config_content)
        .arg("--agent")
        .arg(&opencode_agent);
    if resume_only {
        resume.arg("--continue");
    } else if let Some(session_id) = seed_session_id {
        resume.arg("--session").arg(session_id);
    } else {
        bail!("internal error: missing OpenCode seed session id");
    }
    if let Some(p) = prompt {
        resume.arg("--prompt").arg(p);
    }
    let status = resume
        .status()
        .with_context(|| format!("failed to run `{opencode_bin}` resume command"))?;
    if !status.success() {
        bail!(
            "`{opencode_bin}` resume command failed (status: {})",
            status
                .code()
                .map(|n| n.to_string())
                .unwrap_or_else(|| "signal".to_string())
        );
    }
    Ok(())
}

fn codex_bootstrap_prompt(memory_dir: &Path) -> Result<String> {
    let today = load_today(memory_dir, Local::now().date_naive());
    let snapshot_md = render_today_snapshot(&today);
    Ok(format!(
        "Load this amem snapshot for the next interactive session and reply exactly `MEMORY_READY`.\n\nmemory_root: {}\n\n{}\n",
        memory_dir.to_string_lossy(),
        snapshot_md
    ))
}

fn gemini_bootstrap_prompt(memory_dir: &Path) -> Result<String> {
    let today = load_today(memory_dir, Local::now().date_naive());
    let snapshot_md = render_today_snapshot(&today);
    Ok(format!(
        "Load this amem snapshot for the next interactive session. Reply exactly MEMORY_READY.\n\nmemory_root: {}\n\n{}\n",
        memory_dir.to_string_lossy(),
        snapshot_md
    ))
}

fn claude_bootstrap_prompt(memory_dir: &Path) -> Result<String> {
    let today = load_today(memory_dir, Local::now().date_naive());
    let snapshot_md = render_today_snapshot(&today);
    Ok(format!(
        "Load this amem snapshot for the next interactive session. Reply exactly MEMORY_READY.\n\nmemory_root: {}\n\n{}\n",
        memory_dir.to_string_lossy(),
        snapshot_md
    ))
}

fn copilot_bootstrap_prompt(memory_dir: &Path) -> Result<String> {
    let today = load_today(memory_dir, Local::now().date_naive());
    let snapshot_md = render_today_snapshot(&today);
    Ok(format!(
        "Load this amem snapshot for the next interactive session. Reply exactly MEMORY_READY.\n\nmemory_root: {}\n\n{}\n",
        memory_dir.to_string_lossy(),
        snapshot_md
    ))
}

fn opencode_bootstrap_prompt(memory_dir: &Path) -> Result<String> {
    let today = load_today(memory_dir, Local::now().date_naive());
    let snapshot_md = render_today_snapshot(&today);
    Ok(format!(
        "Load this amem snapshot for the next interactive session. Reply exactly MEMORY_READY.\n\nmemory_root: {}\n\n{}\n",
        memory_dir.to_string_lossy(),
        snapshot_md
    ))
}

fn extract_codex_thread_id(stdout: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(stdout);
    for line in text.lines() {
        let value: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let t = value.get("type").and_then(|v| v.as_str());
        if t == Some("thread.started") {
            let id = value.get("thread_id").and_then(|v| v.as_str());
            if let Some(id) = id {
                return Some(id.to_string());
            }
        }
    }
    None
}

fn extract_gemini_session_id(stdout: &[u8]) -> Option<String> {
    extract_string_field_from_json_output(stdout, &["session_id", "sessionId"])
}

fn extract_claude_session_id(stdout: &[u8]) -> Option<String> {
    extract_string_field_from_json_output(stdout, &["session_id", "sessionId"])
}

fn extract_copilot_session_id_from_output(stdout: &[u8], stderr: &[u8]) -> Option<String> {
    if let Some(id) = extract_string_field_from_json_output(stdout, &["session_id", "sessionId"]) {
        return Some(id);
    }
    if let Some(id) = extract_string_field_from_json_output(stderr, &["session_id", "sessionId"]) {
        return Some(id);
    }

    let text = format!(
        "{}\n{}",
        String::from_utf8_lossy(stdout),
        String::from_utf8_lossy(stderr)
    );
    for token in text.split_whitespace() {
        let cleaned = token.trim_matches(|c: char| {
            c == '"' || c == '\'' || c == '`' || c == ',' || c == ';' || c == '(' || c == ')'
        });
        if let Some(id) = extract_copilot_session_id_from_share_path(Path::new(cleaned)) {
            return Some(id);
        }
    }
    None
}

fn extract_opencode_session_id(stdout: &[u8], stderr: &[u8]) -> Option<String> {
    if let Some(id) =
        extract_string_field_from_json_output(stdout, &["session_id", "sessionId", "sessionID"])
    {
        return Some(id);
    }
    extract_string_field_from_json_output(stderr, &["session_id", "sessionId", "sessionID"])
}

fn collect_copilot_share_files(cwd: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in
        fs::read_dir(cwd).with_context(|| format!("failed to read {}", cwd.to_string_lossy()))?
    {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if !file_type.is_file() {
            continue;
        }
        let path = entry.path();
        if extract_copilot_session_id_from_share_path(&path).is_some() {
            files.push(path);
        }
    }
    Ok(files)
}

fn extract_copilot_session_id_from_share_path(path: &Path) -> Option<String> {
    const PREFIX: &str = "copilot-session-";
    const SUFFIX: &str = ".md";

    let file_name = path.file_name()?.to_str()?;
    if !file_name.starts_with(PREFIX) || !file_name.ends_with(SUFFIX) {
        return None;
    }

    let id = &file_name[PREFIX.len()..file_name.len() - SUFFIX.len()];
    if id.is_empty() {
        None
    } else {
        Some(id.to_string())
    }
}

fn extract_string_field_from_json_output(stdout: &[u8], keys: &[&str]) -> Option<String> {
    let text = String::from_utf8_lossy(stdout);
    let trimmed = text.trim();

    if let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) {
        if let Some(id) = find_string_field_recursive(&value, keys) {
            return Some(id);
        }
    }

    for line in text.lines() {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(line) {
            if let Some(id) = find_string_field_recursive(&value, keys) {
                return Some(id);
            }
        }
    }

    if let (Some(start), Some(end)) = (text.find('{'), text.rfind('}')) {
        let candidate = &text[start..=end];
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(candidate) {
            if let Some(id) = find_string_field_recursive(&value, keys) {
                return Some(id);
            }
        }
    }

    None
}

fn find_string_field_recursive(value: &serde_json::Value, keys: &[&str]) -> Option<String> {
    match value {
        serde_json::Value::Object(map) => {
            for key in keys {
                if let Some(id) = map.get(*key).and_then(|v| v.as_str()) {
                    return Some(id.to_string());
                }
            }
            for v in map.values() {
                if let Some(id) = find_string_field_recursive(v, keys) {
                    return Some(id);
                }
            }
            None
        }
        serde_json::Value::Array(items) => {
            for v in items {
                if let Some(id) = find_string_field_recursive(v, keys) {
                    return Some(id);
                }
            }
            None
        }
        _ => None,
    }
}

fn resolve_claude_bin() -> String {
    if let Ok(bin) = std::env::var("AMEM_CLAUDE_BIN") {
        if !bin.trim().is_empty() {
            return bin;
        }
    }
    if ProcessCommand::new("claude")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
    {
        return "claude".to_string();
    }
    if let Some(path) = find_asdf_claude_bin() {
        return path;
    }
    "claude".to_string()
}

fn find_asdf_claude_bin() -> Option<String> {
    let home = std::env::var("HOME").ok()?;
    let installs = PathBuf::from(home)
        .join(".asdf")
        .join("installs")
        .join("nodejs");
    let mut candidates: Vec<(Vec<u32>, String)> = Vec::new();

    for entry in fs::read_dir(installs).ok()? {
        let entry = entry.ok()?;
        let file_type = entry.file_type().ok()?;
        if !file_type.is_dir() {
            continue;
        }
        let version = entry.file_name().to_string_lossy().to_string();
        let bin = entry.path().join("bin").join("claude");
        if !bin.exists() {
            continue;
        }
        let key = version
            .split(|c: char| !c.is_ascii_digit())
            .filter(|s| !s.is_empty())
            .map(|s| s.parse::<u32>().unwrap_or(0))
            .collect::<Vec<_>>();
        candidates.push((key, bin.to_string_lossy().to_string()));
    }

    if candidates.is_empty() {
        return None;
    }
    candidates.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
    candidates.pop().map(|(_, path)| path)
}

fn load_today(memory_dir: &Path, date: NaiveDate) -> TodayJson {
    let (memories_content, memories_paths) = read_agent_memories(memory_dir);
    let owner_diary_recent = load_recent_owner_diary_sections(memory_dir, date);
    let activity_recent = load_recent_activity_sections(memory_dir, date);
    TodayJson {
        date: date.to_string(),
        agent_identity: read_body_or_empty(memory_dir.join("agent").join("IDENTITY.md")),
        agent_identity_path: memory_dir
            .join("agent")
            .join("IDENTITY.md")
            .to_string_lossy()
            .to_string(),
        agent_soul: read_body_or_empty(memory_dir.join("agent").join("SOUL.md")),
        agent_soul_path: memory_dir
            .join("agent")
            .join("SOUL.md")
            .to_string_lossy()
            .to_string(),
        owner_profile: read_body_or_empty(memory_dir.join("owner").join("profile.md")),
        owner_profile_path: memory_dir
            .join("owner")
            .join("profile.md")
            .to_string_lossy()
            .to_string(),
        owner_preferences: read_body_or_empty(memory_dir.join("owner").join("preferences.md")),
        owner_preferences_path: memory_dir
            .join("owner")
            .join("preferences.md")
            .to_string_lossy()
            .to_string(),
        owner_diary: read_daily_owner_diary(memory_dir, date),
        owner_diary_path: owner_diary_path(memory_dir, date)
            .to_string_lossy()
            .to_string(),
        owner_diary_paths: flatten_recent_section_paths(&owner_diary_recent),
        owner_diary_recent,
        open_tasks: read_open_tasks_summary(memory_dir),
        open_tasks_paths: open_task_paths(memory_dir)
            .into_iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect(),
        activity: read_daily_activity_summary(memory_dir, date),
        activity_paths: flatten_recent_section_paths(&activity_recent),
        activity_recent,
        agent_memories: memories_content,
        agent_memories_paths: memories_paths,
    }
}

fn render_today_snapshot(today: &TodayJson) -> String {
    let mut sections = Vec::new();

    if !today.agent_identity.is_empty() {
        sections.push(format!(
            "== Agent Identity ==\n[{}]\n{}",
            today.agent_identity_path, today.agent_identity
        ));
    }
    if !today.agent_soul.is_empty() {
        sections.push(format!(
            "== Agent Soul ==\n[{}]\n{}",
            today.agent_soul_path, today.agent_soul
        ));
    }

    if !today.agent_memories.is_empty() {
        let memories_paths = today
            .agent_memories_paths
            .iter()
            .filter(|p| Path::new(p).exists())
            .map(|p| format!("[{p}]"))
            .collect::<Vec<_>>()
            .join("\n");
        sections.push(format!(
            "== Agent Memories ==\n{}\n{}",
            if memories_paths.is_empty() {
                String::new()
            } else {
                format!("{}\n", memories_paths)
            },
            format!(
                "{}\n\n_Use `amem set memory` command to keep your own memory._",
                today.agent_memories
            )
        ));
    } else {
        sections.push(format!(
            "== Agent Memories ==\n(none)\n\n_Use `amem set memory` command to keep your own memory._"
        ));
    }

    sections.push(format!(
        "== Owner Profile ==\n[{}]\n{}",
        today.owner_profile_path,
        empty_as_na(&today.owner_profile)
    ));

    if has_meaningful_owner_preferences(&today.owner_preferences) {
        sections.push(format!(
            "== Owner Preferences ==\n[{}]\n{}",
            today.owner_preferences_path,
            empty_as_na(&today.owner_preferences)
        ));
    }

    sections.push(format!(
        "== Owner Diary ==\n{}",
        render_recent_daily_sections(&today.owner_diary_recent)
    ));

    let tasks_paths = today
        .open_tasks_paths
        .iter()
        .filter(|p| Path::new(p).exists())
        .map(|p| format!("[{p}]"))
        .collect::<Vec<_>>()
        .join("\n");
    sections.push(format!(
        "== Agent Tasks ==\n{}\n{}",
        if tasks_paths.is_empty() {
            String::new()
        } else {
            format!("{}\n", tasks_paths)
        },
        empty_as_na(&today.open_tasks)
    ));

    sections.push(format!(
        "== Agent Activities ==\n{}",
        render_recent_daily_sections(&today.activity_recent)
    ));

    sections.join("\n\n")
}

fn flatten_recent_section_paths(entries: &[RecentDailySection]) -> Vec<String> {
    entries
        .iter()
        .flat_map(|entry| entry.paths.iter().cloned())
        .collect()
}

fn render_recent_daily_sections(entries: &[RecentDailySection]) -> String {
    if entries.is_empty() {
        return "(none)".to_string();
    }

    entries
        .iter()
        .map(|entry| {
            let paths = entry
                .paths
                .iter()
                .filter(|p| Path::new(p).exists())
                .map(|p| format!("[{p}]"))
                .collect::<Vec<_>>()
                .join("\n");
            if paths.is_empty() {
                format!("### {}\n{}", entry.date, entry.content)
            } else {
                format!("### {}\n{}\n{}", entry.date, paths, entry.content)
            }
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn has_meaningful_owner_preferences(content: &str) -> bool {
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if trimmed == "-" || trimmed == "*" {
            continue;
        }
        return true;
    }
    false
}

fn parse_or_today(raw: Option<&str>) -> Result<NaiveDate> {
    match raw {
        Some(s) => Ok(NaiveDate::parse_from_str(s, "%Y-%m-%d")
            .with_context(|| format!("invalid date format: {s}, expected yyyy-mm-dd"))?),
        None => Ok(Local::now().date_naive()),
    }
}

fn parse_or_now_time(raw: Option<&str>) -> Result<String> {
    match raw {
        Some(s) => Ok(NaiveTime::parse_from_str(s, "%H:%M")
            .with_context(|| format!("invalid time format: {s}, expected HH:MM (24-hour)"))?
            .format("%H:%M")
            .to_string()),
        None => Ok(Local::now().format("%H:%M").to_string()),
    }
}

fn activity_path(memory_dir: &Path, date: NaiveDate) -> PathBuf {
    agent_activity_path(memory_dir, date)
}

fn agent_activity_path(memory_dir: &Path, date: NaiveDate) -> PathBuf {
    memory_dir
        .join("agent")
        .join("activity")
        .join(format!("{:04}", date.year()))
        .join(format!("{:02}", date.month()))
        .join(format!(
            "{:04}-{:02}-{:02}.md",
            date.year(),
            date.month(),
            date.day()
        ))
}

fn legacy_activity_path(memory_dir: &Path, date: NaiveDate) -> PathBuf {
    memory_dir
        .join("activity")
        .join(format!("{:04}", date.year()))
        .join(format!("{:02}", date.month()))
        .join(format!(
            "{:04}-{:02}-{:02}.md",
            date.year(),
            date.month(),
            date.day()
        ))
}

fn owner_diary_path(memory_dir: &Path, date: NaiveDate) -> PathBuf {
    memory_dir
        .join("owner")
        .join("diary")
        .join(format!("{:04}", date.year()))
        .join(format!("{:02}", date.month()))
        .join(format!(
            "{:04}-{:02}-{:02}.md",
            date.year(),
            date.month(),
            date.day()
        ))
}

fn agent_tasks_open_path(memory_dir: &Path) -> PathBuf {
    memory_dir.join("agent").join("tasks").join("open.md")
}

fn legacy_tasks_open_path(memory_dir: &Path) -> PathBuf {
    memory_dir.join("tasks").join("open.md")
}

fn agent_tasks_done_path(memory_dir: &Path) -> PathBuf {
    memory_dir.join("agent").join("tasks").join("done.md")
}

fn legacy_tasks_done_path(memory_dir: &Path) -> PathBuf {
    memory_dir.join("tasks").join("done.md")
}

fn open_task_paths(memory_dir: &Path) -> Vec<PathBuf> {
    vec![
        agent_tasks_open_path(memory_dir),
        legacy_tasks_open_path(memory_dir),
    ]
}

fn done_task_paths(memory_dir: &Path) -> Vec<PathBuf> {
    vec![
        agent_tasks_done_path(memory_dir),
        legacy_tasks_done_path(memory_dir),
    ]
}

fn agent_inbox_captured_path(memory_dir: &Path) -> PathBuf {
    memory_dir.join("agent").join("inbox").join("captured.md")
}

fn read_open_tasks_summary(memory_dir: &Path) -> String {
    let mut lines = Vec::new();
    for path in open_task_paths(memory_dir) {
        if let Ok(content) = fs::read_to_string(path) {
            for line in content.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with("- ") {
                    lines.push(trimmed.to_string());
                }
            }
        }
    }
    dedup_keep_order(lines).join("\n")
}

fn read_daily_activity_summary(memory_dir: &Path, date: NaiveDate) -> String {
    let mut lines = Vec::new();
    for path in [
        agent_activity_path(memory_dir, date),
        legacy_activity_path(memory_dir, date),
    ] {
        if let Ok(content) = fs::read_to_string(path) {
            let (_, body) = parse_daily_frontmatter_and_body(&content);
            for line in body.lines() {
                let trimmed = line.trim();
                if !trimmed.is_empty() {
                    lines.push(trimmed.to_string());
                }
            }
        }
    }
    dedup_keep_order(lines).join("\n")
}

fn recent_snapshot_dates(date: NaiveDate) -> [NaiveDate; 2] {
    [date, date - Duration::days(1)]
}

fn load_recent_owner_diary_sections(memory_dir: &Path, date: NaiveDate) -> Vec<RecentDailySection> {
    recent_snapshot_dates(date)
        .into_iter()
        .filter_map(|entry_date| {
            let path = owner_diary_path(memory_dir, entry_date);
            let content = read_daily_owner_diary(memory_dir, entry_date);
            if content.is_empty() {
                return None;
            }
            let mut paths = Vec::new();
            if path.exists() {
                paths.push(path.to_string_lossy().to_string());
            }
            Some(RecentDailySection {
                date: entry_date.to_string(),
                paths,
                content,
            })
        })
        .collect()
}

fn load_recent_activity_sections(memory_dir: &Path, date: NaiveDate) -> Vec<RecentDailySection> {
    recent_snapshot_dates(date)
        .into_iter()
        .filter_map(|entry_date| {
            let content = read_daily_activity_summary(memory_dir, entry_date);
            if content.is_empty() {
                return None;
            }
            let paths = [
                agent_activity_path(memory_dir, entry_date),
                legacy_activity_path(memory_dir, entry_date),
            ]
            .into_iter()
            .filter(|path| path.exists())
            .map(|path| path.to_string_lossy().to_string())
            .collect();
            Some(RecentDailySection {
                date: entry_date.to_string(),
                paths,
                content,
            })
        })
        .collect()
}

fn read_daily_owner_diary(memory_dir: &Path, date: NaiveDate) -> String {
    let path = owner_diary_path(memory_dir, date);
    let content = fs::read_to_string(path).unwrap_or_default();
    let (_, body) = parse_daily_frontmatter_and_body(&content);
    body.trim().to_string()
}

fn read_agent_memories(memory_dir: &Path) -> (String, Vec<String>) {
    let mut all_content = Vec::new();
    let mut all_paths = Vec::new();

    let p0_dir = memory_dir.join("agent").join("memory").join("P0");
    if let Ok(entries) = fs::read_dir(p0_dir) {
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }
            if let Ok(content) = fs::read_to_string(&path) {
                let (_, body) = parse_daily_frontmatter_and_body(&content);
                let trimmed = body.trim();
                if !trimmed.is_empty() {
                    all_content.push(format!(
                        "### {}\n{}",
                        path.file_name().unwrap().to_string_lossy(),
                        trimmed
                    ));
                    all_paths.push(path.to_string_lossy().to_string());
                }
            }
        }
    }

    (all_content.join("\n\n"), all_paths)
}

fn dedup_keep_order(lines: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for line in lines {
        if seen.insert(line.clone()) {
            out.push(line);
        }
    }
    out
}

fn ensure_parent(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.to_string_lossy()))?;
    }
    Ok(())
}

fn read_or_empty(path: PathBuf) -> String {
    fs::read_to_string(path)
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn read_body_or_empty(path: PathBuf) -> String {
    let content = fs::read_to_string(path).unwrap_or_default();
    let (_, body) = parse_daily_frontmatter_and_body(&content);
    body.trim().to_string()
}

fn empty_as_na(s: &str) -> String {
    if s.trim().is_empty() {
        "(none)".to_string()
    } else {
        s.to_string()
    }
}

fn memory_files(memory_dir: &Path) -> Result<Vec<PathBuf>> {
    if !memory_dir.exists() {
        return Ok(Vec::new());
    }
    let mut files = Vec::new();
    for entry in WalkDir::new(memory_dir).into_iter().filter_map(|e| e.ok()) {
        if !entry.file_type().is_file() {
            continue;
        }
        let abs = entry.path();
        let rel = match abs.strip_prefix(memory_dir) {
            Ok(p) => p,
            Err(_) => continue,
        };
        let rel_str = rel.to_string_lossy();
        if rel_str.starts_with(".index/") {
            continue;
        }
        if abs.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        files.push(rel.to_path_buf());
    }
    Ok(files)
}

fn load_docs(memory_dir: &Path) -> Result<Vec<(PathBuf, String)>> {
    let mut docs = Vec::new();
    for rel in memory_files(memory_dir)? {
        let abs = memory_dir.join(&rel);
        if let Ok(content) = fs::read_to_string(&abs) {
            docs.push((rel, content));
        }
    }
    Ok(docs)
}

fn search_hits(memory_dir: &Path, query: &str, top_k: usize) -> Result<Vec<SearchHit>> {
    if let Some(index_hits) = search_hits_from_index(memory_dir, query, top_k)? {
        return Ok(index_hits);
    }
    search_hits_from_files(memory_dir, query, top_k)
}

fn search_hits_from_files(memory_dir: &Path, query: &str, top_k: usize) -> Result<Vec<SearchHit>> {
    let docs = load_docs(memory_dir)?;
    let query_chars = query_chars(query);
    let n_docs = docs.len().max(1) as f64;

    let mut df: HashMap<char, usize> = HashMap::new();
    for (_, content) in &docs {
        for c in &query_chars {
            if content.contains(*c) {
                *df.entry(*c).or_insert(0) += 1;
            }
        }
    }

    let mut hits = Vec::new();
    for (path, content) in docs {
        let mut score = 0.0f64;
        for c in &query_chars {
            let tf = content.chars().filter(|x| x == c).count() as f64;
            if tf <= 0.0 {
                continue;
            }
            let d = *df.get(c).unwrap_or(&0) as f64;
            let idf = ((n_docs + 1.0) / (d + 1.0)).ln() + 1.0;
            score += tf * idf;
        }
        if content.contains(query) {
            score += 5.0;
        }
        if score > 0.0 {
            let snippet = content
                .lines()
                .find(|l| l.contains(query))
                .unwrap_or_else(|| content.lines().next().unwrap_or(""))
                .trim()
                .to_string();
            hits.push(SearchHit {
                path: path.to_string_lossy().to_string(),
                score,
                snippet,
            });
        }
    }
    hits.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.path.cmp(&b.path))
    });
    hits.truncate(top_k);
    Ok(hits)
}

fn search_hits_from_index(
    memory_dir: &Path,
    query: &str,
    top_k: usize,
) -> Result<Option<Vec<SearchHit>>> {
    let index_db = memory_dir.join(".index").join("index.db");
    if !index_db.exists() {
        return Ok(None);
    }

    let conn = match Connection::open(&index_db) {
        Ok(c) => c,
        Err(_) => return Ok(None),
    };

    let n_chunks: i64 = match conn.query_row("SELECT COUNT(*) FROM chunks", [], |r| r.get(0)) {
        Ok(v) => v,
        Err(_) => return Ok(None),
    };
    if n_chunks == 0 {
        return Ok(Some(Vec::new()));
    }

    let tokens = query_tokens(query);
    if tokens.is_empty() {
        return Ok(Some(Vec::new()));
    }

    let placeholders = vec!["?"; tokens.len()].join(", ");
    let df_sql = format!(
        "SELECT token, df FROM token_stats WHERE token IN ({})",
        placeholders
    );
    let mut df_stmt = match conn.prepare(&df_sql) {
        Ok(s) => s,
        Err(_) => return Ok(None),
    };
    let mut df_rows = df_stmt.query(params_from_iter(tokens.iter()))?;
    let mut df_map: HashMap<String, i64> = HashMap::new();
    while let Some(row) = df_rows.next()? {
        let token: String = row.get(0)?;
        let df: i64 = row.get(1)?;
        df_map.insert(token, df);
    }
    drop(df_rows);
    drop(df_stmt);

    if df_map.is_empty() {
        return Ok(Some(Vec::new()));
    }

    let postings_sql = format!(
        "SELECT p.token, p.tf, c.path, c.chunk_text \
         FROM postings p \
         JOIN chunks c ON c.id = p.chunk_id \
         WHERE p.token IN ({})",
        placeholders
    );
    let mut stmt = match conn.prepare(&postings_sql) {
        Ok(s) => s,
        Err(_) => return Ok(None),
    };
    let mut rows = stmt.query(params_from_iter(tokens.iter()))?;

    #[derive(Default)]
    struct Acc {
        score: f64,
        snippet: String,
        bonus_applied: bool,
    }

    let mut acc: HashMap<String, Acc> = HashMap::new();
    let n_chunks_f = n_chunks as f64;
    while let Some(row) = rows.next()? {
        let token: String = row.get(0)?;
        let tf: i64 = row.get(1)?;
        let path: String = row.get(2)?;
        let chunk_text: String = row.get(3)?;

        let df = *df_map.get(&token).unwrap_or(&0) as f64;
        let idf = ((n_chunks_f + 1.0) / (df + 1.0)).ln() + 1.0;
        let entry = acc.entry(path).or_default();
        entry.score += (tf as f64) * idf;
        if entry.snippet.is_empty() {
            entry.snippet = chunk_text.lines().next().unwrap_or("").trim().to_string();
        }
        if !entry.bonus_applied && chunk_text.contains(query) {
            entry.score += 5.0;
            entry.bonus_applied = true;
            if let Some(line) = chunk_text.lines().find(|l| l.contains(query)) {
                entry.snippet = line.trim().to_string();
            }
        }
    }

    let mut hits: Vec<SearchHit> = acc
        .into_iter()
        .filter_map(|(path, v)| {
            if v.score > 0.0 {
                Some(SearchHit {
                    path,
                    score: v.score,
                    snippet: v.snippet,
                })
            } else {
                None
            }
        })
        .collect();

    hits.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.path.cmp(&b.path))
    });
    hits.truncate(top_k);
    Ok(Some(hits))
}

fn query_chars(query: &str) -> Vec<char> {
    let mut seen = HashSet::new();
    query
        .chars()
        .filter(|c| !c.is_whitespace())
        .filter(|c| seen.insert(*c))
        .collect()
}

fn query_tokens(query: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    query
        .chars()
        .filter(|c| !c.is_whitespace())
        .map(|c| c.to_string())
        .filter(|t| seen.insert(t.clone()))
        .collect()
}

fn unigram_freqs(text: &str) -> HashMap<String, i64> {
    let mut out = HashMap::new();
    for c in text.chars().filter(|c| !c.is_whitespace()) {
        *out.entry(c.to_string()).or_insert(0) += 1;
    }
    out
}

fn rel_or_abs(memory_dir: &Path, target: &Path) -> String {
    target
        .strip_prefix(memory_dir)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| target.to_string_lossy().to_string())
}
