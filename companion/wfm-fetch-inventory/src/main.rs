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

use anyhow::{anyhow, bail, Context, Result};
use clap::{Args, Parser, Subcommand};
use regex::bytes::Regex;
use reqwest::blocking::Client;
use std::collections::HashMap;
use std::fs;
use std::io::{IsTerminal, Read};
use std::path::{Path, PathBuf};
use std::time::Duration;
use sysinfo::System;

const INVENTORY_URL: &str = "https://api.warframe.com/api/inventory.php";
const WFM_SIGNIN_URL: &str = "https://api.warframe.market/v1/auth/signin";
const WFM_BOOTSTRAP_URL: &str = "https://warframe.market/auth/signin";
// WFM is behind Cloudflare with bot protection. A non-browser UA gets a 1015
// rate-limit error or a JS challenge before our request ever reaches the API.
const BROWSER_UA: &str =
    "Mozilla/5.0 (X11; Linux x86_64; rv:130.0) Gecko/20100101 Firefox/130.0";

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

#[derive(Args, Debug, Default, Clone)]
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

#[derive(Args, Debug)]
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

#[derive(Args, Debug)]
struct ServeArgs {
    /// Override the encrypted-JWT storage location (default: ~/.config/wfminv/wfm-jwt.enc).
    #[arg(long)]
    jwt_path: Option<PathBuf>,

    /// Override the port (default: random ephemeral, recommended).
    #[arg(long, default_value_t = 0)]
    port: u16,

    /// Read the encryption passphrase from stdin instead of prompting on the TTY.
    /// Useful for automation / systemd units. Send a single line.
    #[arg(long)]
    passphrase_stdin: bool,

    /// On startup, open this app URL in the browser pre-connected to this server
    /// (no URL/token copy-paste). Override for local dev, e.g.
    /// `--app-url http://127.0.0.1:5173`.
    #[arg(long, default_value = "https://tennoworth.app")]
    app_url: String,

    /// Don't open a browser on startup (headless / remote serve, or you'll
    /// connect the web app yourself).
    #[arg(long)]
    no_open: bool,
}

struct SessionInfo {
    account_id: String,
    nonce: String,
    build: Option<String>,
    ct: String,
    cred_hits: usize,
    distinct_creds: usize,
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

/// Memory-scan the running game and fetch the raw inventory.json bytes.
/// Uses ONLY the in-memory session creds (accountId + nonce) — never the
/// encrypted JWT — so the inventory path needs no `login`. Silent (no prints):
/// callers add progress output as appropriate. Shared by `fetch` (writes a
/// file) and `serve`'s GET /inventory route (returns the bytes to the browser).
fn fetch_inventory_bytes(
    pid: Option<u32>,
    platform_tag: Option<String>,
) -> Result<(Vec<u8>, SessionInfo)> {
    let pid = match pid {
        Some(p) => p,
        None => find_wf_pid().ok_or_else(|| {
            anyhow!(
                "Warframe doesn't appear to be running.\n\
                 Start the game, log past the title screen, then retry."
            )
        })?,
    };
    let info = scan_session(pid).context("memory scan failed")?;
    let ct = platform_tag.unwrap_or_else(|| info.ct.clone());

    let mut params: Vec<(&str, &str)> =
        vec![("accountId", &info.account_id), ("nonce", &info.nonce), ("ct", &ct)];
    if let Some(b) = &info.build {
        params.push(("appVersion", b.as_str()));
    }
    let client = Client::builder()
        .user_agent(format!(
            "Warframe/{}",
            info.build.as_deref().unwrap_or("unknown")
        ))
        .timeout(Duration::from_secs(60))
        .build()
        .context("building HTTP client")?;
    let resp = client
        .get(INVENTORY_URL)
        .query(&params)
        .send()
        .context("inventory request failed")?;
    let status = resp.status();
    let bytes = resp.bytes().context("reading inventory response")?;
    if !status.is_success() || bytes.len() < 1024 {
        let preview = String::from_utf8_lossy(&bytes[..bytes.len().min(400)]);
        bail!(
            "Inventory endpoint returned HTTP {status} ({} bytes).\nBody:\n{preview}\n\n\
             If the response was small or 4xx, DE may have rotated something.",
            bytes.len()
        );
    }
    Ok((bytes.to_vec(), info))
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

fn find_wf_pid() -> Option<u32> {
    let mut sys = System::new();
    sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    for (pid, process) in sys.processes() {
        if matches_warframe(process) {
            return Some(pid.as_u32());
        }
    }
    None
}

fn matches_warframe(p: &sysinfo::Process) -> bool {
    // /proc/<pid>/comm is capped at 15 chars on Linux, so "Warframe.x64.exe"
    // arrives as "Warframe.x64.ex". Match the un-ambiguous prefix instead.
    let name = p.name().to_string_lossy();
    if name.starts_with("Warframe.x64") || name == "Warframe.exe" {
        return true;
    }
    // Belt-and-braces: check the full exe path (Wine / Proton give a real
    // path; some setups have a different comm than the file name).
    if let Some(exe) = p.exe() {
        let s = exe.to_string_lossy();
        if s.contains("Warframe.x64.exe") || s.ends_with("/Warframe.exe") {
            return true;
        }
    }
    false
}

fn default_out_path() -> PathBuf {
    // The directory the user ran the command from — a manual downloader
    // gets the file next to the binary they just fetched, a PATH user gets
    // it wherever they cd'd. sudo preserves CWD, so no root-home surprise.
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("inventory.json")
}

#[cfg(unix)]
fn real_user_home() -> Option<PathBuf> {
    // Resolve $SUDO_USER's home when invoked via sudo so we don't drop the
    // file into /root.
    if unsafe { libc::geteuid() } != 0 {
        return None;
    }
    let user = std::env::var("SUDO_USER").ok()?;
    let c_user = std::ffi::CString::new(user).ok()?;
    unsafe {
        let pw = libc::getpwnam(c_user.as_ptr());
        if pw.is_null() {
            return None;
        }
        let dir = std::ffi::CStr::from_ptr((*pw).pw_dir);
        Some(PathBuf::from(dir.to_string_lossy().into_owned()))
    }
}

#[cfg(not(unix))]
fn real_user_home() -> Option<PathBuf> {
    None
}

fn dirs_home() -> PathBuf {
    directories::UserDirs::new()
        .and_then(|d| d.home_dir().to_path_buf().into())
        .unwrap_or_else(|| PathBuf::from("."))
}

#[cfg(unix)]
fn chown_to_real_user(path: &std::path::Path) {
    if unsafe { libc::geteuid() } != 0 {
        return;
    }
    let Ok(user) = std::env::var("SUDO_USER") else { return };
    let Ok(c_user) = std::ffi::CString::new(user) else { return };
    let Ok(c_path) = std::ffi::CString::new(path.to_string_lossy().into_owned()) else { return };
    unsafe {
        let pw = libc::getpwnam(c_user.as_ptr());
        if !pw.is_null() {
            libc::chown(c_path.as_ptr(), (*pw).pw_uid, (*pw).pw_gid);
        }
    }
}

#[cfg(not(unix))]
fn chown_to_real_user(_path: &std::path::Path) {}

// ---- login + JWT storage ---------------------------------------------------
//
// WFM auth: POST /v1/auth/signin with {email, password, auth_type: "cookie"},
// X-CSRFToken scraped from the signin page's <meta name="csrf-token">. The JWT
// arrives in `Set-Cookie` (v2 endpoints reject header-style JWTs, so cookie is
// the only flow that works). We encrypt it at rest with AES-256-GCM, key derived
// via PBKDF2-HMAC-SHA256 with 600k iterations (OWASP 2023). The on-disk
// shape mirrors the web app's encrypted-export format so a single human can
// reason about both.

use aes_gcm::aead::{Aead, KeyInit, OsRng};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use hmac::Hmac;
use pbkdf2::pbkdf2;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::io::Write;

const JWT_FORMAT: &str = "wfminv-jwt-v1";
const JWT_KDF_ITERATIONS: u32 = 600_000;

#[derive(Serialize, Deserialize)]
struct EncryptedJwt {
    format: String,
    created: String,
    platform: String,
    kdf: KdfParams,
    cipher: CipherParams,
    ciphertext: String,
}

#[derive(Serialize, Deserialize)]
struct KdfParams {
    name: String,
    hash: String,
    iterations: u32,
    salt: String,
}

#[derive(Serialize, Deserialize)]
struct CipherParams {
    name: String,
    iv: String,
}

fn run_login(args: LoginArgs) -> Result<()> {
    // Reject a mistyped platform up front — an unknown value would otherwise be
    // baked into the encrypted JWT and silently authenticate against the wrong
    // (or a non-existent) WFM market on every later serve.
    const PLATFORMS: [&str; 4] = ["pc", "ps4", "xbox", "switch"];
    if !PLATFORMS.contains(&args.platform.as_str()) {
        bail!(
            "Unknown --platform '{}'. Use one of: {}. (pc covers Steam & Epic.)",
            args.platform,
            PLATFORMS.join(", ")
        );
    }
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

    // --- bootstrap: GET the signin page to populate session cookie + CSRF token ---
    //
    // WFM's signin requires two things in addition to the credentials:
    //   • A `cookie` (session) it sets on the GET of the signin HTML page
    //   • An `X-CSRFToken` header whose value is in <meta name="csrf-token">
    //     embedded in that same HTML response.
    // Discovered May 2026 by inspecting the WFM frontend bundle directly —
    // the API spec doesn't document this.
    eprintln!("→ Bootstrapping session…");
    let client = Client::builder()
        .user_agent(BROWSER_UA)
        .cookie_store(true)
        .timeout(Duration::from_secs(30))
        .build()
        .context("building HTTP client")?;

    let bootstrap = client
        .get(WFM_BOOTSTRAP_URL)
        .send()
        .context("bootstrap GET failed (Cloudflare may have blocked us)")?;
    if !bootstrap.status().is_success() {
        bail!(
            "Bootstrap GET returned HTTP {} — Cloudflare or WFM may have changed.",
            bootstrap.status()
        );
    }
    let bootstrap_html = bootstrap.text().context("reading bootstrap response")?;

    // Cheap regex — we only care about the meta tag, no HTML parsing needed.
    let csrf_re = Regex::new(r#"name="csrf-token"\s+content="([^"]+)""#)
        .expect("static regex");
    let csrf_token = csrf_re
        .captures(bootstrap_html.as_bytes())
        .and_then(|c| c.get(1))
        .map(|m| std::str::from_utf8(m.as_bytes()).unwrap_or("").to_string())
        .ok_or_else(|| {
            anyhow!(
                "Couldn't find <meta name=\"csrf-token\"> on the signin page. \
                 WFM may have changed their auth flow."
            )
        })?;
    eprintln!("→ Got CSRF token ({} chars)", csrf_token.len());

    // --- sign in ---
    eprintln!("→ Signing in to warframe.market…");
    // We sign in with auth_type=cookie. WFM bakes a `csrf_token` claim into
    // the resulting JWT — v2 endpoints (like /v2/order) require this claim
    // type, header-auth JWTs are rejected. The JWT is returned via Set-Cookie.
    let body = serde_json::json!({
        "email": email,
        "password": password,
        "auth_type": "cookie",
    });
    let resp = client
        .post(WFM_SIGNIN_URL)
        .header("Platform", &args.platform)
        .header("Language", "en")
        .header("auth_type", "cookie")
        .header("X-CSRFToken", &csrf_token)
        .json(&body)
        .send()
        .context("signin request failed")?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().unwrap_or_default();
        bail!(
            "Signin failed: HTTP {status}\nResponse body:\n{}",
            &body[..body.len().min(400)]
        );
    }

    // The post-signin JWT comes back in Set-Cookie. Reqwest's cookie store
    // keeps it for subsequent requests, but we need the raw value to encrypt
    // and persist — pull it out of the response headers.
    let jwt = resp
        .headers()
        .get_all("set-cookie")
        .iter()
        .filter_map(|hv| hv.to_str().ok())
        .find_map(|s| {
            // Set-Cookie: JWT=<token>; Domain=...; Path=/; ...
            s.split(';').next()?.strip_prefix("JWT=").map(|s| s.to_string())
        })
        .ok_or_else(|| anyhow!(
            "Signin succeeded but no JWT cookie in response. \
             WFM may have rotated the auth flow."
        ))?;
    if jwt.is_empty() {
        bail!("Empty JWT in Set-Cookie.");
    }
    eprintln!("→ Got JWT ({} chars, cookie-auth)", jwt.len());

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

    let encrypted = encrypt_jwt(&jwt, &passphrase, &args.platform)?;
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

fn encrypt_jwt(jwt: &str, passphrase: &str, platform: &str) -> Result<EncryptedJwt> {
    let mut salt = [0u8; 16];
    let mut iv = [0u8; 12];
    OsRng.fill_bytes(&mut salt);
    OsRng.fill_bytes(&mut iv);

    let mut key_bytes = [0u8; 32];
    pbkdf2::<Hmac<Sha256>>(passphrase.as_bytes(), &salt, JWT_KDF_ITERATIONS, &mut key_bytes)
        .map_err(|e| anyhow!("PBKDF2 failed: {e}"))?;

    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key_bytes));
    let ciphertext = cipher
        .encrypt(Nonce::from_slice(&iv), jwt.as_bytes())
        .map_err(|e| anyhow!("AES-GCM encrypt failed: {e}"))?;

    Ok(EncryptedJwt {
        format: JWT_FORMAT.into(),
        created: chrono_now_iso(),
        platform: platform.into(),
        kdf: KdfParams {
            name: "PBKDF2".into(),
            hash: "SHA-256".into(),
            iterations: JWT_KDF_ITERATIONS,
            salt: B64.encode(salt),
        },
        cipher: CipherParams {
            name: "AES-GCM".into(),
            iv: B64.encode(iv),
        },
        ciphertext: B64.encode(&ciphertext),
    })
}

fn decrypt_jwt(blob: &EncryptedJwt, passphrase: &str) -> Result<String> {
    if blob.format != JWT_FORMAT {
        bail!("Unknown JWT blob format: {}", blob.format);
    }
    let salt = B64.decode(&blob.kdf.salt).context("decoding salt")?;
    let iv = B64.decode(&blob.cipher.iv).context("decoding IV")?;
    let ciphertext = B64.decode(&blob.ciphertext).context("decoding ciphertext")?;

    let mut key_bytes = [0u8; 32];
    pbkdf2::<Hmac<Sha256>>(passphrase.as_bytes(), &salt, blob.kdf.iterations, &mut key_bytes)
        .map_err(|e| anyhow!("PBKDF2 failed: {e}"))?;

    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key_bytes));
    let plaintext = cipher
        .decrypt(Nonce::from_slice(&iv), ciphertext.as_ref())
        .map_err(|_| anyhow!("Wrong passphrase, or the JWT file was modified."))?;
    String::from_utf8(plaintext).context("JWT plaintext was not valid UTF-8")
}

