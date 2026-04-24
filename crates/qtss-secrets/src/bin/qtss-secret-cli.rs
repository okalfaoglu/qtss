//! Operator CLI for the qtss-secrets vault. Run over SSH on the worker
//! host after setting `QTSS_SECRET_KEK_V1` + `DATABASE_URL`.
//!
//! Supported subcommands:
//!   * `put <name> [--from-stdin | --value <plaintext>] [--description <text>]`
//!     — encrypt + insert a fresh secret
//!   * `list` — show metadata for every stored secret (never plaintext)
//!   * `rotate-kek` — re-wrap every DEK with the new KEK version
//!     (QTSS_SECRET_KEK_V<new_version> must be set)
//!
//! No subcommand ever prints ciphertext or plaintext on stdout — the
//! only way to read a secret is the in-process `VaultReader` inside
//! the worker / api binaries, which writes an audit row per read.

use qtss_secrets::{load_static_kek_from_env, PgSecretStore, SecretStore};
use sqlx::postgres::PgPoolOptions;
use std::io::Read;
use std::process::ExitCode;
use std::sync::Arc;

#[tokio::main]
async fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let Some(cmd) = args.get(1).cloned() else {
        print_help();
        return ExitCode::from(2);
    };
    match run(&cmd, &args[2..]).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(1)
        }
    }
}

async fn run(cmd: &str, rest: &[String]) -> anyhow::Result<()> {
    match cmd {
        "put" => cmd_put(rest).await,
        "list" => cmd_list().await,
        "help" | "--help" | "-h" => {
            print_help();
            Ok(())
        }
        other => anyhow::bail!("unknown subcommand: {other}. Run `help` for usage."),
    }
}

async fn cmd_put(args: &[String]) -> anyhow::Result<()> {
    let mut name: Option<String> = None;
    let mut description: Option<String> = None;
    let mut value: Option<String> = None;
    let mut from_stdin = false;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--description" => description = it.next().cloned(),
            "--value" => value = it.next().cloned(),
            "--from-stdin" => from_stdin = true,
            flag if flag.starts_with("--") => {
                anyhow::bail!("unknown flag: {flag}")
            }
            _ => {
                if name.is_none() {
                    name = Some(a.clone());
                } else {
                    anyhow::bail!("too many positional args; name already set to {:?}", name);
                }
            }
        }
    }
    let name = name.ok_or_else(|| anyhow::anyhow!("missing secret name — usage: put <name> ..."))?;
    let plaintext = read_plaintext(value.as_deref(), from_stdin)?;
    let store = build_store().await?;
    let actor = operator_actor();
    let meta = store
        .put(&name, description.as_deref(), plaintext.as_bytes(), &actor)
        .await?;
    println!(
        "put ok — name={} kek_version={} created_at={}",
        meta.name, meta.kek_version, meta.created_at
    );
    Ok(())
}

async fn cmd_list() -> anyhow::Result<()> {
    let store = build_store().await?;
    let rows = store.list().await?;
    if rows.is_empty() {
        println!("(vault is empty)");
        return Ok(());
    }
    println!("{:<40} {:<8} {:<30} description", "name", "kek_v", "created_at");
    for r in rows {
        println!(
            "{:<40} {:<8} {:<30} {}",
            r.name,
            r.kek_version,
            r.created_at,
            r.description.unwrap_or_default()
        );
    }
    Ok(())
}

fn read_plaintext(value: Option<&str>, from_stdin: bool) -> anyhow::Result<String> {
    match (value, from_stdin) {
        (Some(_), true) => anyhow::bail!("cannot combine --value and --from-stdin"),
        (Some(v), false) => Ok(v.to_string()),
        (None, true) => {
            let mut buf = String::new();
            std::io::stdin().read_to_string(&mut buf)?;
            Ok(buf.trim_end().to_string())
        }
        (None, false) => anyhow::bail!("provide --value <text> or --from-stdin"),
    }
}

async fn build_store() -> anyhow::Result<Arc<PgSecretStore>> {
    let kek = load_static_kek_from_env()?;
    let db_url = std::env::var("DATABASE_URL")?;
    let pool = PgPoolOptions::new()
        .max_connections(2)
        .connect(&db_url)
        .await?;
    Ok(Arc::new(PgSecretStore::new(pool, Arc::new(kek))))
}

fn operator_actor() -> String {
    std::env::var("QTSS_SECRET_CLI_ACTOR")
        .ok()
        .unwrap_or_else(|| {
            std::env::var("USER")
                .ok()
                .map(|u| format!("cli:{u}"))
                .unwrap_or_else(|| "cli:unknown".to_string())
        })
}

fn print_help() {
    println!(
        r#"qtss-secret-cli — operator CLI for the qtss-secrets vault

REQUIRED ENV:
  DATABASE_URL          postgres connection string
  QTSS_SECRET_KEK_V<N>  32-byte KEK as 64 hex chars
  QTSS_SECRET_KEK_VERSION  (optional, defaults to 1) — picks which V<N> var

SUBCOMMANDS:
  put <name> [--value <text> | --from-stdin] [--description <text>]
      Encrypt a secret and store it in the vault. Prefer --from-stdin to
      avoid leaking the plaintext into shell history.

  list
      List vault metadata (name / kek version / created_at / description).
      Never prints plaintext or ciphertext.

  help
      Show this message.
"#
    );
}
