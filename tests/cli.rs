use assert_cmd::Command;
use assert_fs::prelude::*;
use chrono::{Duration, Local};
use predicates::prelude::*;
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_amem"))
}

fn set_test_home(cmd: &mut Command, home: &std::path::Path) {
    cmd.env("HOME", home);
    #[cfg(windows)]
    {
        cmd.env("USERPROFILE", home);
    }
}

#[test]
fn init_creates_memory_scaffold() {
    let tmp = assert_fs::TempDir::new().unwrap();
    let mut cmd = bin();
    set_test_home(&mut cmd, tmp.path());
    cmd.current_dir(tmp.path()).arg("init");
    cmd.assert().success();

    tmp.child(".amem/owner/profile.md")
        .assert(predicate::path::exists());
    tmp.child(".amem/owner/profile.md")
        .assert(predicate::str::contains("github_username: "));
    tmp.child(".amem/owner/profile.md")
        .assert(predicate::str::contains("- **Language:** "));
    tmp.child(".amem/owner/personality.md")
        .assert(predicate::path::exists());
    tmp.child(".amem/owner/preferences.md")
        .assert(predicate::path::exists());
    tmp.child(".amem/owner/interests.md")
        .assert(predicate::path::exists());
    tmp.child(".amem/agent/IDENTITY.md")
        .assert(predicate::path::exists());
    tmp.child(".amem/agent/SOUL.md")
        .assert(predicate::path::exists());
    tmp.child(".amem/agent/tasks/open.md")
        .assert(predicate::path::exists());
    tmp.child(".amem/agent/tasks/done.md")
        .assert(predicate::path::exists());
    tmp.child(".amem/agent/inbox/captured.md")
        .assert(predicate::path::exists());
    tmp.child(".amem/agent/activity")
        .assert(predicate::path::is_dir());
    tmp.child(".amem/owner/diary")
        .assert(predicate::path::is_dir());
}

#[test]
fn init_is_idempotent_and_does_not_overwrite_existing_files() {
    let tmp = assert_fs::TempDir::new().unwrap();
    let profile = tmp.child(".amem/owner/profile.md");
    profile.write_str("name: custom\n").unwrap();

    let mut cmd = bin();
    set_test_home(&mut cmd, tmp.path());
    cmd.current_dir(tmp.path()).arg("init");
    cmd.assert().success();

    profile.assert("name: custom\n");
}

#[test]
fn which_prints_resolved_memory_dir() {
    let tmp = assert_fs::TempDir::new().unwrap();
    let memory = tmp.path().join(".amem-custom");

    let mut cmd = bin();
    cmd.current_dir(tmp.path())
        .arg("--memory-dir")
        .arg(&memory)
        .arg("which");

    cmd.assert().success().stdout(predicate::str::contains(
        memory.to_string_lossy().to_string(),
    ));
}

#[test]
fn which_defaults_to_home_dot_amem() {
    let tmp = assert_fs::TempDir::new().unwrap();
    let home = tmp.child("home");
    home.create_dir_all().unwrap();
    let work = tmp.child("work");
    work.create_dir_all().unwrap();
    let expected = home.path().join(".amem");

    let mut cmd = bin();
    set_test_home(&mut cmd, home.path());
    cmd.current_dir(work.path()).arg("which");
    cmd.assert().success().stdout(predicate::str::contains(
        expected.to_string_lossy().to_string(),
    ));
}

#[test]
fn keep_appends_to_activity_log() {
    let tmp = assert_fs::TempDir::new().unwrap();
    let memory = tmp.path().join(".amem");

    let mut cmd = bin();
    cmd.current_dir(tmp.path())
        .arg("--memory-dir")
        .arg(&memory)
        .arg("keep")
        .arg("Went for a walk")
        .arg("--date")
        .arg("2026-02-21");

    cmd.assert().success();

    let activity = tmp.child(".amem/agent/activity/2026/02/2026-02-21.md");
    activity.assert(predicate::path::exists());
    activity.assert(predicate::str::starts_with("---\nsummary: "));
    activity.assert(predicate::str::contains("Went for a walk"));
}