fn default_jwt_path() -> PathBuf {
    let home = real_user_home().unwrap_or_else(dirs_home);
    home.join(".config").join("wfminv").join("wfm-jwt.enc")
}

fn default_pending_path() -> PathBuf {
    let home = real_user_home().unwrap_or_else(dirs_home);
    home.join(".config").join("wfminv").join("pending_plan.json")
}

// Writes `bytes` to `path`, creating the file at 0o600 from the first
// syscall on Unix. This avoids the race window where a default-umask
// (0o644) file exists on disk before a later `restrict_file_perms` call
// tightens it — a window in which another local user can read the
// secret content. On Windows file ACLs are user-scoped by default; fall
// back to plain write there.
fn write_restricted(path: &std::path::Path, bytes: &[u8]) -> Result<()> {
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)
            .with_context(|| format!("creating {} with mode 0600", path.display()))?;
        f.write_all(bytes)
            .with_context(|| format!("writing {}", path.display()))?;
        Ok(())
    }
    #[cfg(not(unix))]
    {
        fs::write(path, bytes)
            .with_context(|| format!("writing {}", path.display()))
    }
}

#[cfg(unix)]
fn restrict_dir_perms(path: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o700));
}

#[cfg(not(unix))]
fn restrict_dir_perms(_path: &std::path::Path) {}

fn chrono_now_iso() -> String {
    // We don't pull in chrono just for this; format manually from SystemTime.
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days_since_epoch = secs / 86400;
    let secs_in_day = secs % 86400;
    let h = secs_in_day / 3600;
    let m = (secs_in_day / 60) % 60;
    let s = secs_in_day % 60;
    let (y, mo, d) = civil_from_days(days_since_epoch as i64);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{m:02}:{s:02}Z")
}

// Howard Hinnant's algorithm — converts days-since-epoch to (year, month, day).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

// ---- serve subcommand -----------------------------------------------------
//
// Loopback HTTP server the web UI uses to bulk-create warframe.market
// listings. We hold the decrypted JWT in memory for the process lifetime;
// the browser never sees it.
//
// Trust:
//   • Server only binds 127.0.0.1, so the network path is local.
//   • A random per-process session token is required in `X-Session-Token`
//     on every non-OPTIONS request — protects against random sites the
//     user might also have open POSTing here.
//   • CORS allows arbitrary origins because origin isn't the protection;
//     the token is.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::thread;

const SERVE_RATE_LIMIT_MS: u64 = 350;
const MAX_PLAN_ITEMS: usize = 50;
const MIN_PLATINUM: u32 = 5;
// Matches WFM's own UI cap (3000) and the browser ListingReviewModal's
// MAX_PLATINUM. Previously 999, which silently blocked maxed-Arcane and
// Galvanized-mod listings that genuinely sell for 1500–2500p.
const MAX_PLATINUM: u32 = 3000;
const SLUG_MISMATCH_GUARD_MULTIPLIER: u32 = 3;

#[derive(Deserialize)]
struct PlanRequest {
    items: Vec<PlanItem>,
}

#[derive(Deserialize, Clone)]
struct PlanItem {
    /// warframe.market url_name.
    slug: String,
    /// Plat the user wants to list at.
    platinum: u32,
    /// How many copies.
    quantity: u32,
    /// "sell" or "buy".
    order_type: String,
    /// false = invisible until manually toggled.
    visible: bool,
    /// Optional rank (for mods / arcanes). When `None`, we use 0 if the
    /// catalog says the item supports ranks, and omit the field otherwise.
    rank: Option<u32>,
    /// Optional subtype (relic refinement, veiled-riven state). When
    /// `None`, we fall back to the catalog's first listed subtype (the
    /// lowest-value default — "intact" for relics, "unrevealed" for
    /// rivens). Omitting the field for items that require it returns 400.
    #[serde(default)]
    subtype: Option<String>,
    /// Reference low_sell from the market snapshot, used for slug-mismatch
    /// detection. Caller is expected to populate this from market.json.
    #[serde(default)]
    reference_low_sell: Option<u32>,
}

#[derive(Serialize)]
struct PlanResponse {
    plan_id: String,
    results: Vec<ItemResult>,
}

#[derive(Serialize)]
struct ItemResult {
    slug: String,
    status: String, // "ok" | "skipped" | "error"
    message: Option<String>,
    /// WFM order id when status = "ok".
    order_id: Option<String>,
}

struct WfmCatalogItem {
    item_id: String,
    /// Human-readable display name from /v2/items i18n.en.name. Used to
    /// enrich GET /orders so the panel doesn't render raw itemIds.
    display_name: String,
    /// Some items (mods, arcanes) accept a `rank` field on POST /v2/order
    /// and **require** that maxRank exists in the catalog. For items
    /// without `maxRank`, sending `rank` at all returns
    /// `app.field.notAllowed` — so we conditionally include the field.
    max_rank: Option<u32>,
    /// Items with multiple variants (relics: intact/exc/fla/rad;
    /// veiled rivens: unrevealed/revealed) require a `subtype` on POST
    /// /v2/order. Without it WFM returns `app.field.required`. We default
    /// to the first listed subtype (lowest-value: intact relic, unrevealed
    /// riven) — the user can pick a different one via the orders panel
    /// after listing succeeds.
    subtypes: Vec<String>,
}

fn run_serve(args: ServeArgs) -> Result<()> {
    // --- 1. Peek the stored login WITHOUT decrypting ---
    // The whole point of serve-first: inventory pull (GET /inventory) uses only
    // the in-memory game creds and needs no login, so serve must start and be
    // useful even with no JWT on disk. The platform tag is stored in plaintext
    // in the envelope, so we can read it without the passphrase; the JWT itself
    // stays encrypted until the first listing action actually needs it.
    let jwt_path = args.jwt_path.unwrap_or_else(default_jwt_path);
    let (listing_init, platform) = if jwt_path.exists() {
        let blob: EncryptedJwt = serde_json::from_slice(&fs::read(&jwt_path)?)
            .with_context(|| format!("reading encrypted JWT from {}", jwt_path.display()))?;
        let platform = blob.platform.clone();
        // --passphrase-stdin is the automation path: stdin may be a one-shot
        // pipe that won't survive to the first request, so capture the line now
        // and unlock eagerly below (fail fast on a bad passphrase).
        let source = if args.passphrase_stdin {
            let mut s = String::new();
            std::io::stdin()
                .read_line(&mut s)
                .context("reading passphrase from stdin")?;
            PassphraseSource::Provided(s.trim_end_matches(['\n', '\r']).to_string())
        } else {
            PassphraseSource::Tty
        };
        (ListingAuth::Locked { blob, source }, platform)
    } else {
        // No login yet — inventory-only. Market data is pc, so report pc.
        (ListingAuth::Unavailable, "pc".to_string())
    };
    let can_list = !matches!(listing_init, ListingAuth::Unavailable);

    // --- 2. Random session token + bind ---
    let session_token = random_token(32);
    let bind = format!("127.0.0.1:{}", args.port);
    let server = tiny_http::Server::http(&bind)
        .map_err(|e| anyhow!("Could not bind {bind}: {e}\nThe port may already be in use — pick another with --port <N> (0 = a random free port)."))?;
    let actual = server
        .server_addr()
        .to_ip()
        .ok_or_else(|| anyhow!("server bound to non-IP address"))?;

    eprintln!("\n  Companion listening on http://{actual}  (platform={platform})");
    eprintln!("  This port is RANDOM and changes every run — it is NOT the website's 5173.");
    eprintln!("\n  ▶ Paste THIS whole line into the web app's Companion tab:");
    eprintln!("      http://{actual}?token={session_token}");
    eprintln!(
        "\n  First connect only: Chrome/Chromium shows an \"allow local network\n  \
         access\" prompt — click Allow, or the app can't reach this server."
    );
    if can_list {
        eprintln!(
            "\n  Listing on warframe.market is available. The first time you list, this\n  \
             terminal will ask for your companion passphrase to unlock it."
        );
    } else {
        eprintln!(
            "\n  Inventory pull is ready. To also create/edit WFM listings from the app,\n  \
             run `wfm-fetch-inventory login` once — listing unlocks on your next attempt,\n  \
             no restart needed."
        );
    }
    eprintln!("\n  Leave this running while you use the app. Ctrl-C (or close the terminal) to stop.\n");

    // Open the web app pre-connected so the user never copies the URL/token.
    // The token rides in the URL fragment (#…) — fragments are never sent to a
    // server, so it stays local. Best-effort: a headless box has no browser.
    if !args.no_open {
        let app = args.app_url.trim_end_matches('/');
        let connect_url = format!("{app}#companion=http://{actual}?token={session_token}");
        match open_in_browser(&connect_url) {
            Ok(()) => eprintln!("  Opened {app} in your browser — it should connect automatically."),
            Err(_) => eprintln!("  (Couldn't open a browser — paste the line above into the Companion tab.)"),
        }
    }

    // --- 4. Serve requests ---
    let state = Arc::new(ServeState {
        platform: Mutex::new(platform),
        session_token,
        pending_path: default_pending_path(),
        plan_running: std::sync::atomic::AtomicBool::new(false),
        listing: Mutex::new(listing_init),
        jwt_path,
        passphrase_stdin: args.passphrase_stdin,
    });

    // Automation path: unlock now so a bad --passphrase-stdin fails at startup
    // rather than on the first listing request.
    if args.passphrase_stdin && can_list {
        if let Err(e) = ensure_unlocked(&state) {
            bail!("listing unlock failed: {}", e.into_message());
        }
    }
    for request in server.incoming_requests() {
        let state = Arc::clone(&state);
        // One thread per request — workload is light, listing batches are
        // bounded to MAX_PLAN_ITEMS so threads exit quickly.
        thread::spawn(move || {
            if let Err(e) = handle_request(request, &state) {
                eprintln!("request handler error: {e:#}");
            }
        });
    }
    Ok(())
}

