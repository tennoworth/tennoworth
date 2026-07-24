//! wfm-fetch-inventory — Rust port of the Python companion.
//!
//! Cross-platform CLI that, while Warframe is running, scrapes the game's
//! process memory for the accountId + nonce + build label the game already
//! obtained at login, then calls api.warframe.com/api/inventory.php and
//! writes the response to the current directory.
//!
//! Platform notes:
//!   • Linux:   reads /proc/<pid>/mem. Needs ptrace permission (sudo or
//!              CAP_SYS_PTRACE setcap on this binary).
//!   • Windows: uses ReadProcessMemory. Works without elevation if running
//!              as the same user that started Warframe.
//!
//! Three subcommands share this file: `fetch` and `login` below are each
//! under 100 lines of adapter code; `serve` — the loopback HTTP server the
//! web UI talks to — is the CLI's largest job and lives in `serve.rs`.

mod serve;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use std::fs;
use std::path::PathBuf;
use wfm_core::auth::{bootstrap_session, encrypt_jwt, signin, validate_platform};
use wfm_core::inventory::fetch_inventory_bytes;
use wfm_core::platform::{chown_to_real_user, restrict_dir_perms, write_restricted};
use wfm_core::util::default_jwt_path;

use serve::{run_serve, ServeArgs};

/// Wraps a secret string (JWT, CSRF token) so it can't be accidentally
/// interpolated in full — Display always prints `<N chars, TAG>`, never the
/// value. The companion's hard invariant is that secrets never hit
/// stdout/stderr (see companion/CLAUDE.md); previously that relied on every
/// log site remembering to call `.len()` by hand. Shadow the plain `String`
/// with this right after it's produced, `.expose()` only at the specific
/// call sites that need the real value — any print added later in the same
/// scope is then safe by construction, not by review.
pub(crate) struct Sealed<'a> {
    value: &'a str,
    tag: &'static str,
}

impl<'a> Sealed<'a> {
    pub(crate) fn new(value: &'a str, tag: &'static str) -> Self {
        Self { value, tag }
    }
    pub(crate) fn expose(&self) -> &'a str {
        self.value
    }
}

impl std::fmt::Display for Sealed<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "<{} chars, {}>", self.value.chars().count(), self.tag)
    }
}

#[derive(Parser, Debug)]
#[command(
    name = "wfm-fetch-inventory",
    about = "Warframe inventory companion — extracts inventory.json from the game, manages warframe.market sessions.",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    /// Top-level fetch args, used when no subcommand is given (back-compat).
    #[command(flatten)]
    fetch: FetchArgs,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Extract inventory.json from the running game process (default action).
    Fetch(FetchArgs),
    /// Log in to warframe.market and store an encrypted JWT for later use.
    Login(LoginArgs),
    /// Run a loopback HTTP server that the web UI talks to for bulk listings.
    Serve(ServeArgs),
}

#[derive(clap::Args, Debug, Default, Clone)]
struct FetchArgs {
    /// Override the auto-detected Warframe PID.
    #[arg(long)]
    pid: Option<u32>,

    /// Output path. Defaults to ./inventory.json (the directory you run from).
    #[arg(long)]
    out: Option<PathBuf>,

    /// Override the auto-detected platform tag (STM/ME/NS/...).
    #[arg(long)]
    platform_tag: Option<String>,
}

#[derive(clap::Args, Debug)]
struct LoginArgs {
    /// warframe.market email. Prompted if omitted.
    #[arg(long)]
    email: Option<String>,

    /// Override the JWT storage location.
    #[arg(long)]
    out: Option<PathBuf>,

    /// WFM account platform: pc (covers Steam & Epic), ps4, xbox, or switch.
    /// Defaults to pc — only override if your warframe.market account is a
    /// console account.
    #[arg(long, default_value = "pc")]
    platform: String,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Some(Command::Login(args)) => run_login(args),
        Some(Command::Fetch(args)) => run_fetch(args),
        Some(Command::Serve(args)) => run_serve(args),
        None => run_fetch(cli.fetch),
    }
}