#[cfg(unix)]
#[test]
fn keep_notifies_discord_via_acomm_when_discord_env_is_enabled() {
    let tmp = assert_fs::TempDir::new().unwrap();
    let memory = tmp.path().join(".amem");
    let bin_dir = tmp.child("bin");
    bin_dir.create_dir_all().unwrap();
    let log_path = tmp.child("acomm-args.log");
    let fake_acomm = bin_dir.child("acomm");
    fake_acomm
        .write_str(
            r#"#!/bin/sh
printf '%s\n' "$@" > "$ACOMM_ARGS_LOG"
printf 'acomm fake stdout\n'
printf 'acomm fake stderr\n' >&2
"#,
        )
        .unwrap();
    let mut perms = fs::metadata(fake_acomm.path()).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(fake_acomm.path(), perms).unwrap();

    let path_env = match std::env::var("PATH") {
        Ok(existing) if !existing.is_empty() => {
            format!("{}:{}", bin_dir.path().display(), existing)
        }
        _ => bin_dir.path().display().to_string(),
    };

    let mut cmd = bin();
    cmd.current_dir(tmp.path())
        .arg("--memory-dir")
        .arg(&memory)
        .arg("keep")
        .arg("Went for a walk")
        .arg("--date")
        .arg("2026-02-21")
        .env("PATH", path_env)
        .env("DISCORD_BOT_TOKEN", "dummy-token")
        .env("DISCORD_NOTIFY_CHANNEL_ID", "123456789")
        .env("ACOMM_ARGS_LOG", log_path.path());

    cmd.assert()
        .success()
        .stdout(predicate::str::contains(
            "agent/activity/2026/02/2026-02-21.md",
        ))
        .stdout(predicate::str::contains("acomm fake stdout").not())
        .stderr(predicate::str::contains("acomm fake stderr").not());

    let mut ready = false;
    for _ in 0..20 {
        if log_path.path().exists() {
            ready = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    assert!(ready, "fake acomm log was not created in time");
    let logged = fs::read_to_string(log_path.path()).unwrap();
    assert!(
        logged.contains("--discord\n--agent\nWent for a walk\n"),
        "expected fake acomm to receive '--discord --agent <text>', got: {logged}"
    );
}

#[test]
fn list_and_ls_alias_work() {
    let tmp = assert_fs::TempDir::new().unwrap();
    tmp.child(".amem/owner/profile.md")
        .write_str("# profile\n")
        .unwrap();
    tmp.child(".amem/agent/tasks/open.md")
        .write_str("- task\n")
        .unwrap();

    let mut list = bin();
    set_test_home(&mut list, tmp.path());
    list.current_dir(tmp.path()).arg("list");
    list.assert()
        .success()
        .stdout(predicate::str::contains("owner/profile.md"))
        .stdout(predicate::str::contains("agent/tasks/open.md"));

    let mut ls = bin();
    set_test_home(&mut ls, tmp.path());
    ls.current_dir(tmp.path()).arg("ls");
    ls.assert()
        .success()
        .stdout(predicate::str::contains("owner/profile.md"))
        .stdout(predicate::str::contains("agent/tasks/open.md"));
}

#[test]
fn search_and_remember_work() {
    let tmp = assert_fs::TempDir::new().unwrap();
    tmp.child(".amem/agent/activity/2026/02/2026-02-21.md")
        .write_str("東京で散歩した\n")
        .unwrap();
    tmp.child(".amem/agent/activity/2026/02/2026-02-20.md")
        .write_str("大阪で会議した\n")
        .unwrap();
    tmp.child(".amem/agent/memory/P1/tokyo.md")
        .write_str("東京のメモ\n")
        .unwrap();

    let mut search = bin();
    set_test_home(&mut search, tmp.path());
    search
        .current_dir(tmp.path())
        .arg("search")
        .arg("東京")
        .arg("--top-k")
        .arg("1");
    search
        .assert()
        .success()
        .stdout(predicate::str::contains("2026-02-21.md"));

    let mut remember = bin();
    set_test_home(&mut remember, tmp.path());
    remember
        .current_dir(tmp.path())
        .arg("remember")
        .arg("--query")
        .arg("東京");
    remember
        .assert()
        .success()
        .stdout(predicate::str::contains("== P1 (tokyo.md) =="))
        .stdout(predicate::str::contains("東京のメモ"));
}

#[test]
fn default_command_runs_today_and_includes_yesterday_daily_sections() {
    let tmp = assert_fs::TempDir::new().unwrap();
    let today = Local::now().date_naive();
    let yesterday = today.pred_opt().unwrap();
    let yyyy = today.format("%Y").to_string();
    let mm = today.format("%m").to_string();
    let ymd = today.format("%Y-%m-%d").to_string();
    let y_yyyy = yesterday.format("%Y").to_string();
    let y_mm = yesterday.format("%m").to_string();
    let y_ymd = yesterday.format("%Y-%m-%d").to_string();

    tmp.child(".amem/owner/profile.md")
        .write_str("name: yuiseki\n")
        .unwrap();
    tmp.child(format!(".amem/owner/diary/{yyyy}/{mm}/{ymd}.md"))
        .write_str("- 09:50 today diary entry\n")
        .unwrap();
    tmp.child(format!(".amem/owner/diary/{y_yyyy}/{y_mm}/{y_ymd}.md"))
        .write_str("- 08:40 yesterday diary entry\n")
        .unwrap();
    tmp.child(".amem/agent/tasks/open.md")
        .write_str("- finish amem\n")
        .unwrap();
    tmp.child(format!(".amem/agent/activity/{yyyy}/{mm}/{ymd}.md"))
        .write_str("- 10:00 [codex] today activity entry\n")
        .unwrap();
    tmp.child(format!(".amem/agent/activity/{y_yyyy}/{y_mm}/{y_ymd}.md"))
        .write_str("- 08:30 [codex] yesterday activity entry\n")
        .unwrap();

    let mut cmd = bin();
    set_test_home(&mut cmd, tmp.path());
    cmd.current_dir(tmp.path());
    let assert = cmd
        .assert()
        .success()
        .stdout(predicate::str::contains("== Owner Diary =="))
        .stdout(predicate::str::contains("today diary entry"))
        .stdout(predicate::str::contains("yesterday diary entry"))
        .stdout(predicate::str::contains("== Agent Tasks =="))
        .stdout(predicate::str::contains("== Agent Activities =="))
        .stdout(predicate::str::contains("== Owner Preferences ==").not())
        .stdout(predicate::str::contains("finish amem"));
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    assert!(
        stdout.find("today diary entry").unwrap() < stdout.find("yesterday diary entry").unwrap()
    );
    assert!(
        stdout.find("today activity entry").unwrap()
            < stdout.find("yesterday activity entry").unwrap()
    );
}

#[test]
fn today_json_includes_yesterday_daily_sections() {
    let tmp = assert_fs::TempDir::new().unwrap();
    let today = Local::now().date_naive();
    let yesterday = today.pred_opt().unwrap();
    let yyyy = today.format("%Y").to_string();
    let mm = today.format("%m").to_string();
    let ymd = today.format("%Y-%m-%d").to_string();
    let y_yyyy = yesterday.format("%Y").to_string();
    let y_mm = yesterday.format("%m").to_string();
    let y_ymd = yesterday.format("%Y-%m-%d").to_string();

    tmp.child(".amem/owner/profile.md")
        .write_str("name: yuiseki\n")
        .unwrap();
    tmp.child(format!(".amem/owner/diary/{yyyy}/{mm}/{ymd}.md"))
        .write_str("- 09:50 today diary entry\n")
        .unwrap();
    tmp.child(format!(".amem/owner/diary/{y_yyyy}/{y_mm}/{y_ymd}.md"))
        .write_str("- 08:40 yesterday diary entry\n")
        .unwrap();
    tmp.child(format!(".amem/agent/activity/{yyyy}/{mm}/{ymd}.md"))
        .write_str("- 10:00 [codex] today activity entry\n")
        .unwrap();
    tmp.child(format!(".amem/agent/activity/{y_yyyy}/{y_mm}/{y_ymd}.md"))
        .write_str("- 08:30 [codex] yesterday activity entry\n")
        .unwrap();

    let mut cmd = bin();
    set_test_home(&mut cmd, tmp.path());
    cmd.current_dir(tmp.path()).arg("today").arg("--json");
    let assert = cmd.assert().success();
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();

    assert_eq!(json["owner_diary_recent"].as_array().unwrap().len(), 2);
    assert_eq!(json["owner_diary_recent"][0]["date"].as_str().unwrap(), ymd);
    assert_eq!(
        json["owner_diary_recent"][1]["date"].as_str().unwrap(),
        y_ymd
    );
    assert!(
        json["owner_diary_recent"][1]["content"]
            .as_str()
            .unwrap()
            .contains("yesterday diary entry")
    );

    assert_eq!(json["activity_recent"].as_array().unwrap().len(), 2);
    assert_eq!(json["activity_recent"][0]["date"].as_str().unwrap(), ymd);
    assert_eq!(json["activity_recent"][1]["date"].as_str().unwrap(), y_ymd);
    assert!(
        json["activity_recent"][1]["content"]
            .as_str()
            .unwrap()
            .contains("yesterday activity entry")
    );
}

#[test]
fn default_command_hides_frontmatter_lines_from_daily_sections() {
    let tmp = assert_fs::TempDir::new().unwrap();
    let today = Local::now().date_naive();
    let yyyy = today.format("%Y").to_string();
    let mm = today.format("%m").to_string();
    let ymd = today.format("%Y-%m-%d").to_string();

    tmp.child(".amem/owner/profile.md")
        .write_str("name: yuiseki\n")
        .unwrap();
    tmp.child(format!(".amem/owner/diary/{yyyy}/{mm}/{ymd}.md"))
        .write_str("---\nsummary: \"diary summary\"\n---\n- 09:50 diary entry\n")
        .unwrap();
    tmp.child(".amem/agent/tasks/open.md")
        .write_str("- finish amem\n")
        .unwrap();
    tmp.child(format!(".amem/agent/activity/{yyyy}/{mm}/{ymd}.md"))
        .write_str("---\nsummary: \"activity summary\"\n---\n- 10:00 [codex] started coding\n")
        .unwrap();

    let mut cmd = bin();
    set_test_home(&mut cmd, tmp.path());
    cmd.current_dir(tmp.path());
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("diary entry"))
        .stdout(predicate::str::contains("started coding"))
        .stdout(predicate::str::contains("summary: ").not())
        .stdout(predicate::str::contains("\n---\n").not());
}

#[test]
fn default_command_reads_legacy_paths_for_compatibility() {
    let tmp = assert_fs::TempDir::new().unwrap();
    let today = Local::now().date_naive();
    let yyyy = today.format("%Y").to_string();
    let mm = today.format("%m").to_string();
    let ymd = today.format("%Y-%m-%d").to_string();

    tmp.child(".amem/owner/profile.md")
        .write_str("name: yuiseki\n")
        .unwrap();
    tmp.child(".amem/tasks/open.md")
        .write_str("- legacy task\n")
        .unwrap();
    tmp.child(format!(".amem/activity/{yyyy}/{mm}/{ymd}.md"))
        .write_str("- legacy activity\n")
        .unwrap();

    let mut cmd = bin();
    set_test_home(&mut cmd, tmp.path());
    cmd.current_dir(tmp.path());
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("legacy task"))
        .stdout(predicate::str::contains("legacy activity"));
}

#[test]
fn default_command_shows_owner_preferences_when_non_empty() {
    let tmp = assert_fs::TempDir::new().unwrap();

    tmp.child(".amem/owner/profile.md")
        .write_str("name: yuiseki\n")
        .unwrap();
    tmp.child(".amem/owner/preferences.md")
        .write_str("# Owner Preferences\n\n- 好きな言語: Rust\n")
        .unwrap();

    let mut cmd = bin();
    set_test_home(&mut cmd, tmp.path());
    cmd.current_dir(tmp.path());
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("== Owner Preferences =="))
        .stdout(predicate::str::contains("好きな言語: Rust"));
}