// Cap request bodies so a malformed/hostile local client can't make us allocate
// an unbounded String before parsing. 64 KB dwarfs any real plan (MAX_PLAN_ITEMS=50).
const MAX_BODY_BYTES: u64 = 64 * 1024;

// Resets the plan-in-flight flag on scope exit (incl. early return / panic) so a
// rejected or crashed request can't leave plan execution wedged.
struct PlanGuard<'a>(&'a std::sync::atomic::AtomicBool);
impl Drop for PlanGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, std::sync::atomic::Ordering::SeqCst);
    }
}

struct ServeState {
    /// Platform shown by /health. Starts as the startup snapshot ("pc" when no
    /// login existed) and is updated to the JWT's real market when listing
    /// unlocks — including a login loaded late without restarting serve. Listing
    /// API calls do NOT read this; they use the platform on `Unlocked`.
    platform: Mutex<String>,
    session_token: String,
    pending_path: PathBuf,
    /// Serializes plan execution: a second concurrent POST /plan or /plan/resume
    /// gets 409 instead of racing on pending_plan.json and clobbering recovery.
    plan_running: std::sync::atomic::AtomicBool,
    /// warframe.market listing credentials, unlocked lazily. Inventory pull and
    /// /health never touch this, so serve is fully useful before (or without) a
    /// login. The first listing action decrypts the JWT + warms the catalog.
    listing: Mutex<ListingAuth>,
    /// Resolved encrypted-JWT path, kept so ensure_unlocked can re-read it: a
    /// `login` that lands while serve keeps running is picked up on the next
    /// listing attempt, no restart.
    jwt_path: PathBuf,
    /// Whether serve was started with --passphrase-stdin. A login that arrives
    /// late can't be unlocked in that mode (stdin was a one-shot pipe), so we
    /// fail with an actionable "restart serve" message instead of hanging.
    passphrase_stdin: bool,
}

/// Where the decryption passphrase comes from when we unlock listing.
enum PassphraseSource {
    /// Captured from stdin at startup (--passphrase-stdin, automation).
    Provided(String),
    /// Prompt on the controlling terminal at first-use.
    Tty,
}

/// Lifecycle of the listing credentials inside a running serve.
enum ListingAuth {
    /// No JWT on disk — listing is off until the user runs `login`.
    Unavailable,
    /// JWT present but not yet decrypted.
    Locked {
        blob: EncryptedJwt,
        source: PassphraseSource,
    },
    /// Decrypted JWT + warmed catalog, ready to hit WFM.
    Unlocked(Arc<Unlocked>),
}

/// Everything a listing request needs, produced once on first use.
struct Unlocked {
    jwt: String,
    username: String,
    /// The market the JWT authenticates against (pc / ps4 / xbox / switch).
    /// Carried with the credential so every listing call sends a Platform header
    /// consistent with the JWT, even when serve's startup snapshot said "pc"
    /// (no login on disk at startup, then a console login loaded late).
    platform: String,
    catalog: Arc<BTreeMap<String, WfmCatalogItem>>,
    /// itemId → display name. Injected into the /orders response so the UI
    /// doesn't show raw 24-char hex IDs.
    id_to_name: Arc<BTreeMap<String, String>>,
}

/// Why a listing route couldn't get credentials — lets handle_request answer
/// with the right status (401 "log in first" vs a transient 502).
enum UnlockError {
    NeedsLogin,
    Failed(anyhow::Error),
}

impl UnlockError {
    fn into_message(self) -> String {
        match self {
            UnlockError::NeedsLogin => {
                "Listing needs a warframe.market login. Run `wfm-fetch-inventory login`, \
                 then try listing again."
                    .to_string()
            }
            UnlockError::Failed(e) => format!("{e:#}"),
        }
    }
}

/// Returns the unlocked listing credentials, decrypting + warming the catalog on
/// first call. Holds the listing mutex across the (possibly interactive) unlock
/// so concurrent listing requests can't double-prompt — the second waits, then
/// sees the already-unlocked state.
fn ensure_unlocked(state: &ServeState) -> std::result::Result<Arc<Unlocked>, UnlockError> {
    let mut guard = state.listing.lock().expect("listing mutex poisoned");
    match &*guard {
        ListingAuth::Unlocked(u) => return Ok(Arc::clone(u)),
        ListingAuth::Unavailable => {
            // Serve started with no JWT on disk. Re-check now — the user may have
            // run `login` while serve kept running — so listing unlocks without a
            // restart. Kept under the listing mutex with the rest of the unlock so
            // concurrent listing calls can't double-prompt or race the transition.
            match late_load_locked(state)? {
                Some(locked) => *guard = locked,
                None => return Err(UnlockError::NeedsLogin),
            }
        }
        ListingAuth::Locked { .. } => {}
    }
    // Take ownership of the Locked payload for the fallible unlock, leaving a
    // placeholder. Restore it on failure so a later retry (mistyped passphrase,
    // network blip) can try again instead of being stuck Unavailable.
    let (blob, source) = match std::mem::replace(&mut *guard, ListingAuth::Unavailable) {
        ListingAuth::Locked { blob, source } => (blob, source),
        _ => unreachable!("checked Locked above"),
    };
    match build_unlocked(&blob, &source) {
        Ok(u) => {
            let arc = Arc::new(u);
            // Keep /health's displayed platform honest: a late-loaded blob may be
            // a console market, not serve's startup "pc" default.
            *state.platform.lock().expect("platform mutex poisoned") = arc.platform.clone();
            *guard = ListingAuth::Unlocked(Arc::clone(&arc));
            Ok(arc)
        }
        Err(e) => {
            *guard = ListingAuth::Locked { blob, source };
            Err(UnlockError::Failed(e))
        }
    }
}

/// Re-reads the encrypted-JWT path so a `login` that landed after serve started
/// is picked up on the next listing attempt. Returns:
/// - `Ok(None)` — still no login file (→ NeedsLogin, unchanged behaviour).
/// - `Ok(Some(Locked))` — a valid blob to feed the normal unlock flow.
/// - `Err(Failed)` — file present but unreadable/unparseable. `login` writes it
///   with truncate+write (`write_restricted`), so a concurrent read can catch a
///   transient partial file; that's 503-retryable, NOT NeedsLogin.
fn late_load_locked(state: &ServeState) -> std::result::Result<Option<ListingAuth>, UnlockError> {
    let bytes = match fs::read(&state.jwt_path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            return Err(UnlockError::Failed(anyhow::Error::new(e).context(format!(
                "reading encrypted JWT from {}",
                state.jwt_path.display()
            ))))
        }
    };
    let blob: EncryptedJwt = serde_json::from_slice(&bytes).map_err(|e| {
        UnlockError::Failed(anyhow::Error::new(e).context(
            "the login file is present but not yet readable (a concurrent `login` \
             may still be writing it) — try listing again",
        ))
    })?;
    let source = late_load_source(state)?;
    Ok(Some(ListingAuth::Locked { blob, source }))
}

/// Passphrase source for a login that arrived while serve was already running.
/// An interactive serve prompts on its terminal. A --passphrase-stdin serve
/// only read stdin at startup *if* a blob existed then (it didn't, or we
/// wouldn't be Unavailable), so its pipe is gone — it can't unlock a late login
/// and must be restarted.
fn late_load_source(state: &ServeState) -> std::result::Result<PassphraseSource, UnlockError> {
    if std::io::stdin().is_terminal() {
        Ok(PassphraseSource::Tty)
    } else if state.passphrase_stdin {
        Err(UnlockError::Failed(anyhow!(
            "A warframe.market login was detected, but serve was started with \
             --passphrase-stdin and no login existed then, so there is no passphrase \
             to unlock it. Restart serve to unlock listing."
        )))
    } else {
        // No tty and no stdin passphrase: build_unlocked's Tty branch fails with
        // its own actionable "start serve in a terminal" message.
        Ok(PassphraseSource::Tty)
    }
}

fn build_unlocked(blob: &EncryptedJwt, source: &PassphraseSource) -> Result<Unlocked> {
    let platform = blob.platform.clone();
    let jwt = match source {
        PassphraseSource::Provided(p) => decrypt_jwt(blob, p)?,
        PassphraseSource::Tty => {
            // Only now do we need a real terminal — serve itself started fine
            // without one (inventory-only). Fail with an actionable message
            // rather than rpassword's cryptic os-error-6.
            if !std::io::stdin().is_terminal() {
                bail!(
                    "Listing needs your passphrase, but serve has no interactive terminal.\n\
                     Start serve in a terminal, or pass --passphrase-stdin."
                );
            }
            eprintln!("\n  ▶ Unlocking warframe.market listing — enter your companion passphrase:");
            let mut attempt = 0;
            loop {
                attempt += 1;
                let pass = rpassword::prompt_password("Encryption passphrase: ")
                    .context("reading passphrase")?;
                match decrypt_jwt(blob, &pass) {
                    Ok(jwt) => break jwt,
                    Err(e) if attempt < 3 => eprintln!("  {e}  ({} attempt(s) left)", 3 - attempt),
                    Err(e) => return Err(e),
                }
            }
        }
    };
    eprintln!("→ Decrypted JWT ({} chars, platform={})", jwt.len(), platform);
    eprintln!("→ Loading WFM item catalog…");
    let http = Client::builder()
        .user_agent(BROWSER_UA)
        .timeout(Duration::from_secs(60))
        .build()
        .context("building HTTP client")?;
    let catalog = fetch_wfm_catalog(&http, &platform)?;
    eprintln!("  {} items in catalog", catalog.len());
    let id_to_name: BTreeMap<String, String> = catalog
        .values()
        .map(|c| (c.item_id.clone(), c.display_name.clone()))
        .collect();
    let username = fetch_wfm_me(&http, &jwt, &platform)?;
    eprintln!("→ Signed in as {username}");
    Ok(Unlocked {
        jwt,
        username,
        platform,
        catalog: Arc::new(catalog),
        id_to_name: Arc::new(id_to_name),
    })
}