fn run_fetch(args: FetchArgs) -> Result<()> {
    eprintln!("Scanning Warframe memory...");
    let (bytes, info) = fetch_inventory_bytes(args.pid, args.platform_tag)?;
    eprintln!(
        "  credentials: 1 of {} unique pair(s) ({} hits)",
        info.distinct_creds, info.cred_hits
    );
    if let Some(b) = &info.build {
        eprintln!("  build label: {b}");
    }
    eprintln!("  platform tag: ct={}", info.ct);
    eprintln!("  inventory: HTTP OK ({} bytes)", bytes.len());

    // Pretty-print if valid JSON, write bytes as-is otherwise.
    let out_path = args.out.unwrap_or_else(default_out_path);
    if let Some(parent) = out_path.parent() {
        // Restrict only a directory we ourselves created (matches run_login
        // and plan persistence). The default target is the CWD — the user's
        // to manage; clamping a pre-existing dir to 0700 would be far more
        // surprising than the metadata leak it prevents.
        if !parent.exists() {
            fs::create_dir_all(parent).ok();
            restrict_dir_perms(parent);
            chown_to_real_user(parent);
        }
    }
    let final_bytes: Vec<u8> = match serde_json::from_slice::<serde_json::Value>(&bytes) {
        Ok(value) => serde_json::to_vec_pretty(&value).unwrap_or_else(|_| bytes.to_vec()),
        Err(_) => bytes.to_vec(),
    };
    // 0600 on unix — inventory.json is the user's data; no reason to leave it
    // world-readable under the default umask.
    write_restricted(&out_path, &final_bytes).with_context(|| {
        format!("writing inventory to {}", out_path.display())
    })?;
    chown_to_real_user(&out_path);

    eprintln!(
        "\nWrote {} ({} bytes)",
        out_path.display(),
        final_bytes.len()
    );
    eprintln!("Drop that file into the web UI — or run `serve` to skip the file entirely.");
    Ok(())
}

fn default_out_path() -> PathBuf {
    // The directory the user ran the command from — a manual downloader
    // gets the file next to the binary they just fetched, a PATH user gets
    // it wherever they cd'd. sudo preserves CWD, so no root-home surprise.
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("inventory.json")
}

// ---- login (thin adapter over wfm_core::auth) ------------------------------
//
// Terminal prompts + progress lines live here; the WFM signin transport and
// the JWT crypto/storage live in wfm_core::auth.

use std::io::Write;

fn run_login(args: LoginArgs) -> Result<()> {
    validate_platform(&args.platform)?;
    // --- collect inputs from the user ---
    let email = match args.email {
        Some(e) => e,
        None => {
            eprint!("warframe.market email: ");
            std::io::stderr().flush().ok();
            let mut s = String::new();
            std::io::stdin().read_line(&mut s).context("reading email")?;
            s.trim().to_string()
        }
    };
    if email.is_empty() {
        bail!("Email cannot be empty.");
    }

    let password = rpassword::prompt_password("warframe.market password: ")
        .context("reading password")?;
    if password.is_empty() {
        bail!("Password cannot be empty.");
    }

    eprintln!("→ Bootstrapping session…");
    let (client, csrf_token) = bootstrap_session()?;
    let csrf_token = Sealed::new(&csrf_token, "CSRF");
    eprintln!("→ Got CSRF token ({csrf_token})");

    eprintln!("→ Signing in to warframe.market…");
    let jwt = signin(&client, &email, &password, &args.platform, csrf_token.expose())?;
    let jwt = Sealed::new(&jwt, "JWT");
    eprintln!("→ Got JWT ({jwt}, cookie-auth)");

    // --- encrypt with a passphrase ---
    let passphrase = rpassword::prompt_password(
        "Encryption passphrase (something only you'd type — used to decrypt the JWT later): "
    ).context("reading passphrase")?;
    let confirm = rpassword::prompt_password("Confirm passphrase: ")
        .context("reading passphrase confirmation")?;
    if passphrase != confirm {
        bail!("Passphrases don't match.");
    }
    if passphrase.len() < 12 {
        bail!("Passphrase must be at least 12 characters — it guards your multi-month WFM token against offline brute force.");
    }

    let encrypted = encrypt_jwt(jwt.expose(), &passphrase, &args.platform)?;
    let out_path = args.out.unwrap_or_else(default_jwt_path);
    if let Some(parent) = out_path.parent() {
        fs::create_dir_all(parent).context("creating config directory")?;
        restrict_dir_perms(parent);
        chown_to_real_user(parent);
    }
    let serialized = serde_json::to_vec_pretty(&encrypted)?;
    write_restricted(&out_path, &serialized)?;
    chown_to_real_user(&out_path);

    eprintln!("\n→ Stored encrypted JWT at {}", out_path.display());
    eprintln!("→ Platform: {}", args.platform);
    eprintln!("\nNext: run `wfm-fetch-inventory serve` (in a terminal) and paste the URL");
    eprintln!("it prints into the web app's Companion tab to list items on warframe.market.");
    eprintln!("Re-run `login` whenever the JWT expires (months from now).");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sealed_display_never_contains_the_secret() {
        let jwt = "jwt.super.secret.payload.abc123";
        let sealed = Sealed::new(jwt, "JWT");
        let shown = format!("{sealed}");
        assert!(!shown.contains(jwt), "Display leaked the raw secret: {shown}");
        assert_eq!(shown, format!("<{} chars, JWT>", jwt.len()));
        assert_eq!(sealed.expose(), jwt);
    }
}