#[test]
fn default_command_shows_agent_identity_and_soul() {
    let tmp = assert_fs::TempDir::new().unwrap();

    tmp.child(".amem/agent/IDENTITY.md")
        .write_str("# Identity\n- Name: TestAgent\n")
        .unwrap();
    tmp.child(".amem/agent/SOUL.md")
        .write_str("# Soul\n- Core: Helpful\n")
        .unwrap();

    let mut cmd = bin();
    set_test_home(&mut cmd, tmp.path());
    cmd.current_dir(tmp.path());
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("== Agent Identity =="))
        .stdout(predicate::str::contains("TestAgent"))
        .stdout(predicate::str::contains("== Agent Soul =="))
        .stdout(predicate::str::contains("Helpful"));
}

#[test]
fn index_creates_sqlite_index_db() {
    let tmp = assert_fs::TempDir::new().unwrap();
    tmp.child(".amem/owner/profile.md")
        .write_str("name: test\n")
        .unwrap();

    let mut cmd = bin();
    set_test_home(&mut cmd, tmp.path());
    cmd.current_dir(tmp.path()).arg("index");
    cmd.assert().success();

    tmp.child(".amem/.index/index.db")
        .assert(predicate::path::exists());
}

#[test]
fn search_uses_sqlite_index_after_indexing() {
    let tmp = assert_fs::TempDir::new().unwrap();
    let src = tmp.child(".amem/agent/activity/2026/02/2026-02-21.md");
    src.write_str("東京で散歩した\n").unwrap();

    let mut index = bin();
    set_test_home(&mut index, tmp.path());
    index.current_dir(tmp.path()).arg("index");
    index.assert().success();

    fs::remove_file(src.path()).unwrap();

    let mut search = bin();
    set_test_home(&mut search, tmp.path());
    search
        .current_dir(tmp.path())
        .arg("search")
        .arg("東京")
        .arg("--top-k")
        .arg("1");
    search
        .assert()
        .success()
        .stdout(predicate::str::contains("2026-02-21.md"));
}

#[test]
fn get_owner_supports_alias_key_and_owner_alias_command() {
    let tmp = assert_fs::TempDir::new().unwrap();
    tmp.child(".amem/owner/profile.md")
        .write_str(
            "# Owner Profile\n\nname: ユイ\ngithub_username: yuiseki\nnative_language: 日本語\n",
        )
        .unwrap();

    let mut get_lang = bin();
    set_test_home(&mut get_lang, tmp.path());
    get_lang
        .current_dir(tmp.path())
        .arg("get")
        .arg("owner")
        .arg("lang");
    get_lang
        .assert()
        .success()
        .stdout(predicate::str::contains("日本語"));

    let mut owner_alias = bin();
    set_test_home(&mut owner_alias, tmp.path());
    owner_alias
        .current_dir(tmp.path())
        .arg("owner")
        .arg("github");
    owner_alias
        .assert()
        .success()
        .stdout(predicate::str::contains("yuiseki"));
}

#[test]
fn set_owner_updates_profile_and_preferences() {
    let tmp = assert_fs::TempDir::new().unwrap();

    let mut set_name = bin();
    set_test_home(&mut set_name, tmp.path());
    set_name
        .current_dir(tmp.path())
        .arg("set")
        .arg("owner")
        .arg("name")
        .arg("ユイ");
    set_name.assert().success();

    let mut set_pref = bin();
    set_test_home(&mut set_pref, tmp.path());
    set_pref
        .current_dir(tmp.path())
        .arg("set")
        .arg("owner")
        .arg("preference")
        .arg("特技:プログラミング");
    set_pref.assert().success();

    tmp.child(".amem/owner/profile.md")
        .assert(predicate::str::contains("name: ユイ"));
    tmp.child(".amem/owner/preferences.md")
        .assert(predicate::str::contains("特技: プログラミング"));
}

#[test]
fn set_diary_writes_owner_diary_with_explicit_date_and_time() {
    let tmp = assert_fs::TempDir::new().unwrap();

    let mut cmd = bin();
    set_test_home(&mut cmd, tmp.path());
    cmd.current_dir(tmp.path())
        .arg("set")
        .arg("diary")
        .arg("Uber Eatsで「マジックの道」で「Magic豚ラーメン(豚3枚)」を注文")
        .arg("--date")
        .arg("2026-02-20")
        .arg("--time")
        .arg("19:56");
    cmd.assert().success();

    tmp.child(".amem/owner/diary/2026/02/2026-02-20.md")
        .assert(predicate::path::exists())
        .assert(predicate::str::starts_with("---\nsummary: "))
        .assert(predicate::str::contains(
            "19:56 Uber Eatsで「マジックの道」で「Magic豚ラーメン(豚3枚)」を注文",
        ));
}

#[test]
fn set_diary_uses_today_and_now_when_date_time_omitted() {
    let tmp = assert_fs::TempDir::new().unwrap();
    let today = Local::now().date_naive();
    let yyyy = today.format("%Y").to_string();
    let mm = today.format("%m").to_string();
    let ymd = today.format("%Y-%m-%d").to_string();

    let mut cmd = bin();
    set_test_home(&mut cmd, tmp.path());
    cmd.current_dir(tmp.path())
        .arg("set")
        .arg("diary")
        .arg("散歩した");
    cmd.assert().success();

    let diary_path = tmp.child(format!(".amem/owner/diary/{yyyy}/{mm}/{ymd}.md"));
    diary_path.assert(predicate::path::exists());
    let content = fs::read_to_string(diary_path.path()).unwrap();
    assert!(content.starts_with("---\nsummary: "));
    assert!(content.contains("summary: \"\""));
    let line = content
        .lines()
        .find(|line| line.starts_with("- "))
        .unwrap_or("");
    assert!(line.starts_with("- "));
    assert!(line.contains(" 散歩した"));
    let mut parts = line.split_whitespace();
    let _dash = parts.next();
    let time = parts.next().unwrap_or("");
    assert_eq!(time.len(), 5);
    assert_eq!(time.chars().nth(2), Some(':'));
}

#[test]
fn get_diary_filters_by_today_period() {
    let tmp = assert_fs::TempDir::new().unwrap();
    let today = Local::now().date_naive();
    let yesterday = today.pred_opt().unwrap();
    let t_yyyy = today.format("%Y").to_string();
    let t_mm = today.format("%m").to_string();
    let t_ymd = today.format("%Y-%m-%d").to_string();
    let y_yyyy = yesterday.format("%Y").to_string();
    let y_mm = yesterday.format("%m").to_string();
    let y_ymd = yesterday.format("%Y-%m-%d").to_string();

    tmp.child(format!(".amem/owner/diary/{t_yyyy}/{t_mm}/{t_ymd}.md"))
        .write_str("- 08:00 today diary\n")
        .unwrap();
    tmp.child(format!(".amem/owner/diary/{y_yyyy}/{y_mm}/{y_ymd}.md"))
        .write_str("- 09:00 yesterday diary\n")
        .unwrap();

    let mut cmd = bin();
    set_test_home(&mut cmd, tmp.path());
    cmd.current_dir(tmp.path())
        .arg("get")
        .arg("diary")
        .arg("today");
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("Owner Diary:"))
        .stdout(predicate::str::contains("today diary"))
        .stdout(predicate::str::contains("yesterday diary").not());
}

#[test]
fn get_diary_week_shows_full_window_by_default() {
    let tmp = assert_fs::TempDir::new().unwrap();
    let today = Local::now().date_naive();
    let yesterday = today.pred_opt().unwrap();
    let t_yyyy = today.format("%Y").to_string();
    let t_mm = today.format("%m").to_string();
    let t_ymd = today.format("%Y-%m-%d").to_string();
    let y_yyyy = yesterday.format("%Y").to_string();
    let y_mm = yesterday.format("%m").to_string();
    let y_ymd = yesterday.format("%Y-%m-%d").to_string();

    let mut today_lines = String::from("---\nsummary: \"\"\n---\n");
    for i in 0..12 {
        today_lines.push_str(&format!("- 08:{:02} today-{}\n", i, i));
    }
    tmp.child(format!(".amem/owner/diary/{t_yyyy}/{t_mm}/{t_ymd}.md"))
        .write_str(&today_lines)
        .unwrap();
    tmp.child(format!(".amem/owner/diary/{y_yyyy}/{y_mm}/{y_ymd}.md"))
        .write_str("---\nsummary: \"yesterday-visible\"\n---\n- 07:00 yesterday-entry\n")
        .unwrap();

    let mut cmd = bin();
    set_test_home(&mut cmd, tmp.path());
    cmd.current_dir(tmp.path())
        .arg("get")
        .arg("diary")
        .arg("week");
    cmd.assert()
        .success()
        .stdout(predicate::str::contains(format!(
            "- [{y_ymd}] yesterday-visible"
        )))
        .stdout(predicate::str::contains("today-0").not())
        .stdout(predicate::str::contains("yesterday-entry").not());
}

