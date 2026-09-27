//! Pure planning commands never authorize or execute account mutations.

use market_domain::dispatch::{DomainRequest, DomainResponse};

#[tauri::command]
pub async fn evaluate_domain(request: DomainRequest) -> Result<DomainResponse, String> {
    tauri::async_runtime::spawn_blocking(move || request.execute())
        .await
        .map_err(|_| "The calculation could not complete. Try again.".to_string())?
}
