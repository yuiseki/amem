use anyhow::{Context, Result, bail};
use chrono::{Datelike, Local, NaiveDate};
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
use std::process::Command as ProcessCommand;
use std::time::UNIX_EPOCH;
use walkdir::WalkDir;

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
    #[command(visible_alias = "remember")]
    Search {
        query: String,
        #[arg(short = 'k', long, default_value_t = 8)]
        top_k: usize,
        #[arg(long, default_value_t = false)]
        lexical_only: bool,
        #[arg(long, default_value_t = false)]
        semantic_only: bool,
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
    owner_profile: String,
    owner_preferences: String,
    open_tasks: String,
    activity: String,
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
        memory_dir.join("tasks"),
        memory_dir.join("inbox"),
        memory_dir.join("activity"),
    ];
    for dir in directories {
        fs::create_dir_all(&dir)
            .with_context(|| format!("failed to create {}", dir.to_string_lossy()))?;
    }

    let files = [
        (
            memory_dir.join("owner").join("profile.md"),
            "# Owner Profile\n\nname: \nlocation: \noccupation: \n",
        ),
        (
            memory_dir.join("owner").join("personality.md"),
            "# Owner Personality\n\n- \n",
        ),
        (
            memory_dir.join("owner").join("preferences.md"),
            "# Owner Preferences\n\n- \n",
        ),
        (
            memory_dir.join("owner").join("interests.md"),
            "# Owner Interests\n\n- \n",
        ),
        (memory_dir.join("tasks").join("open.md"), "# Open Tasks\n\n"),
        (memory_dir.join("tasks").join("done.md"), "# Done Tasks\n\n"),
        (
            memory_dir.join("inbox").join("captured.md"),
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
            let p = memory_dir.join("inbox").join("captured.md");
            ensure_parent(&p)?;
            p
        }
        "task-note" => {
            let p = memory_dir.join("tasks").join("open.md");
            ensure_parent(&p)?;
            p
        }
        other => bail!("unsupported kind: {other}"),
    };

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&target)
        .with_context(|| format!("failed to open {}", target.to_string_lossy()))?;

    let line = format!("- {} [{}] {}\n", now.format("%H:%M"), source, text.trim());
    file.write_all(line.as_bytes())
        .with_context(|| format!("failed to write {}", target.to_string_lossy()))?;

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
    Ok(())
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
                    "activity" => s.starts_with("activity/"),
                    "tasks" => s.starts_with("tasks/"),
                    "inbox" => s.starts_with("inbox/"),
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
    let today = TodayJson {
        date: d.to_string(),
        owner_profile: read_or_empty(memory_dir.join("owner").join("profile.md")),
        owner_preferences: read_or_empty(memory_dir.join("owner").join("preferences.md")),
        open_tasks: read_or_empty(memory_dir.join("tasks").join("open.md")),
        activity: read_or_empty(activity_path(memory_dir, d)),
    };
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
        "\n== Today Snapshot ==\nOpen Tasks:\n{}",
        empty_as_na(&today.open_tasks)
    );
    println!("\nActivity:\n{}", empty_as_na(&today.activity));
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
            .arg("--sandbox")
            .arg("read-only")
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
    resume.current_dir(cwd).arg("--resume");
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
    resume.current_dir(cwd);
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
            .arg("--allow-all-tools")
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
    resume.current_dir(cwd);
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
    TodayJson {
        date: date.to_string(),
        owner_profile: read_or_empty(memory_dir.join("owner").join("profile.md")),
        owner_preferences: read_or_empty(memory_dir.join("owner").join("preferences.md")),
        open_tasks: read_or_empty(memory_dir.join("tasks").join("open.md")),
        activity: read_or_empty(activity_path(memory_dir, date)),
    }
}

fn render_today_snapshot(today: &TodayJson) -> String {
    format!(
        "Today Snapshot ({})\n\n== Owner Profile ==\n{}\n\n== Owner Preferences ==\n{}\n\n== Open Tasks ==\n{}\n\n== Activity ==\n{}",
        today.date,
        empty_as_na(&today.owner_profile),
        empty_as_na(&today.owner_preferences),
        empty_as_na(&today.open_tasks),
        empty_as_na(&today.activity),
    )
}

fn parse_or_today(raw: Option<&str>) -> Result<NaiveDate> {
    match raw {
        Some(s) => Ok(NaiveDate::parse_from_str(s, "%Y-%m-%d")
            .with_context(|| format!("invalid date format: {s}, expected yyyy-mm-dd"))?),
        None => Ok(Local::now().date_naive()),
    }
}

fn activity_path(memory_dir: &Path, date: NaiveDate) -> PathBuf {
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