#[test]
fn get_diary_week_detail_shows_full_entries() {
    let tmp = assert_fs::TempDir::new().unwrap();
    let today = Local::now().date_naive();
    let yesterday = today.pred_opt().unwrap();
    let t_yyyy = today.format("%Y").to_string();
    let t_mm = today.format("%m").to_string();
    let t_ymd = today.format("%Y-%m-%d").to_string();
    let y_yyyy = yesterday.format("%Y").to_string();
    let y_mm = yesterday.format("%m").to_string();
    let y_ymd = yesterday.format("%Y-%m-%d").to_string();

    tmp.child(format!(".amem/owner/diary/{t_yyyy}/{t_mm}/{t_ymd}.md"))
        .write_str("---\nsummary: \"\"\n---\n- 08:00 today-entry\n")
        .unwrap();
    tmp.child(format!(".amem/owner/diary/{y_yyyy}/{y_mm}/{y_ymd}.md"))
        .write_str("---\nsummary: \"yesterday summary\"\n---\n- 07:00 yesterday-entry\n")
        .unwrap();

    let mut cmd = bin();
    set_test_home(&mut cmd, tmp.path());
    cmd.current_dir(tmp.path())
        .arg("get")
        .arg("diary")
        .arg("week")
        .arg("--detail");
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("today-entry"))
        .stdout(predicate::str::contains("yesterday-entry"))
        .stdout(predicate::str::contains(format!("- [{y_ymd}] yesterday summary")).not());
}

#[test]
fn get_diary_month_shows_daily_summaries_by_default() {
    let tmp = assert_fs::TempDir::new().unwrap();
    let today = Local::now().date_naive();
    let old = today - Duration::days(40);
    let t_yyyy = today.format("%Y").to_string();
    let t_mm = today.format("%m").to_string();
    let t_ymd = today.format("%Y-%m-%d").to_string();
    let o_yyyy = old.format("%Y").to_string();
    let o_mm = old.format("%m").to_string();

    tmp.child(format!(".amem/owner/diary/{t_yyyy}/{t_mm}/{t_ymd}.md"))
        .write_str("---\nsummary: \"today-summary\"\n---\n- 08:00 today-entry\n")
        .unwrap();
    tmp.child(format!(
        ".amem/owner/diary/{o_yyyy}/{o_mm}/{}.md",
        old.format("%Y-%m-%d")
    ))
    .write_str("---\nsummary: \"old-summary\"\n---\n- 07:00 old-entry\n")
    .unwrap();

    let mut cmd = bin();
    set_test_home(&mut cmd, tmp.path());
    cmd.current_dir(tmp.path())
        .arg("get")
        .arg("diary")
        .arg("month");
    cmd.assert()
        .success()
        .stdout(predicate::str::contains(format!(
            "- [{t_ymd}] today-summary"
        )))
        .stdout(predicate::str::contains("today-entry").not())
        .stdout(predicate::str::contains("old-summary").not());
}

#[test]
fn get_diary_month_detail_shows_full_entries() {
    let tmp = assert_fs::TempDir::new().unwrap();
    let today = Local::now().date_naive();
    let t_yyyy = today.format("%Y").to_string();
    let t_mm = today.format("%m").to_string();
    let t_ymd = today.format("%Y-%m-%d").to_string();

    tmp.child(format!(".amem/owner/diary/{t_yyyy}/{t_mm}/{t_ymd}.md"))
        .write_str("---\nsummary: \"today-summary\"\n---\n- 08:00 today-entry\n")
        .unwrap();

    let mut cmd = bin();
    set_test_home(&mut cmd, tmp.path());
    cmd.current_dir(tmp.path())
        .arg("get")
        .arg("diary")
        .arg("month")
        .arg("--detail");
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("today-entry"))
        .stdout(predicate::str::contains("today-summary").not());
}

#[test]
fn set_owner_without_target_fails() {
    let tmp = assert_fs::TempDir::new().unwrap();

    let mut cmd = bin();
    set_test_home(&mut cmd, tmp.path());
    cmd.current_dir(tmp.path()).arg("set").arg("owner");
    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("missing target"));
}

#[test]
fn set_tasks_add_blocks_duplicates_and_done_moves_task() {
    let tmp = assert_fs::TempDir::new().unwrap();

    let mut add = bin();
    set_test_home(&mut add, tmp.path());
    add.current_dir(tmp.path())
        .arg("set")
        .arg("tasks")
        .arg("xxxについて調査する");
    let add_output = add.assert().success().get_output().stdout.clone();
    let hash = String::from_utf8(add_output).unwrap().trim().to_string();
    assert!(hash.len() == 7);

    let mut dup = bin();
    set_test_home(&mut dup, tmp.path());
    dup.current_dir(tmp.path())
        .arg("set")
        .arg("tasks")
        .arg("xxxについて調査する");
    dup.assert()
        .failure()
        .stderr(predicate::str::contains("task already exists"));

    let mut done = bin();
    set_test_home(&mut done, tmp.path());
    done.current_dir(tmp.path())
        .arg("set")
        .arg("tasks")
        .arg("done")
        .arg(&hash);
    done.assert().success();

    tmp.child(".amem/agent/tasks/open.md")
        .assert(predicate::str::contains("xxxについて調査する").not());
    tmp.child(".amem/agent/tasks/done.md")
        .assert(predicate::str::contains("xxxについて調査する"));
}

#[test]
fn get_acts_filters_by_today_period() {
    let tmp = assert_fs::TempDir::new().unwrap();
    let today = Local::now().date_naive();
    let yesterday = today.pred_opt().unwrap();
    let t_yyyy = today.format("%Y").to_string();
    let t_mm = today.format("%m").to_string();
    let t_ymd = today.format("%Y-%m-%d").to_string();
    let y_yyyy = yesterday.format("%Y").to_string();
    let y_mm = yesterday.format("%m").to_string();
    let y_ymd = yesterday.format("%Y-%m-%d").to_string();

    tmp.child(format!(".amem/agent/activity/{t_yyyy}/{t_mm}/{t_ymd}.md"))
        .write_str("- 08:13 [codex] today task\n")
        .unwrap();
    tmp.child(format!(".amem/agent/activity/{y_yyyy}/{y_mm}/{y_ymd}.md"))
        .write_str("- 07:00 [codex] yesterday task\n")
        .unwrap();

    let mut cmd = bin();
    set_test_home(&mut cmd, tmp.path());
    cmd.current_dir(tmp.path())
        .arg("get")
        .arg("acts")
        .arg("today");
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("today task"))
        .stdout(predicate::str::contains("yesterday task").not());
}

#[test]
fn get_acts_rejects_invalid_period() {
    let tmp = assert_fs::TempDir::new().unwrap();
    let mut cmd = bin();
    set_test_home(&mut cmd, tmp.path());
    cmd.current_dir(tmp.path())
        .arg("get")
        .arg("acts")
        .arg("foo");
    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("unsupported period"));
}

