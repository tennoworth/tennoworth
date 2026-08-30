//! The DeepSeek advisor relay (`ask_assistant`, POST /assistant's desktop
//! mirror) - the only command with third-party egress. Key resolution, caps,
//! prompt fencing, and the throttle all live in `wfm_core::assistant`; this
//! is wiring only, so the API key never reaches the webview.

use std::sync::Arc;
use std::time::Instant;
use tauri::State;

use wfm_core::assistant::{
    assistant_rate_limited, assistant_request_too_large, build_assistant_messages, call_deepseek,
    cap_history, deepseek_client, resolve_deepseek_key, short_reason, AssistantErrorCode,
    AssistantMessage, AssistantResponse,
};

use wfm_core::poison::guard;

use crate::wfm_session::{CmdError, WfmSession};

#[tauri::command]
pub async fn ask_assistant(
    session: State<'_, Arc<WfmSession>>,
    question: String,
    history: Vec<AssistantMessage>,
    context: Option<String>,
) -> Result<AssistantResponse, CmdError> {
    let s = Arc::clone(&session);
    tauri::async_runtime::spawn_blocking(move || {
        let context = context.unwrap_or_default();
        if assistant_request_too_large(&question, &context) {
            return Err(CmdError::of(
                AssistantErrorCode::TooLarge.as_str(),
                "Question or context is too large.",
            ));
        }
        let api_key = resolve_deepseek_key(
            std::env::var("DEEPSEEK_API_KEY").ok().as_deref(),
            s.key_dir(),
        )
        .ok_or_else(|| {
            CmdError::of(
                AssistantErrorCode::NoApiKey.as_str(),
                "No DeepSeek API key configured - set DEEPSEEK_API_KEY or the deepseek-key config file.",
            )
        })?;
        // Checked just before the upstream call - a rejected/oversized/keyless
        // request never counts against the budget (same as serve).
        {
            let mut calls = guard(&s.assistant_calls);
            if assistant_rate_limited(&mut calls, Instant::now()) {
                return Err(CmdError::of(
                    AssistantErrorCode::RateLimited.as_str(),
                    "Too many advisor requests - wait a minute and try again.",
                ));
            }
        }
        let messages = build_assistant_messages(&context, &cap_history(history), &question);
        let client = deepseek_client()
            .map_err(|e| CmdError::of(AssistantErrorCode::Upstream.as_str(), short_reason(&e)))?;
        let (answer, usage) = call_deepseek(&client, &api_key, messages)
            .map_err(|e| CmdError::of(AssistantErrorCode::Upstream.as_str(), short_reason(&e)))?;
        Ok(AssistantResponse { answer, usage })
    })
    .await
    .map_err(|e| CmdError::internal(format!("assistant task failed to run: {e}")))?
}
