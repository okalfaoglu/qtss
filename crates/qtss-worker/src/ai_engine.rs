//! AI layer timers — `qtss-ai` tactical / operational / strategic sweeps + stale decision expiry (FAZ 5).

use std::time::Duration;

use sqlx::PgPool;
use tracing::{info, warn};

fn ai_worker_env_enabled() -> bool {
    match std::env::var("QTSS_AI_ENGINE_WORKER") {
        Ok(s) => matches!(
            s.trim().to_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        ),
        Err(_) => true,
    }
}

pub fn spawn_ai_background_tasks(pool: &PgPool) {
    if !ai_worker_env_enabled() {
        info!("QTSS_AI_ENGINE_WORKER kapalı — AI arka plan döngüleri başlatılmıyor");
        return;
    }
    let p_exp = pool.clone();
    tokio::spawn(async move {
        qtss_ai::expire_stale_ai_decisions_loop(p_exp).await;
    });

    let p_t = pool.clone();
    tokio::spawn(async move {
        ai_tactical_loop(p_t).await;
    });
    let p_o = pool.clone();
    tokio::spawn(async move {
        ai_operational_loop(p_o).await;
    });
    let p_s = pool.clone();
    tokio::spawn(async move {
        ai_strategic_loop(p_s).await;
    });
}

async fn ai_tactical_loop(pool: PgPool) {
    let mut sleep_secs = 900_u64;
    loop {
        match qtss_ai::AiRuntime::from_pool(pool.clone()).await {
            Ok(rt) => {
                sleep_secs = rt.config().tactical_tick_secs.max(60);
                if let Err(e) = qtss_ai::run_tactical_sweep(&rt).await {
                    warn!(%e, "AI tactical sweep");
                }
            }
            Err(e) => warn!(%e, "AI runtime (tactical) yüklenemedi"),
        }
        tokio::time::sleep(Duration::from_secs(sleep_secs)).await;
    }
}

async fn ai_operational_loop(pool: PgPool) {
    let mut sleep_secs = 120_u64;
    loop {
        match qtss_ai::AiRuntime::from_pool(pool.clone()).await {
            Ok(rt) => {
                sleep_secs = rt.config().operational_tick_secs.max(30);
                if let Err(e) = qtss_ai::run_operational_sweep(&rt).await {
                    warn!(%e, "AI operational sweep");
                }
            }
            Err(e) => warn!(%e, "AI runtime (operational) yüklenemedi"),
        }
        tokio::time::sleep(Duration::from_secs(sleep_secs)).await;
    }
}

async fn ai_strategic_loop(pool: PgPool) {
    let mut sleep_secs = 86_400_u64;
    loop {
        match qtss_ai::AiRuntime::from_pool(pool.clone()).await {
            Ok(rt) => {
                sleep_secs = rt.config().strategic_tick_secs.max(3600);
                if rt.config().strategic_layer_enabled {
                    if let Err(e) = qtss_ai::run_strategic_sweep(&rt).await {
                        warn!(%e, "AI strategic sweep");
                    }
                }
            }
            Err(e) => warn!(%e, "AI runtime (strategic) yüklenemedi"),
        }
        tokio::time::sleep(Duration::from_secs(sleep_secs)).await;
    }
}