#[test]
fn get_acts_week_shows_full_window_by_default() {
    let tmp = assert_fs::TempDir::new().unwrap();
    let today = Local::now().date_naive();
    let yesterday = today.pred_opt().unwrap();
    let t_yyyy = today.format("%Y").to_string();
    let t_mm = today.format("%m").to_string();
    let t_ymd = today.format("%Y-%m-%d").to_string();
    let y_yyyy = yesterday.format("%Y").to_string();
    let y_mm = yesterday.format("%m").to_string();
    let y_ymd = yesterday.format("%Y-%m-%d").to_string();

    let mut today_lines = String::from("---\nsummary: \"\"\n---\n");
    for i in 0..12 {
        today_lines.push_str(&format!("- 08:{:02} [codex] today-{}\n", i, i));
    }
    tmp.child(format!(".amem/agent/activity/{t_yyyy}/{t_mm}/{t_ymd}.md"))
        .write_str(&today_lines)
        .unwrap();
    tmp.child(format!(".amem/agent/activity/{y_yyyy}/{y_mm}/{y_ymd}.md"))
        .write_str("---\nsummary: \"yesterday-visible\"\n---\n- 07:00 [codex] yesterday-entry\n")
        .unwrap();

    let mut cmd = bin();
    set_test_home(&mut cmd, tmp.path());
    cmd.current_dir(tmp.path())
        .arg("get")
        .arg("acts")
        .arg("week");
    cmd.assert()
        .success()
        .stdout(predicate::str::contains(format!(
            "- [{y_ymd}] yesterday-visible"
        )))
        .stdout(predicate::str::contains("today-0").not())
        .stdout(predicate::str::contains("yesterday-entry").not());
}

#[test]
fn get_acts_week_detail_shows_full_entries() {
    let tmp = assert_fs::TempDir::new().unwrap();
    let today = Local::now().date_naive();
    let yesterday = today.pred_opt().unwrap();
    let t_yyyy = today.format("%Y").to_string();
    let t_mm = today.format("%m").to_string();
    let t_ymd = today.format("%Y-%m-%d").to_string();
    let y_yyyy = yesterday.format("%Y").to_string();
    let y_mm = yesterday.format("%m").to_string();
    let y_ymd = yesterday.format("%Y-%m-%d").to_string();

    tmp.child(format!(".amem/agent/activity/{t_yyyy}/{t_mm}/{t_ymd}.md"))
        .write_str("---\nsummary: \"\"\n---\n- 08:00 [codex] today-entry\n")
        .unwrap();
    tmp.child(format!(".amem/agent/activity/{y_yyyy}/{y_mm}/{y_ymd}.md"))
        .write_str("---\nsummary: \"yesterday summary\"\n---\n- 07:00 [codex] yesterday-entry\n")
        .unwrap();

    let mut cmd = bin();
    set_test_home(&mut cmd, tmp.path());
    cmd.current_dir(tmp.path())
        .arg("get")
        .arg("acts")
        .arg("week")
        .arg("--detail");
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("today-entry"))
        .stdout(predicate::str::contains("yesterday-entry"))
        .stdout(predicate::str::contains(format!("- [{y_ymd}] yesterday summary")).not());
}

#[test]
fn get_acts_month_shows_daily_summaries_by_default() {
    let tmp = assert_fs::TempDir::new().unwrap();
    let today = Local::now().date_naive();
    let old = today - Duration::days(40);
    let t_yyyy = today.format("%Y").to_string();
    let t_mm = today.format("%m").to_string();
    let t_ymd = today.format("%Y-%m-%d").to_string();
    let o_yyyy = old.format("%Y").to_string();
    let o_mm = old.format("%m").to_string();

    tmp.child(format!(".amem/agent/activity/{t_yyyy}/{t_mm}/{t_ymd}.md"))
        .write_str("---\nsummary: \"today-summary\"\n---\n- 08:00 [codex] today-entry\n")
        .unwrap();
    tmp.child(format!(
        ".amem/agent/activity/{o_yyyy}/{o_mm}/{}.md",
        old.format("%Y-%m-%d")
    ))
    .write_str("---\nsummary: \"old-summary\"\n---\n- 07:00 [codex] old-entry\n")
    .unwrap();

    let mut cmd = bin();
    set_test_home(&mut cmd, tmp.path());
    cmd.current_dir(tmp.path())
        .arg("get")
        .arg("acts")
        .arg("month");
    cmd.assert()
        .success()
        .stdout(predicate::str::contains(format!(
            "- [{t_ymd}] today-summary"
        )))
        .stdout(predicate::str::contains("today-entry").not())
        .stdout(predicate::str::contains("old-summary").not());
}

#[test]
fn get_acts_month_detail_shows_full_entries() {
    let tmp = assert_fs::TempDir::new().unwrap();
    let today = Local::now().date_naive();
    let t_yyyy = today.format("%Y").to_string();
    let t_mm = today.format("%m").to_string();
    let t_ymd = today.format("%Y-%m-%d").to_string();

    tmp.child(format!(".amem/agent/activity/{t_yyyy}/{t_mm}/{t_ymd}.md"))
        .write_str("---\nsummary: \"today-summary\"\n---\n- 08:00 [codex] today-entry\n")
        .unwrap();

    let mut cmd = bin();
    set_test_home(&mut cmd, tmp.path());
    cmd.current_dir(tmp.path())
        .arg("get")
        .arg("acts")
        .arg("month")
        .arg("--detail");
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("today-entry"))
        .stdout(predicate::str::contains("today-summary").not());
}

#[test]
fn codex_subcommand_seeds_then_resumes_last() {
    let tmp = assert_fs::TempDir::new().unwrap();
    let today = Local::now().date_naive();
    let yesterday = today.pred_opt().unwrap();
    let t_yyyy = today.format("%Y").to_string();
    let t_mm = today.format("%m").to_string();
    let t_ymd = today.format("%Y-%m-%d").to_string();
    let y_yyyy = yesterday.format("%Y").to_string();
    let y_mm = yesterday.format("%m").to_string();
    let y_ymd = yesterday.format("%Y-%m-%d").to_string();
    tmp.child(".amem/owner/profile.md")
        .write_str("name: tester\n")
        .unwrap();
    tmp.child(format!(".amem/owner/diary/{t_yyyy}/{t_mm}/{t_ymd}.md"))
        .write_str("- 09:10 today diary entry\n")
        .unwrap();
    tmp.child(format!(".amem/owner/diary/{y_yyyy}/{y_mm}/{y_ymd}.md"))
        .write_str("- 08:10 yesterday diary entry\n")
        .unwrap();
    tmp.child(format!(".amem/agent/activity/{t_yyyy}/{t_mm}/{t_ymd}.md"))
        .write_str("- 09:20 [codex] today activity entry\n")
        .unwrap();
    tmp.child(format!(".amem/agent/activity/{y_yyyy}/{y_mm}/{y_ymd}.md"))
        .write_str("- 08:20 [codex] yesterday activity entry\n")
        .unwrap();

    let mock = tmp.child("mock-codex.sh");
    mock.write_str(
        r#"#!/usr/bin/env bash
set -eu
case "${1:-}" in
  exec)
    if [[ "$*" == *"== Owner Profile =="* ]]; then
      if [[ "$*" == *"today diary entry"* && "$*" == *"yesterday diary entry"* && "$*" == *"today activity entry"* && "$*" == *"yesterday activity entry"* ]]; then
        if [[ "$*" == *"--dangerously-bypass-approvals-and-sandbox"* ]]; then
          echo "exec markdown window yolo" >> "$AMEM_MOCK_CODEX_LOG"
        else
          echo "exec markdown window no-yolo" >> "$AMEM_MOCK_CODEX_LOG"
        fi
      else
        if [[ "$*" == *"--dangerously-bypass-approvals-and-sandbox"* ]]; then
          echo "exec markdown no-window yolo" >> "$AMEM_MOCK_CODEX_LOG"
        else
          echo "exec markdown no-window no-yolo" >> "$AMEM_MOCK_CODEX_LOG"
        fi
      fi
    else
      if [[ "$*" == *"--dangerously-bypass-approvals-and-sandbox"* ]]; then
        echo "exec non-markdown yolo" >> "$AMEM_MOCK_CODEX_LOG"
      else
        echo "exec non-markdown no-yolo" >> "$AMEM_MOCK_CODEX_LOG"
      fi
    fi
    echo '{"type":"thread.started","thread_id":"019c7f9d-2298-70f1-a19d-c164f18d7f45"}'
    ;;
  resume)
    shift
    echo "resume $*" >> "$AMEM_MOCK_CODEX_LOG"
    ;;
  *)
    echo "other $*" >> "$AMEM_MOCK_CODEX_LOG"
    ;;
