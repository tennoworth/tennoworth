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
use std::fs;
use std::io::{IsTerminal, Read};
use std::path::{Path, PathBuf};
use wfm_core::assistant::{
    assistant_rate_limited, assistant_request_too_large, build_assistant_messages, call_deepseek,
    cap_history, deepseek_client, resolve_deepseek_key, short_reason, AssistantRequest,
    AssistantResponse, MAX_ASSISTANT_BODY_BYTES,
};
use wfm_core::auth::{
    bootstrap_session, decrypt_jwt, encrypt_jwt, fetch_wfm_me, signin, validate_platform,
    EncryptedJwt,
};
use wfm_core::inventory::{fetch_inventory_bytes, InventoryScanner};
use wfm_core::listing::{
    bulk_set_visibility, delete_order, execute_plan, fetch_wfm_catalog, list_user_orders,
    run_pending, update_order, PlanRequest, Unlocked, UpdateRequest, VisibilityRequest, MAX_PLATINUM,
};
use wfm_core::pending::{clear_pending, load_pending};
use wfm_core::platform::{chown_to_real_user, restrict_dir_perms, write_restricted};
use wfm_core::util::{browser_client, default_jwt_path, default_pending_path, random_token};

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
    eprintln!("→ Got CSRF token ({} chars)", csrf_token.len());

    eprintln!("→ Signing in to warframe.market…");
    let jwt = signin(&client, &email, &password, &args.platform, &csrf_token)?;
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
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Instant;

fn run_serve(args: ServeArgs) -> Result<()> {
    // --- 1. Peek the stored login WITHOUT decrypting ---
    // The whole point of serve-first: inventory pull (GET /inventory) uses only
    // the in-memory game creds and needs no login, so serve must start and be
    // useful even with no JWT on disk. The platform tag is stored in plaintext
    // in the envelope, so we can read it without the passphrase; the JWT itself
    // stays encrypted until the first listing action actually needs it.
    let jwt_path = args.jwt_path.unwrap_or_else(default_jwt_path);
    // The DeepSeek key file lives alongside the JWT (same config dir), resolved
    // once here so a per-request `--jwt-path` override doesn't need re-resolving.
    let deepseek_key_dir = jwt_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
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
    match resolve_deepseek_key(std::env::var("DEEPSEEK_API_KEY").ok().as_deref(), &deepseek_key_dir) {
        Some(_) => eprintln!("  AI advisor is available (DeepSeek key found)."),
        None => eprintln!(
            "  AI advisor is off — set the DEEPSEEK_API_KEY env var, or write your key\n  \
             (trimmed, no quotes) to {}.",
            deepseek_key_dir.join("deepseek-key").display()
        ),
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
        scanner: InventoryScanner::new(),
        listing: Mutex::new(listing_init),
        deepseek_key_dir,
        assistant_calls: Mutex::new(VecDeque::new()),
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
    /// Single-flight guard for GET /inventory memory scans — a concurrent scan
    /// request gets a busy 503 instead of a second parallel address-space walk.
    scanner: InventoryScanner,
    /// warframe.market listing credentials, unlocked lazily. Inventory pull and
    /// /health never touch this, so serve is fully useful before (or without) a
    /// login. The first listing action decrypts the JWT + warms the catalog.
    listing: Mutex<ListingAuth>,
    /// Directory the DeepSeek key file (`deepseek-key`) is read from — the same
    /// directory the JWT lives in, resolved once at startup.
    deepseek_key_dir: PathBuf,
    /// Sliding-window timestamps of recent /assistant calls. Caps the DeepSeek
    /// call rate (MAX_ASSISTANT_CALLS per ASSISTANT_RATE_WINDOW) so a runaway
    /// client can't burn the user's API credit — see `assistant_rate_limited`.
    assistant_calls: Mutex<VecDeque<Instant>>,
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
    let http = browser_client(60)?;
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
        // Single-flight: a second concurrent /inventory gets a busy 503 rather
        // than launching a redundant parallel walk of the game's address space.
        return match state.scanner.scan(None, None) {
            Ok((bytes, _)) => respond_raw_json(request, 200, bytes),
            Err(e) => respond_json(
                request,
                503,
                &serde_json::json!({ "error": e.into_message() }),
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
        let response = run_pending(&state.pending_path, &unlocked, &mut pending);
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
        let response = execute_plan(&state.pending_path, &unlocked, plan);
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

    // AI advisor chat — proxies to DeepSeek so the API key never reaches the
    // browser. Same auth/CORS treatment as every other route above (the
    // X-Session-Token gate already ran before we got here).
    if path == "/assistant" && matches!(method, tiny_http::Method::Post) {
        let mut body = String::new();
        std::io::Read::take(request.as_reader(), MAX_ASSISTANT_BODY_BYTES)
            .read_to_string(&mut body)
            .context("reading request body")?;
        let req: AssistantRequest = match serde_json::from_str(&body) {
            Ok(v) => v,
            Err(_) => return respond_json(request, 400, &serde_json::json!({"error": "bad_request"})),
        };
        if assistant_request_too_large(&req.question, &req.context) {
            return respond_json(request, 400, &serde_json::json!({"error": "too_large"}));
        }
        let api_key = match resolve_deepseek_key(std::env::var("DEEPSEEK_API_KEY").ok().as_deref(), &state.deepseek_key_dir) {
            Some(k) => k,
            None => return respond_json(request, 503, &serde_json::json!({"error": "no_api_key"})),
        };
        // Call-rate throttle, checked just before the upstream call: a
        // rejected/oversized/keyless request never counts against the budget.
        {
            let mut calls = state.assistant_calls.lock().expect("assistant_calls mutex poisoned");
            if assistant_rate_limited(&mut calls, Instant::now()) {
                return respond_json(
                    request,
                    429,
                    &serde_json::json!({"error": "rate_limited", "detail": "Too many advisor requests — wait a minute and try again."}),
                );
            }
        }
        let AssistantRequest { question, history, context } = req;
        let messages = build_assistant_messages(&context, &cap_history(history), &question);
        let client = match deepseek_client() {
            Ok(c) => c,
            Err(e) => return respond_json(request, 502, &serde_json::json!({"error": "upstream", "detail": short_reason(&e)})),
        };
        return match call_deepseek(&client, &api_key, messages) {
            Ok((answer, usage)) => respond_json(request, 200, &AssistantResponse { answer, usage }),
            Err(e) => respond_json(request, 502, &serde_json::json!({"error": "upstream", "detail": short_reason(&e)})),
        };
    }

    respond_json(
        request,
        404,
        &serde_json::json!({ "error": format!("no route for {method} {path}") }),
    )
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use wfm_core::auth::{CipherParams, KdfParams, JWT_KDF_ITERATIONS};

    fn tmp_path(name: &str) -> PathBuf {
        let mut p = env::temp_dir();
        p.push(format!("wfminv-test-{}-{}.json", std::process::id(), name));
        p
    }

    // --- late-load unlock (login while serve keeps running) --------------

    fn serve_state_for(jwt_path: PathBuf, passphrase_stdin: bool) -> ServeState {
        ServeState {
            platform: Mutex::new("pc".into()),
            session_token: "test-token".into(),
            pending_path: tmp_path("late-load-pending"),
            plan_running: std::sync::atomic::AtomicBool::new(false),
            scanner: InventoryScanner::new(),
            listing: Mutex::new(ListingAuth::Unavailable),
            deepseek_key_dir: env::temp_dir(),
            assistant_calls: Mutex::new(VecDeque::new()),
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
}
