//! `qtss-reset` — Kill worker, wipe all processed data, verify clean state.
//!
//! Usage:
//!   cargo run -p qtss-worker --bin qtss-reset
//!   cargo run -p qtss-worker --bin qtss-reset -- --dry-run   # only report, don't delete

use qtss_common::{load_dotenv, postgres_url_from_env_or_default};
use qtss_storage::create_pool;
use sqlx::PgPool;

const RED: &str = "\x1b[31m";
const GREEN: &str = "\x1b[32m";
const YELLOW: &str = "\x1b[33m";
const CYAN: &str = "\x1b[36m";
const BOLD: &str = "\x1b[1m";
const RESET: &str = "\x1b[0m";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    load_dotenv();

    let dry_run = std::env::args().any(|a| a == "--dry-run");

    println!("\n{BOLD}{CYAN}╔══════════════════════════════════════════╗{RESET}");
    println!("{BOLD}{CYAN}║        QTSS FULL DATA RESET TOOL         ║{RESET}");
    println!("{BOLD}{CYAN}╚══════════════════════════════════════════╝{RESET}\n");

    if dry_run {
        println!("{YELLOW}[DRY-RUN] Sadece rapor — silme yapilmayacak{RESET}\n");
    }

    // ── 1. Kill worker process ──────────────────────────────────────
    println!("{BOLD}[1/4] Worker process kontrol...{RESET}");
    kill_worker_processes(dry_run);

    // ── 2. Connect DB ───────────────────────────────────────────────
    let db_url = postgres_url_from_env_or_default("");
    if db_url.trim().is_empty() {
        eprintln!("{RED}DATABASE_URL bulunamadi!{RESET}");
        std::process::exit(1);
    }
    let pool = create_pool(&db_url, 2).await?;

    // ── 3. Report current state ─────────────────────────────────────
    println!("\n{BOLD}[2/4] Mevcut veri durumu:{RESET}");
    report_table_counts(&pool).await;

    if dry_run {
        println!("\n{YELLOW}[DRY-RUN] --dry-run bayragi kaldirilarak tekrar calistirilabilir.{RESET}");
        return Ok(());
    }

    // ── 4. Wipe all data ────────────────────────────────────────────
    println!("\n{BOLD}[3/4] Tum veriler siliniyor...{RESET}");
    wipe_all_data(&pool).await?;

    // ── 5. Verify ───────────────────────────────────────────────────
    println!("\n{BOLD}[4/4] Dogrulama:{RESET}");
    let clean = verify_clean(&pool).await;

    if clean {
        println!("\n{GREEN}{BOLD}✓ Tum veriler basariyla sifirlandi.{RESET}");
        println!("{CYAN}  Worker baslatilabilir: cargo run -p qtss-worker{RESET}\n");
    } else {
        println!("\n{RED}{BOLD}✗ Bazi tablolar hala dolu! Worker calismiyor mu kontrol edin.{RESET}\n");
        std::process::exit(1);
    }

    Ok(())
}