esac
"#,
    )
    .unwrap();

    #[cfg(unix)]
    {
        let mut perms = fs::metadata(mock.path()).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(mock.path(), perms).unwrap();
    }

    let log = tmp.child("codex.log");
    let mut cmd = bin();
    set_test_home(&mut cmd, tmp.path());
    cmd.current_dir(tmp.path())
        .env("AMEM_CODEX_BIN", mock.path())
        .env("AMEM_MOCK_CODEX_LOG", log.path())
        .arg("codex")
        .arg("--prompt")
        .arg("continue with today tasks");

    cmd.assert().success();

    let lines: Vec<String> = fs::read_to_string(log.path())
        .unwrap()
        .lines()
        .map(|s| s.to_string())
        .collect();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0], "exec markdown window yolo");
    assert!(lines[1].starts_with("resume "));
    assert!(lines[1].contains("--dangerously-bypass-approvals-and-sandbox"));
    assert!(lines[1].contains("019c7f9d-2298-70f1-a19d-c164f18d7f45"));
    assert!(!lines[1].contains(" --last"));
    assert!(lines[1].contains("continue with today tasks"));
}

#[test]
fn codex_subcommand_resume_only_skips_seed() {
    let tmp = assert_fs::TempDir::new().unwrap();
    let mock = tmp.child("mock-codex.sh");
    mock.write_str(
        r#"#!/usr/bin/env bash
set -eu
echo "$*" >> "$AMEM_MOCK_CODEX_LOG"
"#,
    )
    .unwrap();

    #[cfg(unix)]
    {
        let mut perms = fs::metadata(mock.path()).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(mock.path(), perms).unwrap();
    }

    let log = tmp.child("codex.log");
    let mut cmd = bin();
    set_test_home(&mut cmd, tmp.path());
    cmd.current_dir(tmp.path())
        .env("AMEM_CODEX_BIN", mock.path())
        .env("AMEM_MOCK_CODEX_LOG", log.path())
        .arg("codex")
        .arg("--resume-only");
    cmd.assert().success();

    let lines: Vec<String> = fs::read_to_string(log.path())
        .unwrap()
        .lines()
        .map(|s| s.to_string())
        .collect();
    assert_eq!(lines.len(), 1);
    assert!(lines[0].contains("resume --dangerously-bypass-approvals-and-sandbox --last"));
}

#[test]
fn gemini_subcommand_seeds_then_resumes_latest() {
    let tmp = assert_fs::TempDir::new().unwrap();
    tmp.child(".amem/owner/profile.md")
        .write_str("name: tester\n")
        .unwrap();

    let mock = tmp.child("mock-gemini.sh");
    mock.write_str(
        r#"#!/usr/bin/env bash
set -eu
if [[ "$*" == *"--resume"* ]]; then
  echo "resume $*" >> "$AMEM_MOCK_GEMINI_LOG"
else
  if [[ "$*" == *"== Owner Profile =="* ]]; then
    if [[ "$*" == *"--approval-mode yolo"* ]]; then
      echo "seed markdown yolo" >> "$AMEM_MOCK_GEMINI_LOG"
    else
      echo "seed markdown no-yolo" >> "$AMEM_MOCK_GEMINI_LOG"
    fi
  else
    if [[ "$*" == *"--approval-mode yolo"* ]]; then
      echo "seed non-markdown yolo" >> "$AMEM_MOCK_GEMINI_LOG"
    else
      echo "seed non-markdown no-yolo" >> "$AMEM_MOCK_GEMINI_LOG"
    fi
  fi
  echo '{"session_id":"f8db4215-e94c-41ec-b57a-51757fa65cc4","response":"MEMORY_READY"}'
fi
"#,
    )
    .unwrap();

    #[cfg(unix)]
    {
        let mut perms = fs::metadata(mock.path()).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(mock.path(), perms).unwrap();
    }

    let log = tmp.child("gemini.log");
    let mut cmd = bin();
    set_test_home(&mut cmd, tmp.path());
    cmd.current_dir(tmp.path())
        .env("AMEM_GEMINI_BIN", mock.path())
        .env("AMEM_MOCK_GEMINI_LOG", log.path())
        .arg("gemini")
        .arg("--prompt")
        .arg("continue with today tasks");

    cmd.assert().success();

    let lines: Vec<String> = fs::read_to_string(log.path())
        .unwrap()
        .lines()
        .map(|s| s.to_string())
        .collect();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0], "seed markdown yolo");
    assert!(lines[1].starts_with("resume "));
    assert!(lines[1].contains("--resume f8db4215-e94c-41ec-b57a-51757fa65cc4"));
    assert!(lines[1].contains("--approval-mode yolo"));
    assert!(!lines[1].contains(" latest"));
    assert!(lines[1].contains("continue with today tasks"));
}

#[test]
fn gemini_subcommand_resume_only_skips_seed() {
    let tmp = assert_fs::TempDir::new().unwrap();
    let mock = tmp.child("mock-gemini.sh");
    mock.write_str(
        r#"#!/usr/bin/env bash
set -eu
if [[ "$*" == *"--resume"* ]]; then
  echo "resume $*" >> "$AMEM_MOCK_GEMINI_LOG"
else
  echo "seed $*" >> "$AMEM_MOCK_GEMINI_LOG"
fi
"#,
    )
    .unwrap();

    #[cfg(unix)]
    {
        let mut perms = fs::metadata(mock.path()).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(mock.path(), perms).unwrap();
    }

    let log = tmp.child("gemini.log");
    let mut cmd = bin();
    set_test_home(&mut cmd, tmp.path());
    cmd.current_dir(tmp.path())
        .env("AMEM_GEMINI_BIN", mock.path())
        .env("AMEM_MOCK_GEMINI_LOG", log.path())
        .arg("gemini")
        .arg("--resume-only");
    cmd.assert().success();

    let lines: Vec<String> = fs::read_to_string(log.path())
        .unwrap()
        .lines()
        .map(|s| s.to_string())
        .collect();
    assert_eq!(lines.len(), 1);
    assert!(lines[0].contains("resume --approval-mode yolo --resume latest"));
}

#[test]
fn claude_subcommand_seeds_then_resumes_with_session_id() {
    let tmp = assert_fs::TempDir::new().unwrap();
    tmp.child(".amem/owner/profile.md")
        .write_str("name: tester\n")
        .unwrap();

    let mock = tmp.child("mock-claude.sh");
    mock.write_str(
        r#"#!/usr/bin/env bash
set -eu
if [[ "$*" == *"--print"* ]]; then
    if [[ "$*" == *"== Owner Profile =="* ]]; then
      if [[ "$*" == *"--dangerously-skip-permissions"* ]]; then
        echo "seed markdown yolo" >> "$AMEM_MOCK_CLAUDE_LOG"
      else
        echo "seed markdown no-yolo" >> "$AMEM_MOCK_CLAUDE_LOG"
      fi
    else
      if [[ "$*" == *"--dangerously-skip-permissions"* ]]; then
        echo "seed non-markdown yolo" >> "$AMEM_MOCK_CLAUDE_LOG"
      else
        echo "seed non-markdown no-yolo" >> "$AMEM_MOCK_CLAUDE_LOG"
      fi
    fi
    echo '{"session_id":"7f6e5d4c-3b2a-1908-7654-3210abcdef12","response":"MEMORY_READY"}'
elif [[ "$*" == *"--resume"* ]]; then
  echo "resume $*" >> "$AMEM_MOCK_CLAUDE_LOG"
elif [[ "$*" == *"--continue"* ]]; then
  echo "continue $*" >> "$AMEM_MOCK_CLAUDE_LOG"
else
  echo "other $*" >> "$AMEM_MOCK_CLAUDE_LOG"
fi
"#,
    )
    .unwrap();

    #[cfg(unix)]
    {
        let mut perms = fs::metadata(mock.path()).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(mock.path(), perms).unwrap();
    }

    let log = tmp.child("claude.log");
    let mut cmd = bin();
    set_test_home(&mut cmd, tmp.path());
    cmd.current_dir(tmp.path())
        .env("AMEM_CLAUDE_BIN", mock.path())
        .env("AMEM_MOCK_CLAUDE_LOG", log.path())
        .arg("claude")
        .arg("--prompt")
        .arg("continue with today tasks");

    cmd.assert().success();

    let lines: Vec<String> = fs::read_to_string(log.path())
        .unwrap()
        .lines()
        .map(|s| s.to_string())
        .collect();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0], "seed markdown yolo");
    assert!(lines[1].starts_with("resume "));
    assert!(lines[1].contains("--resume 7f6e5d4c-3b2a-1908-7654-3210abcdef12"));
    assert!(lines[1].contains("--dangerously-skip-permissions"));
    assert!(lines[1].contains("continue with today tasks"));
}