// Persisted between requests so a crash mid-batch doesn't lose work. The
// browser polls /plan/pending on (re)connect and offers Resume / Discard.
// Atomic-writes via tmp + rename so a concurrent read never sees a torn file
// — same convention as `os.replace` in wfm_demand.py.
#[derive(Serialize, Deserialize, Clone)]
struct PendingPlan {
    plan_id: String,
    started_at: String,
    items: Vec<PendingItem>,
}

#[derive(Serialize, Deserialize, Clone)]
struct PendingItem {
    slug: String,
    platinum: u32,
    quantity: u32,
    order_type: String,
    visible: bool,
    rank: Option<u32>,
    #[serde(default)]
    subtype: Option<String>,
    #[serde(default)]
    reference_low_sell: Option<u32>,
    /// "pending" | "ok" | "error"
    status: String,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    order_id: Option<String>,
}

fn write_pending_atomic(path: &Path, plan: &PendingPlan) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).ok();
        restrict_dir_perms(parent);
        chown_to_real_user(parent);
    }
    let tmp = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec(plan).context("serializing pending plan")?;
    // Create the tmp file at 0o600 from the first syscall — pending plans
    // contain unsubmitted listing details, not OK to leak to other local
    // users even briefly.
    write_restricted(&tmp, &bytes)?;
    fs::rename(&tmp, path)
        .with_context(|| format!("renaming {} → {}", tmp.display(), path.display()))?;
    // chown the final path back to the real user so a sudo invocation of
    // `serve` doesn't leave a root-owned file in their config dir.
    chown_to_real_user(path);
    Ok(())
}

fn load_pending(path: &Path) -> Option<PendingPlan> {
    let data = fs::read(path).ok()?;
    serde_json::from_slice(&data).ok()
}

fn clear_pending(path: &Path) {
    let _ = fs::remove_file(path);
}

#[derive(Deserialize)]
struct VisibilityRequest {
    order_ids: Vec<String>,
    visible: bool,
}

#[derive(Deserialize)]
struct UpdateRequest {
    platinum: Option<u32>,
    quantity: Option<u32>,
    visible: Option<bool>,
    rank: Option<u32>,
}

#[derive(Serialize)]
struct PerOrderResult {
    order_id: String,
    status: String, // "ok" | "error"
    message: Option<String>,
}

fn handle_request(
    mut request: tiny_http::Request,
    state: &ServeState,
) -> Result<()> {
    let method = request.method().clone();
    let url = request.url().to_string();
    let path = url.split('?').next().unwrap_or("/");

    // CORS preflight — browser hits this before the real POST.
    if matches!(method, tiny_http::Method::Options) {
        return respond_cors_preflight(request);
    }

    // Health endpoint — no auth required.
    if path == "/health" && matches!(method, tiny_http::Method::Get) {
        let platform = state.platform.lock().expect("platform mutex poisoned").clone();
        return respond_json(
            request,
            200,
            &serde_json::json!({ "ok": true, "platform": platform }),
        );
    }

    // Every other route requires the session token. Constant-time compare —
    // `str ==` short-circuits on the first mismatching byte. Loopback timing
    // attacks against a per-process token are far-fetched, but the fix is one
    // line and `subtle` is already in the dependency tree via aes-gcm.
    let token_ok = request.headers().iter().any(|h| {
        use subtle::ConstantTimeEq;
        h.field.equiv("X-Session-Token")
            && h.value.as_str().as_bytes().ct_eq(state.session_token.as_bytes()).into()
    });
    if !token_ok {
        return respond_json(
            request,
            401,
            &serde_json::json!({ "error": "missing or invalid X-Session-Token" }),
        );
    }

    // Inventory pull — memory-scan the running game and hand inventory.json
    // straight to the browser, so the user never touches a file. Uses ONLY the
    // in-memory session creds (accountId + nonce); the decrypted JWT is never
    // involved. 503 with an actionable message when the game isn't scannable.
    if path == "/inventory" && matches!(method, tiny_http::Method::Get) {
        return match fetch_inventory_bytes(None, None) {
            Ok((bytes, _)) => respond_raw_json(request, 200, bytes),
            Err(e) => respond_json(
                request,
                503,
                &serde_json::json!({ "error": format!("{e:#}") }),
            ),
        };
    }

    if path == "/plan/pending" {
        match method {
            tiny_http::Method::Get => {
                return match load_pending(&state.pending_path) {
                    Some(p) => respond_json(request, 200, &p),
                    None => respond_json(request, 404, &serde_json::json!({"error": "no pending plan"})),
                };
            }
            tiny_http::Method::Delete => {
                clear_pending(&state.pending_path);
                return respond_json(request, 200, &serde_json::json!({"ok": true}));
            }
            _ => return respond_json(request, 405, &serde_json::json!({"error": "method not allowed"})),
        }
    }

    if path == "/plan/resume" && matches!(method, tiny_http::Method::Post) {
        let mut pending = match load_pending(&state.pending_path) {
            Some(p) => p,
            None => return respond_json(request, 404, &serde_json::json!({"error": "no pending plan"})),
        };
        let unlocked = match ensure_unlocked(state) {
            Ok(u) => u,
            Err(e) => return respond_unlock_error(request, e),
        };
        let _guard = match state.plan_running.compare_exchange(
            false, true,
            std::sync::atomic::Ordering::SeqCst,
            std::sync::atomic::Ordering::SeqCst,
        ) {
            Ok(_) => PlanGuard(&state.plan_running),
            Err(_) => return respond_json(request, 409, &serde_json::json!({"error": "a plan is already running; retry after it finishes"})),
        };
        let response = run_pending(state, &unlocked, &mut pending);
        clear_pending(&state.pending_path);
        return respond_json(request, 200, &response);
    }

    if path == "/plan" && matches!(method, tiny_http::Method::Post) {
        let mut body = String::new();
        std::io::Read::take(request.as_reader(), MAX_BODY_BYTES).read_to_string(&mut body).context("reading request body")?;
        let plan: PlanRequest = match serde_json::from_str(&body) {
            Ok(p) => p,
            Err(e) => {
                return respond_json(
                    request,
                    400,
                    &serde_json::json!({ "error": format!("malformed plan: {e}") }),
                );
            }
        };
        let unlocked = match ensure_unlocked(state) {
            Ok(u) => u,
            Err(e) => return respond_unlock_error(request, e),
        };
        // Reject a concurrent plan instead of letting two threads race on
        // pending_plan.json. The guard clears the flag on any return path.
        let _guard = match state.plan_running.compare_exchange(
            false, true,
            std::sync::atomic::Ordering::SeqCst,
            std::sync::atomic::Ordering::SeqCst,
        ) {
            Ok(_) => PlanGuard(&state.plan_running),
            Err(_) => return respond_json(request, 409, &serde_json::json!({"error": "a plan is already running; retry after it finishes"})),
        };
        let response = execute_plan(state, &unlocked, plan);
        return respond_json(request, 200, &response);
    }

    if path == "/orders" && matches!(method, tiny_http::Method::Get) {
        let unlocked = match ensure_unlocked(state) {
            Ok(u) => u,
            Err(e) => return respond_unlock_error(request, e),
        };
        return match list_user_orders(&unlocked) {
            Ok(v) => respond_json(request, 200, &v),
            Err(e) => respond_json(request, 502, &serde_json::json!({"error": e.to_string()})),
        };
    }

    if path == "/orders/visibility" && matches!(method, tiny_http::Method::Post) {
        let mut body = String::new();
        std::io::Read::take(request.as_reader(), MAX_BODY_BYTES).read_to_string(&mut body).context("reading request body")?;
        let req: VisibilityRequest = match serde_json::from_str(&body) {
            Ok(v) => v,
            Err(e) => return respond_json(request, 400, &serde_json::json!({"error": format!("malformed: {e}")})),
        };
        let unlocked = match ensure_unlocked(state) {
            Ok(u) => u,
            Err(e) => return respond_unlock_error(request, e),
        };
        let results = bulk_set_visibility(&unlocked, &req);
        return respond_json(request, 200, &serde_json::json!({"results": results}));
    }

    if let Some(id) = path.strip_prefix("/order/") {
        // Slashes shouldn't appear in WFM order ids; guard anyway.
        if id.contains('/') || id.is_empty() {
            return respond_json(request, 400, &serde_json::json!({"error": "bad order id"}));
        }
        match method {
            tiny_http::Method::Delete => {
                let unlocked = match ensure_unlocked(state) {
                    Ok(u) => u,
                    Err(e) => return respond_unlock_error(request, e),
                };
                return match delete_order(&unlocked, id) {
                    Ok(_) => respond_json(request, 200, &serde_json::json!({"ok": true})),
                    Err(e) => respond_json(request, 502, &serde_json::json!({"error": e.to_string()})),
                };
            }
            tiny_http::Method::Patch => {
                let mut body = String::new();
                std::io::Read::take(request.as_reader(), MAX_BODY_BYTES).read_to_string(&mut body).context("reading request body")?;
                let upd: UpdateRequest = match serde_json::from_str(&body) {
                    Ok(v) => v,
                    Err(e) => return respond_json(request, 400, &serde_json::json!({"error": format!("malformed: {e}")})),
                };
                // Same cap as the create path (build_order_body) — without it,
                // an edit could push a listing past what the WFM UI allows, and
                // WFM's rejection comes back as a per-order error the browser
                // used to swallow.
                if let Some(p) = upd.platinum {
                    if p > MAX_PLATINUM {
                        return respond_json(request, 400, &serde_json::json!({"error": format!("price {p}p > max {MAX_PLATINUM}p")}));
                    }
                }
                let unlocked = match ensure_unlocked(state) {
                    Ok(u) => u,
                    Err(e) => return respond_unlock_error(request, e),
                };
                return match update_order(&unlocked, id, &upd) {
                    Ok(v) => respond_json(request, 200, &v),
                    Err(e) => respond_json(request, 502, &serde_json::json!({"error": e.to_string()})),
                };
            }
            _ => {
                return respond_json(request, 405, &serde_json::json!({"error": "method not allowed"}));
            }
        }
    }

    respond_json(
        request,
        404,
        &serde_json::json!({ "error": format!("no route for {method} {path}") }),
    )
}

// ---- WFM API helpers ------------------------------------------------------