fn kill_worker_processes(dry_run: bool) {
    // Find qtss-worker PIDs (exclude ourselves and grep/pgrep)
    let my_pid = std::process::id();
    let output = std::process::Command::new("sh")
        .args(["-c", &format!(
            "ps aux | grep '[q]tss-worker' | grep -v qtss-reset | grep -v {} | awk '{{print $2}}'",
            my_pid
        )])
        .output();

    match output {
        Ok(out) => {
            let pids: Vec<&str> = std::str::from_utf8(&out.stdout)
                .unwrap_or("")
                .lines()
                .filter(|l| !l.trim().is_empty())
                .collect();

            let pids: Vec<&&str> = pids.iter().collect();

            if pids.is_empty() {
                println!("  {GREEN}Worker calismıyor{RESET}");
            } else {
                for pid in &pids {
                    if dry_run {
                        println!("  {YELLOW}[DRY-RUN] Worker PID {pid} bulundu — kill edilmeyecek{RESET}");
                    } else {
                        println!("  {RED}Worker PID {pid} kill ediliyor...{RESET}");
                        let _ = std::process::Command::new("kill")
                            .args(["-15", pid])
                            .status();
                    }
                }
                if !dry_run {
                    // Wait for graceful shutdown
                    std::thread::sleep(std::time::Duration::from_secs(3));

                    // Check if still running, force kill
                    let check = std::process::Command::new("sh")
                        .args(["-c", &format!(
                            "ps aux | grep '[q]tss-worker' | grep -v qtss-reset | grep -v {} | awk '{{print $2}}'",
                            my_pid
                        )])
                        .output();
                    if let Ok(out) = check {
                        let remaining: Vec<String> = std::str::from_utf8(&out.stdout)
                            .unwrap_or("")
                            .lines()
                            .filter(|l| !l.trim().is_empty())
                            .map(|s| s.to_string())
                            .collect();
                        for pid in &remaining {
                            println!("  {RED}Worker PID {pid} force kill (SIGKILL)...{RESET}");
                            let _ = std::process::Command::new("kill")
                                .args(["-9", pid])
                                .status();
                        }
                        if !remaining.is_empty() {
                            std::thread::sleep(std::time::Duration::from_secs(2));
                        }
                    }
                    println!("  {GREEN}Worker durduruldu{RESET}");
                }
            }
        }
        Err(_) => {
            println!("  {YELLOW}pgrep bulunamadi — manuel kontrol edin{RESET}");
        }
    }
}

/// Tables to wipe: (table_name, use_truncate_cascade)
const WIPE_TABLES: &[(&str, bool)] = &[
    // Candle data
    ("market_bars", false),
    // Pivot / zigzag — canonical `pivots` table (pivot_cache retired).
    ("pivots", true),
    // Elliott / wave
    ("qtss_v2_detections", true),
    ("qtss_v2_detection_outcomes", true),
    ("wave_chain", true),
    // Regime / Wyckoff
    ("regime_snapshots", false),
    ("regime_param_overrides", false),
    ("wyckoff_structures", false),
    // Confluence / analysis
    ("qtss_v2_confluence", false),
    ("market_confluence_snapshots", false),
    ("analysis_snapshots", false),
    // Setup
    ("qtss_setups", true),
    ("qtss_v2_setup_events", false),
    ("qtss_v2_setup_rejections", false),
    ("qtss_v2_correlation_groups", false),
    // Range
    ("range_signal_events", false),
    // Onchain
    ("onchain_signal_scores", false),
    ("qtss_v2_onchain_metrics", false),
    ("nansen_raw_flows", false),
    ("nansen_enriched_signals", false),
    ("nansen_setup_rows", false),
    ("nansen_setup_runs", false),
    ("nansen_snapshots", false),
    // AI
    ("ai_decisions", false),
    ("ai_tactical_decisions", false),
    // Intake
    ("intake_playbook_candidates", true),
    ("intake_playbook_runs", false),
    // Notify
    ("notify_outbox", false),
    // Data snapshots
    ("data_snapshots", false),
    // Ingestion state (not the progress table — that gets reset, not deleted)
    ("engine_symbol_ingestion_state", false),
];

async fn report_table_counts(pool: &PgPool) {
    for (table, _) in WIPE_TABLES {
        let sql = format!("SELECT COUNT(*)::bigint FROM {}", table);
        match sqlx::query_scalar::<_, i64>(&sql).fetch_one(pool).await {
            Ok(count) if count > 0 => {
                println!("  {YELLOW}{:>35}{RESET}  {BOLD}{count}{RESET} satir", table);
            }
            Ok(_) => {
                println!("  {:>35}  0", table);
            }
            Err(_) => {
                println!("  {:>35}  {RED}tablo yok{RESET}", table);
            }
        }
    }
    // backfill_progress state summary
    match sqlx::query_as::<_, (String, i64)>(
        "SELECT state, COUNT(*)::bigint FROM backfill_progress GROUP BY state ORDER BY state",
    )
    .fetch_all(pool)
    .await
    {
        Ok(rows) => {
            let parts: Vec<String> = rows.iter().map(|(s, c)| format!("{s}={c}")).collect();
            println!("  {:>35}  {}", "backfill_progress", parts.join(", "));
        }
        Err(_) => {}
    }
}

