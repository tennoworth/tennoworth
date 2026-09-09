//! Offline policy validation for publication and deployment.
use std::path::Path;
fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    let file = args
        .get(1)
        .ok_or("usage: wfm-policy <envelope.json> [previous-envelope.json]")?;
    let key = wfm_client::policy::PUBLIC_KEY.ok_or(
        "Build with TENNOWORTH_WFM_POLICY_PUBLIC_KEY set to the dedicated Minisign public key.",
    )?;
    let read = |path: &Path| -> Result<Vec<u8>, String> {
        use std::io::Read;
        let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
        let mut bytes = Vec::new();
        file.take(wfm_client::policy::MAX_POLICY_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        Ok(bytes)
    };
    let (_, policy) =
        wfm_client::policy::verify(&read(Path::new(file))?, key).map_err(|e| e.to_string())?;
    if let Some(prior) = args.get(2) {
        let (_, previous) =
            wfm_client::policy::verify(&read(Path::new(prior))?, key).map_err(|e| e.to_string())?;
        if policy.revision <= previous.revision {
            return Err("Policy publication requires an increasing revision.".into());
        }
    }
    println!("Verified WFM policy revision {}", policy.revision);
    Ok(())
}
fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}