fn fetch_wfm_me(client: &Client, jwt: &str, platform: &str) -> Result<String> {
    let resp = client
        .get("https://api.warframe.market/v2/me")
        .header("Platform", platform)
        .header("Language", "en")
        .header("Crossplay", "true")
        .header("Cookie", format!("JWT={jwt}"))
        .header("Origin", "https://warframe.market")
        .header("Referer", "https://warframe.market/")
        .send()
        .context("/v2/me request failed")?;
    let status = resp.status();
    let body: serde_json::Value = resp.json().context("parsing /v2/me")?;
    if !status.is_success() {
        bail!("/v2/me returned {status}: {body}");
    }
    body.pointer("/data/slug")
        .or_else(|| body.pointer("/data/ingameName"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow!("/v2/me response shape unexpected: {body}"))
}

fn list_user_orders(unlocked: &Unlocked) -> Result<serde_json::Value> {
    let client = wfm_client()?;
    let url = format!(
        "https://api.warframe.market/v2/orders/user/{}",
        unlocked.username
    );
    let resp = client
        .get(&url)
        .header("Platform", &unlocked.platform)
        .header("Language", "en")
        .header("Crossplay", "true")
        .header("Cookie", format!("JWT={}", unlocked.jwt))
        .header("Origin", "https://warframe.market")
        .header("Referer", "https://warframe.market/")
        .send()
        .context("/v2/orders/user request failed")?;
    let status = resp.status();
    let mut body: serde_json::Value = resp.json().context("parsing orders response")?;
    if !status.is_success() {
        bail!("WFM HTTP {status}: {body}");
    }
    enrich_orders_with_names(&mut body, &unlocked.id_to_name);
    Ok(body)
}

// WFM /v2/orders/user/<username> returns orders that carry `itemId` but no
// display name. The MyOrdersPanel falls all the way through to the raw id
// without this. We mutate the response in place to attach
// `item: { name, slug }` per order, looked up against the catalog we already
// loaded at startup. Tolerates both shapes WFM has shipped:
//   { data: { sell: [...], buy: [...] } }   ← current v2
//   { data: [...] }                          ← flat list, occasional v1-ish
fn enrich_orders_with_names(body: &mut serde_json::Value, id_to_name: &BTreeMap<String, String>) {
    let Some(data) = body.get_mut("data") else { return };
    if let Some(arr) = data.as_array_mut() {
        for o in arr {
            attach_item_name(o, id_to_name);
        }
        return;
    }
    for bucket in ["sell", "buy"] {
        if let Some(arr) = data.get_mut(bucket).and_then(|v| v.as_array_mut()) {
            for o in arr {
                attach_item_name(o, id_to_name);
            }
        }
    }
}

fn attach_item_name(order: &mut serde_json::Value, id_to_name: &BTreeMap<String, String>) {
    let id = order
        .get("itemId")
        .and_then(|v| v.as_str())
        .or_else(|| order.get("item_id").and_then(|v| v.as_str()))
        .map(|s| s.to_string());
    let Some(id) = id else { return };
    let Some(name) = id_to_name.get(&id) else { return };
    if let Some(obj) = order.as_object_mut() {
        // Don't clobber if WFM has started including item metadata on its own.
        if !obj.contains_key("item") {
            obj.insert("item".into(), serde_json::json!({ "name": name }));
        } else if let Some(item_obj) = obj.get_mut("item").and_then(|v| v.as_object_mut()) {
            if !item_obj.contains_key("name") {
                item_obj.insert("name".into(), serde_json::json!(name));
            }
        }
    }
}

fn bulk_set_visibility(unlocked: &Unlocked, req: &VisibilityRequest) -> Vec<PerOrderResult> {
    let client = match wfm_client() {
        Ok(c) => c,
        Err(e) => {
            return req.order_ids.iter().map(|id| PerOrderResult {
                order_id: id.clone(),
                status: "error".into(),
                message: Some(format!("client: {e}")),
            }).collect();
        }
    };
    let mut out = Vec::with_capacity(req.order_ids.len());
    let mut last = std::time::Instant::now() - Duration::from_millis(SERVE_RATE_LIMIT_MS);
    for id in &req.order_ids {
        let elapsed = last.elapsed();
        if elapsed < Duration::from_millis(SERVE_RATE_LIMIT_MS) {
            thread::sleep(Duration::from_millis(SERVE_RATE_LIMIT_MS) - elapsed);
        }
        last = std::time::Instant::now();
        out.push(patch_one_order(&client, unlocked, id, &serde_json::json!({"visible": req.visible})));
    }
    out
}

fn update_order(unlocked: &Unlocked, id: &str, upd: &UpdateRequest) -> Result<PerOrderResult> {
    let client = wfm_client()?;
    let mut body = serde_json::Map::new();
    if let Some(v) = upd.platinum { body.insert("platinum".into(), serde_json::json!(v)); }
    if let Some(v) = upd.quantity { body.insert("quantity".into(), serde_json::json!(v)); }
    if let Some(v) = upd.visible  { body.insert("visible".into(),  serde_json::json!(v)); }
    if let Some(v) = upd.rank     { body.insert("rank".into(),     serde_json::json!(v)); }
    if body.is_empty() {
        bail!("update body has no fields to patch");
    }
    Ok(patch_one_order(&client, unlocked, id, &serde_json::Value::Object(body)))
}

fn patch_one_order(
    client: &Client,
    unlocked: &Unlocked,
    id: &str,
    body: &serde_json::Value,
) -> PerOrderResult {
    let url = format!("https://api.warframe.market/v2/order/{id}");
    let resp = client
        .patch(&url)
        .header("Platform", &unlocked.platform)
        .header("Language", "en")
        .header("Crossplay", "true")
        .header("Cookie", format!("JWT={}", unlocked.jwt))
        .header("Origin", "https://warframe.market")
        .header("Referer", "https://warframe.market/")
        .json(body)
        .send();
    match resp {
        Ok(r) => {
            let status = r.status();
            if status.is_success() {
                PerOrderResult { order_id: id.into(), status: "ok".into(), message: None }
            } else {
                let body: serde_json::Value = r.json().unwrap_or(serde_json::Value::Null);
                PerOrderResult {
                    order_id: id.into(),
                    status: "error".into(),
                    message: Some(format!("HTTP {status}: {}", body.get("error").map(|v| v.to_string()).unwrap_or_else(|| "(no message)".into()))),
                }
            }
        }
        Err(e) => PerOrderResult {
            order_id: id.into(),
            status: "error".into(),
            message: Some(format!("HTTP request failed: {e}")),
        },
    }
}

fn delete_order(unlocked: &Unlocked, id: &str) -> Result<()> {
    let client = wfm_client()?;
    let url = format!("https://api.warframe.market/v2/order/{id}");
    let resp = client
        .delete(&url)
        .header("Platform", &unlocked.platform)
        .header("Language", "en")
        .header("Crossplay", "true")
        .header("Cookie", format!("JWT={}", unlocked.jwt))
        .header("Origin", "https://warframe.market")
        .header("Referer", "https://warframe.market/")
        .send()
        .context("DELETE request failed")?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().unwrap_or_default();
        bail!("WFM HTTP {status}: {}", &body[..body.len().min(300)]);
    }
    Ok(())
}

fn wfm_client() -> Result<Client> {
    Client::builder()
        .user_agent(BROWSER_UA)
        .timeout(Duration::from_secs(30))
        .build()
        .context("building HTTP client")
}

fn respond_cors_preflight(request: tiny_http::Request) -> Result<()> {
    use tiny_http::Header;
    let response = tiny_http::Response::empty(204)
        .with_header(Header::from_bytes(&b"Access-Control-Allow-Origin"[..], &b"*"[..]).unwrap())
        .with_header(Header::from_bytes(&b"Access-Control-Allow-Methods"[..], &b"GET, POST, PATCH, DELETE, OPTIONS"[..]).unwrap())
        .with_header(Header::from_bytes(&b"Access-Control-Allow-Headers"[..], &b"Content-Type, X-Session-Token"[..]).unwrap())
        .with_header(Header::from_bytes(&b"Access-Control-Max-Age"[..], &b"600"[..]).unwrap());
    request.respond(response).context("sending CORS preflight")?;
    Ok(())
}

fn respond_json<T: serde::Serialize>(
    request: tiny_http::Request,
    status: u16,
    body: &T,
) -> Result<()> {
    use tiny_http::Header;
    let json = serde_json::to_string(body).context("serializing response")?;
    let response = tiny_http::Response::from_string(json)
        .with_status_code(status)
        .with_header(Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap())
        .with_header(Header::from_bytes(&b"Access-Control-Allow-Origin"[..], &b"*"[..]).unwrap());
    request.respond(response).context("sending response")?;
    Ok(())
}

// Turn a failed listing-unlock into an HTTP response. NeedsLogin is a 401 with
// a `needs_login` flag the browser keys off to steer the user to `login`; a
// decrypt/network failure is a 503 (transient — retrying may work).
fn respond_unlock_error(request: tiny_http::Request, err: UnlockError) -> Result<()> {
    let needs_login = matches!(err, UnlockError::NeedsLogin);
    let status = if needs_login { 401 } else { 503 };
    respond_json(
        request,
        status,
        &serde_json::json!({ "error": err.into_message(), "needs_login": needs_login }),
    )
}

// Send already-serialized JSON bytes (e.g. the upstream inventory.json) without
// a parse+reserialize round trip. Same CORS/content-type as respond_json.
fn respond_raw_json(request: tiny_http::Request, status: u16, body: Vec<u8>) -> Result<()> {
    use tiny_http::Header;
    let response = tiny_http::Response::from_data(body)
        .with_status_code(status)
        .with_header(Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap())
        .with_header(Header::from_bytes(&b"Access-Control-Allow-Origin"[..], &b"*"[..]).unwrap());
    request.respond(response).context("sending response")?;
    Ok(())
}

/// Open `url` in the user's default browser. Spawns and returns immediately —
/// never blocks the server. Best-effort: errors if no opener exists (e.g. a
/// headless box), which the caller treats as non-fatal.
fn open_in_browser(url: &str) -> Result<()> {
    use std::process::{Command, Stdio};
    #[cfg(target_os = "linux")]
    let mut cmd = { let mut c = Command::new("xdg-open"); c.arg(url); c };
    #[cfg(target_os = "windows")]
    let mut cmd = {
        // `start` is a cmd builtin; the "" is the window title. Our URL is a
        // loopback host + digits + base64url token — no cmd-special characters.
        let mut c = Command::new("cmd");
        c.args(["/C", "start", "", url]);
        c
    };
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("launching browser")?;
    Ok(())
}

fn random_token(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    OsRng.fill_bytes(&mut buf);
    B64.encode(&buf)
        .replace('+', "-")
        .replace('/', "_")
        .trim_end_matches('=')
        .to_string()
}

fn fetch_wfm_catalog(client: &Client, platform: &str) -> Result<BTreeMap<String, WfmCatalogItem>> {
    // v1 retired; v2 returns a flat `data` array of {id, slug, ...}.
    // Order creation is v2 as well (POST /v2/order, see build_order_body).
    let resp = client
        .get("https://api.warframe.market/v2/items")
        .header("Platform", platform)
        .header("Language", "en")
        .header("Crossplay", "true")
        .send()
        .context("fetching /v2/items")?;
    if !resp.status().is_success() {
        bail!("/v2/items returned HTTP {}", resp.status());
    }
    let body: serde_json::Value = resp.json().context("parsing /v2/items")?;
    let items = body
        .get("data")
        .and_then(|v| v.as_array())
        .ok_or_else(|| anyhow!("/v2/items response shape changed (no top-level data array)"))?;
    let mut out = BTreeMap::new();
    for it in items {
        let id = it.get("id").and_then(|v| v.as_str()).unwrap_or("");
        let slug = it.get("slug").and_then(|v| v.as_str()).unwrap_or("");
        if !id.is_empty() && !slug.is_empty() {
            let display_name = it
                .pointer("/i18n/en/name")
                .and_then(|v| v.as_str())
                .unwrap_or(slug)
                .to_string();
            let max_rank = it.get("maxRank").and_then(|v| v.as_u64()).map(|n| n as u32);
            let subtypes: Vec<String> = it
                .get("subtypes")
                .and_then(|v| v.as_array())
                .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                .unwrap_or_default();
            out.insert(slug.to_string(), WfmCatalogItem {
                item_id: id.to_string(),
                display_name,
                max_rank,
                subtypes,
            });
        }
    }
    Ok(out)
}