#[test]
fn claude_subcommand_resume_only_uses_continue() {
    let tmp = assert_fs::TempDir::new().unwrap();
    let mock = tmp.child("mock-claude.sh");
    mock.write_str(
        r#"#!/usr/bin/env bash
set -eu
echo "$*" >> "$AMEM_MOCK_CLAUDE_LOG"
"#,
    )
    .unwrap();

    #[cfg(unix)]
    {
        let mut perms = fs::metadata(mock.path()).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(mock.path(), perms).unwrap();
    }

    let log = tmp.child("claude.log");
    let mut cmd = bin();
    set_test_home(&mut cmd, tmp.path());
    cmd.current_dir(tmp.path())
        .env("AMEM_CLAUDE_BIN", mock.path())
        .env("AMEM_MOCK_CLAUDE_LOG", log.path())
        .arg("claude")
        .arg("--resume-only");
    cmd.assert().success();

    let lines: Vec<String> = fs::read_to_string(log.path())
        .unwrap()
        .lines()
        .map(|s| s.to_string())
        .collect();
    assert_eq!(lines.len(), 1);
    assert!(lines[0].contains("--dangerously-skip-permissions --continue"));
}

#[test]
fn copilot_subcommand_seeds_then_resumes_with_session_id() {
    let tmp = assert_fs::TempDir::new().unwrap();
    tmp.child(".amem/owner/profile.md")
        .write_str("name: tester\n")
        .unwrap();

    let mock = tmp.child("mock-copilot.sh");
    mock.write_str(
        r#"#!/usr/bin/env bash
set -eu
if [[ "$*" == *"--resume"* ]]; then
    echo "resume $*" >> "$AMEM_MOCK_COPILOT_LOG"
elif [[ "$*" == *"--continue"* ]]; then
    echo "continue $*" >> "$AMEM_MOCK_COPILOT_LOG"
elif [[ "$*" == *"== Owner Profile =="* ]]; then
    if [[ "$*" == *"--allow-all"* ]]; then
      echo "seed markdown yolo" >> "$AMEM_MOCK_COPILOT_LOG"
    else
      echo "seed markdown no-yolo" >> "$AMEM_MOCK_COPILOT_LOG"
    fi
    touch "$PWD/copilot-session-abcd1234.md"
else
    echo "other $*" >> "$AMEM_MOCK_COPILOT_LOG"
fi
"#,
    )
    .unwrap();

    #[cfg(unix)]
    {
        let mut perms = fs::metadata(mock.path()).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(mock.path(), perms).unwrap();
    }

    let log = tmp.child("copilot.log");
    let mut cmd = bin();
    set_test_home(&mut cmd, tmp.path());
    cmd.current_dir(tmp.path())
        .env("AMEM_COPILOT_BIN", mock.path())
        .env("AMEM_MOCK_COPILOT_LOG", log.path())
        .arg("copilot")
        .arg("--prompt")
        .arg("continue with today tasks");

    cmd.assert().success();

    let lines: Vec<String> = fs::read_to_string(log.path())
        .unwrap()
        .lines()
        .map(|s| s.to_string())
        .collect();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0], "seed markdown yolo");
    assert!(lines[1].starts_with("resume "));
    assert!(lines[1].contains("--resume abcd1234"));
    assert!(lines[1].contains("--allow-all"));
    assert!(lines[1].contains("-i continue with today tasks"));
    assert!(!tmp.path().join("copilot-session-abcd1234.md").exists());
}

#[test]
fn copilot_subcommand_resume_only_uses_continue() {
    let tmp = assert_fs::TempDir::new().unwrap();
    let mock = tmp.child("mock-copilot.sh");
    mock.write_str(
        r#"#!/usr/bin/env bash
set -eu
echo "$*" >> "$AMEM_MOCK_COPILOT_LOG"
"#,
    )
    .unwrap();

    #[cfg(unix)]
    {
        let mut perms = fs::metadata(mock.path()).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(mock.path(), perms).unwrap();
    }

    let log = tmp.child("copilot.log");
    let mut cmd = bin();
    set_test_home(&mut cmd, tmp.path());
    cmd.current_dir(tmp.path())
        .env("AMEM_COPILOT_BIN", mock.path())
        .env("AMEM_MOCK_COPILOT_LOG", log.path())
        .arg("copilot")
        .arg("--resume-only");
    cmd.assert().success();

    let lines: Vec<String> = fs::read_to_string(log.path())
        .unwrap()
        .lines()
        .map(|s| s.to_string())
        .collect();
    assert_eq!(lines.len(), 1);
    assert!(lines[0].contains("--allow-all --continue"));
}

#[test]
fn opencode_subcommand_seeds_then_resumes_with_session_id() {
    let tmp = assert_fs::TempDir::new().unwrap();
    tmp.child(".amem/owner/profile.md")
        .write_str("name: tester\n")
        .unwrap();

    let mock = tmp.child("mock-opencode.sh");
    mock.write_str(
        r#"#!/usr/bin/env bash
set -eu
if [[ "${1:-}" == "run" ]]; then
    if [[ "$*" == *"== Owner Profile =="* ]]; then
      if [[ "$*" == *"--format json"* && "$*" == *"--agent build"* ]]; then
        echo "seed markdown json yolo perm:$OPENCODE_PERMISSION cfg:$OPENCODE_CONFIG_CONTENT" >> "$AMEM_MOCK_OPENCODE_LOG"
      else
        echo "seed markdown non-yolo perm:$OPENCODE_PERMISSION cfg:$OPENCODE_CONFIG_CONTENT" >> "$AMEM_MOCK_OPENCODE_LOG"
      fi
    else
      echo "seed non-markdown perm:$OPENCODE_PERMISSION cfg:$OPENCODE_CONFIG_CONTENT" >> "$AMEM_MOCK_OPENCODE_LOG"
    fi
    echo '{"type":"step_start","sessionID":"ses_abcd1234"}'
elif [[ "$*" == *"--session"* ]]; then
    echo "resume $* perm:$OPENCODE_PERMISSION cfg:$OPENCODE_CONFIG_CONTENT" >> "$AMEM_MOCK_OPENCODE_LOG"
elif [[ "$*" == *"--continue"* ]]; then
    echo "continue $* perm:$OPENCODE_PERMISSION cfg:$OPENCODE_CONFIG_CONTENT" >> "$AMEM_MOCK_OPENCODE_LOG"
else
    echo "other $* perm:$OPENCODE_PERMISSION cfg:$OPENCODE_CONFIG_CONTENT" >> "$AMEM_MOCK_OPENCODE_LOG"
fi
"#,
    )
    .unwrap();

    #[cfg(unix)]
    {
        let mut perms = fs::metadata(mock.path()).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(mock.path(), perms).unwrap();
    }

    let log = tmp.child("opencode.log");
    let mut cmd = bin();
    set_test_home(&mut cmd, tmp.path());
    cmd.current_dir(tmp.path())
        .env("AMEM_OPENCODE_BIN", mock.path())
        .env("AMEM_MOCK_OPENCODE_LOG", log.path())
        .arg("opencode")
        .arg("--prompt")
        .arg("continue with today tasks");

    cmd.assert().success();

    let lines: Vec<String> = fs::read_to_string(log.path())
        .unwrap()
        .lines()
        .map(|s| s.to_string())
        .collect();
    assert_eq!(lines.len(), 2);
    assert!(lines[0].starts_with("seed markdown json yolo"));
    assert!(lines[0].contains("\"*\":\"allow\""));
    assert!(lines[0].contains("\"agent\":{\"build\":{\"permission\":{\"*\":\"allow\"}}}"));
    assert!(lines[1].starts_with("resume "));
    assert!(lines[1].contains("--agent build"));
    assert!(lines[1].contains("--session ses_abcd1234"));
    assert!(lines[1].contains("--prompt continue with today tasks"));
    assert!(lines[1].contains("\"*\":\"allow\""));
    assert!(lines[1].contains("\"agent\":{\"build\":{\"permission\":{\"*\":\"allow\"}}}"));
}