async fn wipe_all_data(pool: &PgPool) -> anyhow::Result<()> {
    for (table, cascade) in WIPE_TABLES {
        let sql = if *cascade {
            format!("TRUNCATE {} CASCADE", table)
        } else {
            format!("DELETE FROM {}", table)
        };
        match sqlx::query(&sql).execute(pool).await {
            Ok(r) => {
                let affected = r.rows_affected();
                if affected > 0 || *cascade {
                    println!("  {GREEN}✓{RESET} {table}: {affected} satir silindi");
                }
            }
            Err(e) => {
                // Table might not exist — skip
                let msg = e.to_string();
                if msg.contains("does not exist") {
                    // skip silently
                } else {
                    println!("  {RED}✗{RESET} {table}: {e}");
                }
            }
        }
    }

    // Reset backfill_progress to pending
    sqlx::query(
        r#"UPDATE backfill_progress
           SET state = 'pending', oldest_fetched = NULL, newest_fetched = NULL,
               bar_count = 0, expected_bars = NULL, gap_count = 0, max_gap_seconds = NULL,
               backfill_started_at = NULL, backfill_finished_at = NULL, verified_at = NULL,
               last_error = NULL, pages_fetched = 0, bars_upserted = 0, updated_at = now()"#,
    )
    .execute(pool)
    .await?;
    println!("  {GREEN}✓{RESET} backfill_progress: tumu 'pending' yapildi");

    // Small delay to let any in-flight writes settle
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    // Second pass — catch anything written during the wipe window
    let leftover: i64 = sqlx::query_scalar("SELECT COUNT(*)::bigint FROM market_bars")
        .fetch_one(pool)
        .await
        .unwrap_or(0);
    if leftover > 0 {
        println!("  {YELLOW}⚠{RESET} market_bars'ta {leftover} artik kayit — temizleniyor...");
        sqlx::query("DELETE FROM market_bars")
            .execute(pool)
            .await?;
    }

    Ok(())
}

async fn verify_clean(pool: &PgPool) -> bool {
    let mut all_clean = true;

    let critical_tables = [
        "market_bars",
        "pivots",
        "qtss_v2_detections",
        "wave_chain",
        "regime_snapshots",
        "wyckoff_structures",
        "analysis_snapshots",
        "qtss_v2_confluence",
    ];

    for table in critical_tables {
        let sql = format!("SELECT COUNT(*)::bigint FROM {}", table);
        let count: i64 = sqlx::query_scalar(&sql)
            .fetch_one(pool)
            .await
            .unwrap_or(-1);
        if count == 0 {
            println!("  {GREEN}✓{RESET} {table} = 0");
        } else {
            println!("  {RED}✗{RESET} {table} = {count}");
            all_clean = false;
        }
    }

    // Check backfill_progress
    let non_pending: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::bigint FROM backfill_progress WHERE state != 'pending'",
    )
    .fetch_one(pool)
    .await
    .unwrap_or(-1);

    if non_pending == 0 {
        println!("  {GREEN}✓{RESET} backfill_progress: hepsi pending");
    } else {
        println!("  {RED}✗{RESET} backfill_progress: {non_pending} satir pending degil");
        all_clean = false;
    }

    // Active engine_symbols summary
    let active: Vec<(String, String, i64)> = sqlx::query_as(
        "SELECT symbol, segment, COUNT(*)::bigint FROM engine_symbols WHERE enabled = true GROUP BY symbol, segment ORDER BY symbol",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    println!("\n  {BOLD}Aktif semboller:{RESET}");
    for (sym, seg, cnt) in &active {
        println!("    {CYAN}{sym}{RESET} ({seg}) — {cnt} timeframe");
    }

    all_clean
}