fn execute_plan(state: &ServeState, unlocked: &Unlocked, plan: PlanRequest) -> PlanResponse {
    let plan_id = random_token(8);

    if plan.items.is_empty() {
        return PlanResponse { plan_id, results: vec![] };
    }

    // --- enforced caps (defense in depth — the browser also validates) ---
    if plan.items.len() > MAX_PLAN_ITEMS {
        return PlanResponse {
            plan_id,
            results: vec![ItemResult {
                slug: "<batch>".into(),
                status: "error".into(),
                message: Some(format!(
                    "Batch has {} items; companion cap is {MAX_PLAN_ITEMS}.",
                    plan.items.len()
                )),
                order_id: None,
            }],
        };
    }

    // Seed the pending file before the first POST so a crash here is
    // recoverable — the browser polls /plan/pending on next connect.
    let mut pending = PendingPlan {
        plan_id: plan_id.clone(),
        started_at: chrono_now_iso(),
        items: plan.items.into_iter().map(|p| PendingItem {
            slug: p.slug,
            platinum: p.platinum,
            quantity: p.quantity,
            order_type: p.order_type,
            visible: p.visible,
            rank: p.rank,
            subtype: p.subtype,
            reference_low_sell: p.reference_low_sell,
            status: "pending".into(),
            message: None,
            order_id: None,
        }).collect(),
    };
    if let Err(e) = write_pending_atomic(&state.pending_path, &pending) {
        eprintln!("warning: could not seed pending plan: {e:#}");
    }

    let response = run_pending(state, unlocked, &mut pending);
    clear_pending(&state.pending_path);
    response
}

// Drives a PendingPlan to completion, skipping items already in a terminal
// state (ok / error). Used both by the initial /plan POST and /plan/resume.
// Rewrites the on-disk pending file atomically after every item so a crash
// at any point leaves a consistent record.
fn run_pending(state: &ServeState, unlocked: &Unlocked, pending: &mut PendingPlan) -> PlanResponse {
    let http = match Client::builder()
        .user_agent(BROWSER_UA)
        .timeout(Duration::from_secs(30))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            return PlanResponse {
                plan_id: pending.plan_id.clone(),
                results: vec![ItemResult {
                    slug: "<batch>".into(),
                    status: "error".into(),
                    message: Some(format!("HTTP client build failed: {e}")),
                    order_id: None,
                }],
            };
        }
    };

    let mut last_call = std::time::Instant::now()
        - Duration::from_millis(SERVE_RATE_LIMIT_MS);
    for i in 0..pending.items.len() {
        if pending.items[i].status != "pending" {
            continue;
        }
        let since = last_call.elapsed();
        if since < Duration::from_millis(SERVE_RATE_LIMIT_MS) {
            thread::sleep(Duration::from_millis(SERVE_RATE_LIMIT_MS) - since);
        }
        last_call = std::time::Instant::now();

        let plan_item = PlanItem {
            slug: pending.items[i].slug.clone(),
            platinum: pending.items[i].platinum,
            quantity: pending.items[i].quantity,
            order_type: pending.items[i].order_type.clone(),
            visible: pending.items[i].visible,
            rank: pending.items[i].rank,
            subtype: pending.items[i].subtype.clone(),
            reference_low_sell: pending.items[i].reference_low_sell,
        };
        let result = execute_one(&http, unlocked, &plan_item);
        pending.items[i].status = result.status.clone();
        pending.items[i].message = result.message.clone();
        pending.items[i].order_id = result.order_id.clone();
        if let Err(e) = write_pending_atomic(&state.pending_path, pending) {
            eprintln!("warning: could not persist pending update: {e:#}");
        }
    }

    PlanResponse {
        plan_id: pending.plan_id.clone(),
        results: pending.items.iter().map(|i| ItemResult {
            slug: i.slug.clone(),
            status: i.status.clone(),
            message: i.message.clone(),
            order_id: i.order_id.clone(),
        }).collect(),
    }
}

// Maximum items per single in-game trade — six slots per side in Warframe's
// trade window. WFM rejects `perTrade` values above this with
// `app.field.tooBig` (verified on a real relic listing, May 2026).
const MAX_PER_TRADE: u32 = 6;

// `perTrade` must EVENLY DIVIDE `quantity` on bulk-tradable items (relics
// and similar). Listing qty=27 with perTrade=6 returns
// `app.field.orders.perTradeMustDivideQuantity` because 27/6 is not an
// integer. We pick the largest divisor of `quantity` that fits under
// MAX_PER_TRADE. Examples:
//   qty=27 → 3   (divisors: 1, 3, 9, 27; only 3 fits ≤ 6)
//   qty=10 → 5   (1, 2, 5, 10; 5 is the largest ≤ 6)
//   qty=12 → 6   (1, 2, 3, 4, 6, 12; 6 fits exactly)
//   qty=7  → 1   (1, 7; only 1 fits)
//   qty=1  → 1
fn per_trade_for(quantity: u32) -> u32 {
    if quantity == 0 {
        return 1;
    }
    let start = quantity.min(MAX_PER_TRADE);
    for d in (1..=start).rev() {
        if quantity % d == 0 {
            return d;
        }
    }
    1
}

// Constructs the JSON body for `POST /v2/order`. Per-field rules captured
// from WFM 400 responses (May 2026):
//   - `itemId`, `type` (not `order_type`!), `platinum`, `quantity`,
//     `visible` are always required.
//   - `perTrade` is always required and capped at 6 (in-game trade
//     slots). Listings with quantity > 6 still work — buyers just split
//     across multiple trades. We default to min(quantity, 6).
//   - `rank` is required for items with `maxRank` in the catalog, and is
//     `app.field.notAllowed` for items without it. Default to 0 (unranked).
//   - `subtype` is required for items with `subtypes[]` in the catalog.
//     Default to the first listed subtype — that's the lowest-value
//     variant by WFM convention (intact relic, unrevealed riven) and
//     matches what the user almost always wants to dump first.
fn build_order_body(item: &PlanItem, cat: &WfmCatalogItem) -> serde_json::Value {
    let mut body = serde_json::Map::new();
    body.insert("itemId".into(), serde_json::json!(cat.item_id));
    body.insert("type".into(), serde_json::json!(item.order_type));
    body.insert("platinum".into(), serde_json::json!(item.platinum));
    body.insert("quantity".into(), serde_json::json!(item.quantity));
    body.insert("visible".into(), serde_json::json!(item.visible));
    body.insert("perTrade".into(), serde_json::json!(per_trade_for(item.quantity)));
    if cat.max_rank.is_some() {
        body.insert("rank".into(), serde_json::json!(item.rank.unwrap_or(0)));
    }
    if !cat.subtypes.is_empty() {
        let chosen = item
            .subtype
            .clone()
            .filter(|s| cat.subtypes.contains(s))
            .unwrap_or_else(|| cat.subtypes[0].clone());
        body.insert("subtype".into(), serde_json::json!(chosen));
    }
    serde_json::Value::Object(body)
}