#[test]
fn opencode_subcommand_resume_only_uses_continue() {
    let tmp = assert_fs::TempDir::new().unwrap();
    let mock = tmp.child("mock-opencode.sh");
    mock.write_str(
        r#"#!/usr/bin/env bash
set -eu
echo "$* perm:$OPENCODE_PERMISSION cfg:$OPENCODE_CONFIG_CONTENT" >> "$AMEM_MOCK_OPENCODE_LOG"
"#,
    )
    .unwrap();

    #[cfg(unix)]
    {
        let mut perms = fs::metadata(mock.path()).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(mock.path(), perms).unwrap();
    }

    let log = tmp.child("opencode.log");
    let mut cmd = bin();
    set_test_home(&mut cmd, tmp.path());
    cmd.current_dir(tmp.path())
        .env("AMEM_OPENCODE_BIN", mock.path())
        .env("AMEM_MOCK_OPENCODE_LOG", log.path())
        .arg("opencode")
        .arg("--resume-only");
    cmd.assert().success();

    let lines: Vec<String> = fs::read_to_string(log.path())
        .unwrap()
        .lines()
        .map(|s| s.to_string())
        .collect();
    assert_eq!(lines.len(), 1);
    assert!(lines[0].contains("--agent build --continue"));
    assert!(lines[0].contains("\"*\":\"allow\""));
    assert!(lines[0].contains("\"agent\":{\"build\":{\"permission\":{\"*\":\"allow\"}}}"));
}

#[test]
fn opencode_subcommand_supports_agent_override_env() {
    let tmp = assert_fs::TempDir::new().unwrap();
    let mock = tmp.child("mock-opencode.sh");
    mock.write_str(
        r#"#!/usr/bin/env bash
set -eu
echo "$* perm:$OPENCODE_PERMISSION cfg:$OPENCODE_CONFIG_CONTENT" >> "$AMEM_MOCK_OPENCODE_LOG"
"#,
    )
    .unwrap();

    #[cfg(unix)]
    {
        let mut perms = fs::metadata(mock.path()).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(mock.path(), perms).unwrap();
    }

    let log = tmp.child("opencode.log");
    let mut cmd = bin();
    set_test_home(&mut cmd, tmp.path());
    cmd.current_dir(tmp.path())
        .env("AMEM_OPENCODE_BIN", mock.path())
        .env("AMEM_OPENCODE_AGENT", "custom-yolo")
        .env("AMEM_MOCK_OPENCODE_LOG", log.path())
        .arg("opencode")
        .arg("--resume-only");
    cmd.assert().success();

    let lines: Vec<String> = fs::read_to_string(log.path())
        .unwrap()
        .lines()
        .map(|s| s.to_string())
        .collect();
    assert_eq!(lines.len(), 1);
    assert!(lines[0].contains("--agent custom-yolo --continue"));
    assert!(lines[0].contains("\"*\":\"allow\""));
    assert!(lines[0].contains("\"agent\":{\"custom-yolo\":{\"permission\":{\"*\":\"allow\"}}}"));
}

#[test]
fn opencode_subcommand_supports_permission_override_env() {
    let tmp = assert_fs::TempDir::new().unwrap();
    let mock = tmp.child("mock-opencode.sh");
    mock.write_str(
        r#"#!/usr/bin/env bash
set -eu
echo "$* perm:$OPENCODE_PERMISSION cfg:$OPENCODE_CONFIG_CONTENT" >> "$AMEM_MOCK_OPENCODE_LOG"
"#,
    )
    .unwrap();

    #[cfg(unix)]
    {
        let mut perms = fs::metadata(mock.path()).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(mock.path(), perms).unwrap();
    }

    let log = tmp.child("opencode.log");
    let mut cmd = bin();
    set_test_home(&mut cmd, tmp.path());
    cmd.current_dir(tmp.path())
        .env("AMEM_OPENCODE_BIN", mock.path())
        .env("AMEM_OPENCODE_PERMISSION", r#"{"*":"ask"}"#)
        .env("AMEM_MOCK_OPENCODE_LOG", log.path())
        .arg("opencode")
        .arg("--resume-only");
    cmd.assert().success();

    let lines: Vec<String> = fs::read_to_string(log.path())
        .unwrap()
        .lines()
        .map(|s| s.to_string())
        .collect();
    assert_eq!(lines.len(), 1);
    assert!(lines[0].contains("--agent build --continue"));
    assert!(lines[0].contains("\"*\":\"ask\""));
    assert!(lines[0].contains("\"agent\":{\"build\":{\"permission\":{\"*\":\"allow\"}}}"));
}

#[test]
fn opencode_subcommand_honors_existing_opencode_permission_env() {
    let tmp = assert_fs::TempDir::new().unwrap();
    let mock = tmp.child("mock-opencode.sh");
    mock.write_str(
        r#"#!/usr/bin/env bash
set -eu
echo "$* perm:$OPENCODE_PERMISSION cfg:$OPENCODE_CONFIG_CONTENT" >> "$AMEM_MOCK_OPENCODE_LOG"
"#,
    )
    .unwrap();

    #[cfg(unix)]
    {
        let mut perms = fs::metadata(mock.path()).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(mock.path(), perms).unwrap();
    }

    let log = tmp.child("opencode.log");
    let mut cmd = bin();
    set_test_home(&mut cmd, tmp.path());
    cmd.current_dir(tmp.path())
        .env("AMEM_OPENCODE_BIN", mock.path())
        .env("OPENCODE_PERMISSION", r#"{"*":"deny"}"#)
        .env("AMEM_MOCK_OPENCODE_LOG", log.path())
        .arg("opencode")
        .arg("--resume-only");
    cmd.assert().success();

    let lines: Vec<String> = fs::read_to_string(log.path())
        .unwrap()
        .lines()
        .map(|s| s.to_string())
        .collect();
    assert_eq!(lines.len(), 1);
    assert!(lines[0].contains("--agent build --continue"));
    assert!(lines[0].contains("\"*\":\"deny\""));
    assert!(lines[0].contains("\"agent\":{\"build\":{\"permission\":{\"*\":\"allow\"}}}"));
}

#[test]
fn opencode_subcommand_supports_config_content_override_env() {
    let tmp = assert_fs::TempDir::new().unwrap();
    let mock = tmp.child("mock-opencode.sh");
    mock.write_str(
        r#"#!/usr/bin/env bash
set -eu
echo "$* perm:$OPENCODE_PERMISSION cfg:$OPENCODE_CONFIG_CONTENT" >> "$AMEM_MOCK_OPENCODE_LOG"
"#,
    )
    .unwrap();

    #[cfg(unix)]
    {
        let mut perms = fs::metadata(mock.path()).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(mock.path(), perms).unwrap();
    }

    let log = tmp.child("opencode.log");
    let mut cmd = bin();
    set_test_home(&mut cmd, tmp.path());
    cmd.current_dir(tmp.path())
        .env("AMEM_OPENCODE_BIN", mock.path())
        .env(
            "AMEM_OPENCODE_CONFIG_CONTENT",
            r#"{"agent":{"build":{"permission":{"*":"deny"}}}}"#,
        )
        .env("AMEM_MOCK_OPENCODE_LOG", log.path())
        .arg("opencode")
        .arg("--resume-only");
    cmd.assert().success();

    let lines: Vec<String> = fs::read_to_string(log.path())
        .unwrap()
        .lines()
        .map(|s| s.to_string())
        .collect();
    assert_eq!(lines.len(), 1);
    assert!(lines[0].contains("--agent build --continue"));
    assert!(lines[0].contains("cfg:{\"agent\":{\"build\":{\"permission\":{\"*\":\"deny\"}}}}"));
}