fn execute_one(http: &Client, unlocked: &Unlocked, item: &PlanItem) -> ItemResult {
    let mk_err = |msg: String| ItemResult {
        slug: item.slug.clone(),
        status: "error".into(),
        message: Some(msg),
        order_id: None,
    };

    // --- safety caps ---
    if item.platinum < MIN_PLATINUM {
        return mk_err(format!("price {}p < min {MIN_PLATINUM}p", item.platinum));
    }
    if item.platinum > MAX_PLATINUM {
        return mk_err(format!("price {}p > max {MAX_PLATINUM}p", item.platinum));
    }
    if let Some(low) = item.reference_low_sell {
        if low > 0 && low > item.platinum * SLUG_MISMATCH_GUARD_MULTIPLIER {
            return mk_err(format!(
                "ref low_sell {low}p is more than {SLUG_MISMATCH_GUARD_MULTIPLIER}× our {}p; \
                 likely a slug mismatch — refusing",
                item.platinum
            ));
        }
    }
    if !matches!(item.order_type.as_str(), "sell" | "buy") {
        return mk_err(format!("order_type {:?} not in (sell, buy)", item.order_type));
    }
    if item.quantity == 0 {
        return mk_err("quantity must be > 0".into());
    }

    // --- resolve slug → item_id ---
    let cat = match unlocked.catalog.get(&item.slug) {
        Some(c) => c,
        None => return mk_err(format!("slug {:?} not in WFM catalog", item.slug)),
    };
    let body = build_order_body(item, cat);

    // Order-creation endpoint (verified via the WFM frontend's actual
    // network call, May 2026): POST /v2/order. Singular. /v2/me/orders
    // returns 404 for POST — that path is for GET-list semantics, not
    // create. v2 endpoints rely on the JWT cookie that the website sets
    // (not the Authorization header). We send both so either auth path
    // works — the WFM server picks whichever it understands for v1 vs v2.
    // Header set captured from the live frontend's preflight:
    //   access-control-request-headers: content-type, crossplay, language, platform
    // It uses pure cookie auth — no Authorization header. We mirror that.
    let resp = http
        .post("https://api.warframe.market/v2/order")
        .header("Platform", &unlocked.platform)
        .header("Language", "en")
        .header("Crossplay", "true")
        .header("Cookie", format!("JWT={}", unlocked.jwt))
        .header("Origin", "https://warframe.market")
        .header("Referer", "https://warframe.market/")
        .json(&body)
        .send();
    let resp = match resp {
        Ok(r) => r,
        Err(e) => return mk_err(format!("HTTP request failed: {e}")),
    };
    let status = resp.status();
    let resp_body: serde_json::Value = resp.json().unwrap_or(serde_json::Value::Null);
    if !status.is_success() {
        // v2 puts errors under `.error` (object or array of strings); v1 used
        // a top-level `.error` string. Render whatever we can find verbatim
        // so the user sees the real validation message.
        let msg = resp_body
            .get("error")
            .map(|v| v.to_string())
            .unwrap_or_else(|| "(no error message)".to_string());
        return mk_err(format!("WFM HTTP {status}: {msg}"));
    }
    // v2 returns the created order under .data; v1 used .payload.order. Try both.
    let order_id = resp_body
        .pointer("/data/id")
        .or_else(|| resp_body.pointer("/payload/order/id"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    ItemResult {
        slug: item.slug.clone(),
        status: "ok".into(),
        message: None,
        order_id,
    }
}

// ---- pattern scanning ------------------------------------------------------

fn cred_re() -> Regex {
    // Confirmed in May 2026 memory scan: this exact form appears in the URLs
    // the game sends. Update here if DE ever rotates the parameter names.
    // ASCII [0-9] (not \d) so we don't need the regex crate's unicode-perl
    // feature — saves ~150 KB on the binary.
    Regex::new(r"accountId=([0-9a-fA-F]{24})&nonce=([0-9]{6,})").unwrap()
}

fn build_re() -> Regex {
    Regex::new(r#""BuildLabel":"([0-9.]+)/[A-Za-z0-9]+"#).unwrap()
}

fn ct_re() -> Regex {
    Regex::new(r"&ct=([A-Z]{2,4})\b").unwrap()
}

fn aggregate_match<'a>(haystack: &'a [u8], counts: &mut PatternCounts) {
    for cap in cred_re().captures_iter(haystack) {
        let aid = String::from_utf8_lossy(&cap[1]).to_ascii_lowercase();
        let nonce = String::from_utf8_lossy(&cap[2]).into_owned();
        *counts.creds.entry((aid, nonce)).or_insert(0) += 1;
    }
    for cap in build_re().captures_iter(haystack) {
        *counts
            .builds
            .entry(String::from_utf8_lossy(&cap[1]).into_owned())
            .or_insert(0) += 1;
    }
    for cap in ct_re().captures_iter(haystack) {
        *counts
            .cts
            .entry(String::from_utf8_lossy(&cap[1]).into_owned())
            .or_insert(0) += 1;
    }
}

#[derive(Default)]
struct PatternCounts {
    creds: HashMap<(String, String), usize>,
    builds: HashMap<String, usize>,
    cts: HashMap<String, usize>,
}

fn pick_dominant(counts: PatternCounts) -> Result<SessionInfo> {
    if counts.creds.is_empty() {
        bail!(
            "No accountId/nonce pair found in WF memory.\n\
             Make sure you're past the login screen and a recent network\n\
             call has fired (opening the trade or profile screen is reliable)."
        );
    }
    let total_distinct = counts.creds.len();
    let ((aid, nonce), hits) = counts
        .creds
        .into_iter()
        .max_by_key(|(_, v)| *v)
        .expect("non-empty checked above");
    let build = counts
        .builds
        .into_iter()
        .max_by_key(|(_, v)| *v)
        .map(|(k, _)| k);
    let ct = counts
        .cts
        .into_iter()
        .max_by_key(|(_, v)| *v)
        .map(|(k, _)| k)
        .unwrap_or_else(|| "STM".to_string());
    Ok(SessionInfo {
        account_id: aid,
        nonce,
        build,
        ct,
        cred_hits: hits,
        distinct_creds: total_distinct,
    })
}

// ---- Linux ---------------------------------------------------------------

#[cfg(target_os = "linux")]
fn scan_session(pid: u32) -> Result<SessionInfo> {
    use std::fs::File;
    use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};

    let maps_path = format!("/proc/{pid}/maps");
    let mem_path = format!("/proc/{pid}/mem");

    let maps_file = File::open(&maps_path)
        .with_context(|| format!("cannot open {maps_path} — does PID {pid} exist?"))?;
    let mut mem_file =
        File::open(&mem_path).map_err(|e| ptrace_open_error(&mem_path, pid, e))?;

    let mut counts = PatternCounts::default();
    let mut buf = vec![0u8; 4 * 1024 * 1024];
    let mut tail: Vec<u8> = Vec::new();
    let overlap = 96;

    let skip_substrings = ["[vvar]", "[vsyscall]", "[vdso]", "/dev/", "/SYSV"];

    for line in BufReader::new(maps_file).lines() {
        let line = line?;
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 5 {
            continue;
        }
        let addr_range = parts[0];
        let perms = parts[1];
        let path = if parts.len() >= 6 { parts[5] } else { "" };
        if !perms.contains('r') {
            continue;
        }
        if skip_substrings.iter().any(|s| path.contains(s)) {
            continue;
        }
        let (start_s, end_s) = match addr_range.split_once('-') {
            Some(p) => p,
            None => continue,
        };
        let start: u64 = u64::from_str_radix(start_s, 16)?;
        let end: u64 = u64::from_str_radix(end_s, 16)?;
        let mut offset = start;
        tail.clear();
        while offset < end {
            let want = std::cmp::min(buf.len() as u64, end - offset) as usize;
            if mem_file.seek(SeekFrom::Start(offset)).is_err() {
                break;
            }
            let n = match mem_file.read(&mut buf[..want]) {
                Ok(0) => break,
                Ok(n) => n,
                Err(_) => break,
            };
            // Concatenate small overlap from previous chunk so a pattern
            // straddling the boundary still matches.
            let mut hay: Vec<u8> = Vec::with_capacity(tail.len() + n);
            hay.extend_from_slice(&tail);
            hay.extend_from_slice(&buf[..n]);
            aggregate_match(&hay, &mut counts);
            tail.clear();
            let keep = std::cmp::min(overlap, n);
            tail.extend_from_slice(&buf[n - keep..n]);
            offset += n as u64;
        }
    }

    pick_dominant(counts)
}

// Turn a /proc/<pid>/mem open failure into actionable guidance. Permission
// denied is the common case (no CAP_SYS_PTRACE) and we lead with the
// grant-once setcap path so users never need sudo again; anything else
// usually means the PID exited between lookup and read.
#[cfg(target_os = "linux")]
fn ptrace_open_error(mem_path: &str, pid: u32, e: std::io::Error) -> anyhow::Error {
    if e.kind() != std::io::ErrorKind::PermissionDenied {
        return anyhow!(
            "cannot open {mem_path}: {e}\n\
             PID {pid} may have exited — restart Warframe past the title screen and retry."
        );
    }
    let bin = std::env::current_exe()
        .ok()
        .and_then(|p| p.to_str().map(str::to_owned))
        .unwrap_or_else(|| "wfm-fetch-inventory".to_string());
    let mut msg = format!(
        "Permission denied reading {mem_path} — reading the game's memory needs CAP_SYS_PTRACE.\n\
         Grant it once (no sudo needed afterwards):\n  \
         sudo setcap cap_sys_ptrace=eip \"{bin}\"\n  \
         {bin}\n\
         Or run this one invocation with sudo:\n  \
         sudo {bin}\n\
         Note: re-installing or rebuilding the binary clears the capability — re-run setcap after an upgrade."
    );
    // Yama scope 3 disables ptrace entirely; even a capable binary can't attach.
    if matches!(
        std::fs::read_to_string("/proc/sys/kernel/yama/ptrace_scope"),
        Ok(s) if s.trim() == "3"
    ) {
        msg.push_str(
            "\n\nkernel.yama.ptrace_scope is 3 (ptrace disabled) — setcap alone won't help.\n\
             Lower it until reboot:\n  sudo sysctl kernel.yama.ptrace_scope=1",
        );
    }
    anyhow!(msg)
}

// ---- Windows -------------------------------------------------------------

#[cfg(target_os = "windows")]
fn scan_session(pid: u32) -> Result<SessionInfo> {
    use windows::Win32::Foundation::{CloseHandle, BOOL, HANDLE};
    use windows::Win32::System::Memory::{
        VirtualQueryEx, MEMORY_BASIC_INFORMATION, MEM_COMMIT, PAGE_GUARD, PAGE_NOACCESS,
    };
    use windows::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ,
    };
    use windows::Win32::System::Diagnostics::Debug::ReadProcessMemory;

    unsafe {
        let handle: HANDLE = OpenProcess(
            PROCESS_QUERY_INFORMATION | PROCESS_VM_READ,
            BOOL(0),
            pid,
        )
        .context("OpenProcess failed — not running as same user, or pid is wrong")?;

        let mut counts = PatternCounts::default();
        let mut addr: usize = 0;
        let mut mbi = MEMORY_BASIC_INFORMATION::default();
        let mbi_size = std::mem::size_of::<MEMORY_BASIC_INFORMATION>();

        loop {
            let q = VirtualQueryEx(
                handle,
                Some(addr as *const _),
                &mut mbi,
                mbi_size,
            );
            if q == 0 {
                break;
            }
            let next = mbi.BaseAddress as usize + mbi.RegionSize;
            let readable = mbi.State == MEM_COMMIT
                && (mbi.Protect.0 & (PAGE_NOACCESS.0 | PAGE_GUARD.0)) == 0;
            if readable {
                let mut buf = vec![0u8; mbi.RegionSize];
                let mut read_n: usize = 0;
                let ok = ReadProcessMemory(
                    handle,
                    mbi.BaseAddress,
                    buf.as_mut_ptr() as *mut _,
                    mbi.RegionSize,
                    Some(&mut read_n),
                );
                if ok.is_ok() && read_n > 0 {
                    aggregate_match(&buf[..read_n], &mut counts);
                }
            }
            addr = next;
            if addr == 0 {
                break;
            }
        }

        let _ = CloseHandle(handle);
        pick_dominant(counts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    fn sample_plan() -> PendingPlan {
        PendingPlan {
            plan_id: "abc12345".into(),
            started_at: "2026-05-27T15:30:00Z".into(),
            items: vec![
                PendingItem {
                    slug: "loki_prime_set".into(),
                    platinum: 120,
                    quantity: 1,
                    order_type: "sell".into(),
                    visible: false,
                    rank: None,
                    subtype: None,
                    reference_low_sell: Some(110),
                    status: "ok".into(),
                    message: None,
                    order_id: Some("order-1".into()),
                },
                PendingItem {
                    slug: "rhino_prime_set".into(),
                    platinum: 95,
                    quantity: 1,
                    order_type: "sell".into(),
                    visible: false,
                    rank: None,
                    subtype: None,
                    reference_low_sell: Some(90),
                    status: "pending".into(),
                    message: None,
                    order_id: None,
                },
            ],
        }
    }

    fn tmp_path(name: &str) -> PathBuf {
        let mut p = env::temp_dir();
        p.push(format!("wfminv-test-{}-{}.json", std::process::id(), name));
        p
    }

    #[test]
    fn pending_plan_roundtrips_through_disk() {
        let path = tmp_path("roundtrip");
        let plan = sample_plan();
        write_pending_atomic(&path, &plan).unwrap();

        let loaded = load_pending(&path).expect("file readable");
        assert_eq!(loaded.plan_id, plan.plan_id);
        assert_eq!(loaded.items.len(), 2);
        assert_eq!(loaded.items[0].status, "ok");
        assert_eq!(loaded.items[1].status, "pending");

        clear_pending(&path);
        assert!(load_pending(&path).is_none());
    }

    #[test]
    fn load_pending_returns_none_when_missing() {
        let path = tmp_path("missing");
        let _ = std::fs::remove_file(&path);
        assert!(load_pending(&path).is_none());
    }

    #[test]
    fn load_pending_tolerates_missing_optional_fields() {
        // older file written before rank/reference_low_sell were optional, or
        // a hand-edit. Deserialization must still succeed.
        let path = tmp_path("partial");
        let raw = r#"{
            "plan_id":"x","started_at":"t","items":[
              {"slug":"a","platinum":5,"quantity":1,"order_type":"sell","visible":false,"rank":null,"status":"pending"}
            ]}"#;
        std::fs::write(&path, raw).unwrap();
        let loaded = load_pending(&path).expect("parses");
        assert_eq!(loaded.items.len(), 1);
        assert!(loaded.items[0].order_id.is_none());
        assert!(loaded.items[0].reference_low_sell.is_none());
        clear_pending(&path);
    }

    // --- late-load unlock (login while serve keeps running) --------------

    fn serve_state_for(jwt_path: PathBuf, passphrase_stdin: bool) -> ServeState {
        ServeState {
            platform: Mutex::new("pc".into()),
            session_token: "test-token".into(),
            pending_path: tmp_path("late-load-pending"),
            plan_running: std::sync::atomic::AtomicBool::new(false),
            listing: Mutex::new(ListingAuth::Unavailable),
            jwt_path,
            passphrase_stdin,
        }
    }

    fn encrypted_jwt_bytes(platform: &str) -> Vec<u8> {
        let blob = EncryptedJwt {
            format: "wfminv-jwt-v1".into(),
            created: "2026-07-19T00:00:00Z".into(),
            platform: platform.into(),
            kdf: KdfParams {
                name: "PBKDF2".into(),
                hash: "SHA-256".into(),
                iterations: JWT_KDF_ITERATIONS,
                salt: "AAAAAAAAAAAAAAAAAAAAAA==".into(),
            },
            cipher: CipherParams {
                name: "AES-256-GCM".into(),
                iv: "AAAAAAAAAAAAAAAA".into(),
            },
            ciphertext: "AAAAAAAA".into(),
        };
        serde_json::to_vec(&blob).unwrap()
    }

    #[test]
    fn late_load_returns_none_when_login_still_absent() {
        let path = tmp_path("late-load-absent");
        let _ = std::fs::remove_file(&path);
        let state = serve_state_for(path, false);
        assert!(matches!(late_load_locked(&state), Ok(None)));
        // ensure_unlocked maps that to NeedsLogin (the 401 the browser keys off).
        assert!(matches!(ensure_unlocked(&state), Err(UnlockError::NeedsLogin)));
    }

    #[test]
    fn late_load_partial_file_is_failed_not_needs_login() {
        // `login` truncates then writes, so a concurrent read can see a partial
        // file. That must be 503-retryable, never a NeedsLogin 401.
        let path = tmp_path("late-load-partial");
        std::fs::write(&path, br#"{"format":"wfminv-jwt-v1","platf"#).unwrap();
        let state = serve_state_for(path.clone(), false);
        assert!(matches!(late_load_locked(&state), Err(UnlockError::Failed(_))));
        assert!(matches!(ensure_unlocked(&state), Err(UnlockError::Failed(_))));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn late_load_success_carries_blob_platform() {
        // Success path up to the network boundary: a valid blob that landed after
        // startup transitions Unavailable → Locked carrying the JWT's real
        // platform (here a console market, not serve's "pc" default). The actual
        // decrypt + catalog warm in build_unlocked needs WFM and isn't offline.
        let path = tmp_path("late-load-ok");
        std::fs::write(&path, encrypted_jwt_bytes("ps4")).unwrap();
        let state = serve_state_for(path.clone(), false);
        match late_load_locked(&state) {
            Ok(Some(ListingAuth::Locked { blob, .. })) => assert_eq!(blob.platform, "ps4"),
            _ => panic!("expected a Locked state carrying the blob's ps4 platform"),
        }
        let _ = std::fs::remove_file(&path);
    }

    fn sample_id_map() -> BTreeMap<String, String> {
        let mut m = BTreeMap::new();
        m.insert("54aae292e7798909064f1575".into(), "Secura Dual Cestra".into());
        m.insert("aaaaaaaaaaaaaaaaaaaaaaaa".into(), "Loki Prime Set".into());
        m
    }

    #[test]
    fn enrich_orders_handles_split_sell_buy_shape() {
        let mut body = serde_json::json!({
            "data": {
                "sell": [
                    {"id": "o1", "itemId": "aaaaaaaaaaaaaaaaaaaaaaaa", "platinum": 120},
                ],
                "buy": [
                    {"id": "o2", "itemId": "54aae292e7798909064f1575", "platinum": 5},
                ]
            }
        });
        enrich_orders_with_names(&mut body, &sample_id_map());
        assert_eq!(body["data"]["sell"][0]["item"]["name"], "Loki Prime Set");
        assert_eq!(body["data"]["buy"][0]["item"]["name"], "Secura Dual Cestra");
    }

    #[test]
    fn enrich_orders_handles_flat_array_shape() {
        let mut body = serde_json::json!({
            "data": [
                {"id": "o1", "itemId": "aaaaaaaaaaaaaaaaaaaaaaaa", "platinum": 120},
            ]
        });
        enrich_orders_with_names(&mut body, &sample_id_map());
        assert_eq!(body["data"][0]["item"]["name"], "Loki Prime Set");
    }

    #[test]
    fn enrich_orders_leaves_unknown_ids_alone() {
        let mut body = serde_json::json!({
            "data": { "sell": [{ "id": "o1", "itemId": "deadbeef", "platinum": 9 }] }
        });
        enrich_orders_with_names(&mut body, &sample_id_map());
        // No `item` key injected because the id wasn't in the catalog.
        assert!(body["data"]["sell"][0].get("item").is_none());
    }

    fn cat(name: &str, max_rank: Option<u32>, subtypes: &[&str]) -> WfmCatalogItem {
        WfmCatalogItem {
            item_id: format!("id-{name}"),
            display_name: name.into(),
            max_rank,
            subtypes: subtypes.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn plan_item(slug: &str, rank: Option<u32>, subtype: Option<&str>) -> PlanItem {
        PlanItem {
            slug: slug.into(),
            platinum: 12,
            quantity: 3,
            order_type: "sell".into(),
            visible: false,
            rank,
            subtype: subtype.map(|s| s.into()),
            reference_low_sell: None,
        }
    }

    #[test]
    fn order_body_for_relic_includes_subtype_omits_rank() {
        // Reproducer for the May 2026 400: {"rank":"app.field.notAllowed",
        // "subtype":"app.field.required","perTrade":"app.field.required"}.
        let cat = cat("neo_b2_relic", None, &["intact", "exceptional", "flawless", "radiant"]);
        let item = plan_item("neo_b2_relic", None, None);
        let body = build_order_body(&item, &cat);
        assert_eq!(body["itemId"], "id-neo_b2_relic");
        assert_eq!(body["type"], "sell");
        assert_eq!(body["platinum"], 12);
        assert_eq!(body["quantity"], 3);
        assert_eq!(body["visible"], false);
        assert_eq!(body["perTrade"], 3);
        assert_eq!(body["subtype"], "intact");          // default to first
        assert!(body.get("rank").is_none(), "rank must be absent for non-rankable items");
    }

    #[test]
    fn order_body_for_mod_includes_rank_omits_subtype() {
        let cat = cat("creeping_bullseye", Some(5), &[]);
        let item = plan_item("creeping_bullseye", None, None);
        let body = build_order_body(&item, &cat);
        assert_eq!(body["rank"], 0); // default for unmaxed
        assert!(body.get("subtype").is_none());
    }

    #[test]
    fn order_body_respects_explicit_rank_for_mods() {
        let cat = cat("creeping_bullseye", Some(5), &[]);
        let item = plan_item("creeping_bullseye", Some(5), None);
        let body = build_order_body(&item, &cat);
        assert_eq!(body["rank"], 5);
    }

    #[test]
    fn order_body_uses_user_subtype_when_valid() {
        let cat = cat("neo_b2_relic", None, &["intact", "exceptional", "flawless", "radiant"]);
        let item = plan_item("neo_b2_relic", None, Some("radiant"));
        let body = build_order_body(&item, &cat);
        assert_eq!(body["subtype"], "radiant");
    }

    #[test]
    fn order_body_falls_back_to_first_when_user_subtype_invalid() {
        // Don't silently send a bogus subtype WFM will reject.
        let cat = cat("neo_b2_relic", None, &["intact", "exceptional", "flawless", "radiant"]);
        let item = plan_item("neo_b2_relic", None, Some("super-radiant"));
        let body = build_order_body(&item, &cat);
        assert_eq!(body["subtype"], "intact");
    }

    #[test]
    fn per_trade_picks_largest_divisor_under_cap() {
        // Reproducer for `app.field.orders.perTradeMustDivideQuantity` —
        // WFM rejects when perTrade does not evenly divide quantity.
        assert_eq!(per_trade_for(27), 3);  // {1,3,9,27} ∩ ≤6 → 3
        assert_eq!(per_trade_for(10), 5);  // {1,2,5,10} ∩ ≤6 → 5
        assert_eq!(per_trade_for(12), 6);  // {1,2,3,4,6,12} ∩ ≤6 → 6
        assert_eq!(per_trade_for(6),  6);  // exact fit
        assert_eq!(per_trade_for(7),  1);  // prime > 6 → only 1 divides
        assert_eq!(per_trade_for(11), 1);  // prime > 6 → 1
        assert_eq!(per_trade_for(1),  1);
        assert_eq!(per_trade_for(0),  1);  // defensive
    }

    #[test]
    fn order_body_per_trade_divides_quantity_for_27_relic_stack() {
        // Reproducer for the May 2026 400 on a 27-relic stack:
        // {"inputs":{"perTrade":"app.field.orders.perTradeMustDivideQuantity"}}.
        // perTrade must EVENLY DIVIDE quantity. Largest divisor of 27 ≤ 6 is 3.
        let cat = cat("neo_b2_relic", None, &["intact", "exceptional", "flawless", "radiant"]);
        let mut item = plan_item("neo_b2_relic", None, None);
        item.quantity = 27;
        let body = build_order_body(&item, &cat);
        assert_eq!(body["quantity"], 27);
        assert_eq!(body["perTrade"], 3);
        // Sanity: 27 must divide perfectly.
        assert_eq!(body["quantity"].as_u64().unwrap() % body["perTrade"].as_u64().unwrap(), 0);
    }

    #[test]
    fn order_body_per_trade_uses_quantity_when_quantity_under_cap() {
        let cat = cat("ash_prime_set", None, &[]);
        let mut item = plan_item("ash_prime_set", None, None);
        item.quantity = 3;
        let body = build_order_body(&item, &cat);
        assert_eq!(body["perTrade"], 3);
    }

    #[test]
    fn enrich_orders_preserves_existing_item_metadata() {
        // If WFM ever starts returning `item` itself, don't clobber.
        let mut body = serde_json::json!({
            "data": { "sell": [{
                "id": "o1",
                "itemId": "aaaaaaaaaaaaaaaaaaaaaaaa",
                "item": { "name": "Custom Name", "icon": "x.png" },
            }]}
        });
        enrich_orders_with_names(&mut body, &sample_id_map());
        assert_eq!(body["data"]["sell"][0]["item"]["name"], "Custom Name");
        assert_eq!(body["data"]["sell"][0]["item"]["icon"], "x.png");
    }

    #[cfg(unix)]
    #[test]
    fn write_restricted_creates_file_at_0600_from_first_syscall() {
        use std::os::unix::fs::PermissionsExt;
        let path = tmp_path("perms");
        let _ = std::fs::remove_file(&path);
        write_restricted(&path, b"hello").unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "file must be created at 0600, not chmod'd later");
        clear_pending(&path);
    }

    #[test]
    fn atomic_write_leaves_no_tmp_file() {
        let path = tmp_path("notmp");
        write_pending_atomic(&path, &sample_plan()).unwrap();
        let tmp = path.with_extension("json.tmp");
        assert!(!tmp.exists(), "tmp file should be renamed away");
        assert!(path.exists());
        clear_pending(&path);
    }
}

