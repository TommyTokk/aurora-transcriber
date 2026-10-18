//! Reserved, catalog-scoped model selection, provisioning and removal commands.
use super::{fetcher, manifest, model_files, AsrModelId, EngineSnapshot, EngineState};
use crate::{model_storage::ModelStorage, settings::TranscriptionSettings};
use tauri::Manager;

/// A validated operation whose full lifetime belongs to the shared model reservation.
pub(crate) enum ModelAction {
    Select(AsrModelId),
    Download(String),
    Delete(String),
}

/// Executes one operation on a worker and always releases its reservation, including worker failures.
pub(crate) async fn perform<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    action: ModelAction,
) -> Result<EngineSnapshot, String> {
    app.state::<EngineState>().reserve()?;
    let worker = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || apply(&worker, action))
        .await
        .map_err(|_| "Model management worker failed. Retry the action.".to_string())
        .and_then(|result| result);
    if result.is_err() {
        // A panicking worker must not leave the UI permanently in a busy phase.
        let state = app.state::<EngineState>();
        if state
            .snapshot()
            .is_ok_and(|s| matches!(s.phase.as_str(), "loading" | "downloading"))
        {
            state.publish(
                &app,
                "unavailable",
                None,
                result.as_ref().err().cloned(),
                vec![],
                None,
                None,
            );
        }
    }
    app.state::<EngineState>().release();
    crate::model_storage::publish(&app);
    result?;
    app.state::<EngineState>().snapshot()
}

/// Applies a validated identity without downloading on selection or implicitly changing the active ASR.
fn apply<R: tauri::Runtime>(app: &tauri::AppHandle<R>, action: ModelAction) -> Result<(), String> {
    let state = app.state::<EngineState>();
    match action {
        ModelAction::Select(model) => {
            // Commit first: failed settings writes preserve the current resident weights and choice.
            app.state::<TranscriptionSettings>().save_model(model)?;
            state.reload_destination(app)?;
        }
        ModelAction::Download(id) => {
            let model = manifest::find(&id)?;
            state.publish(app, "downloading", None, None, vec![], None, Some(0.0));
            let transfer = fetcher::download_one(app, model);
            state.refresh_catalog(app)?;
            // Restore readiness using existing weights, or report missing/corrupt selected resources.
            // An inactive model's successful download remains successful when the active one is absent.
            let _ = state.with_sessions(app, |_| Ok(()));
            transfer?;
        }
        ModelAction::Delete(id) => {
            let model = manifest::find(&id)?;
            let base = app.state::<ModelStorage>().prepare()?;
            // Reject unsafe destinations before dropping a currently usable model.
            model_files::artifact(&base, model)?;
            model_files::cache(&base, model)?;
            let selected = state.snapshot()?.selected_model;
            let required = model.descriptor.id == selected.resource().descriptor.id
                || model.descriptor.purpose == manifest::ModelPurpose::Vad;
            if required {
                state.clear_sessions()?;
            }
            let deletion = model_files::delete(&base, model);
            state.refresh_catalog(app)?;
            if required {
                // Never substitute another ASR after deletion; load publishes the missing requirements.
                state.reload_destination(app)?;
            }
            deletion?;
        }
    }
    Ok(())
}

/// Persists a catalog ASR choice and attempts an offline load, allowing missing selections.
#[tauri::command]
pub async fn select_asr_model(
    app: tauri::AppHandle,
    model_id: AsrModelId,
) -> Result<EngineSnapshot, String> {
    perform(app, ModelAction::Select(model_id)).await
}

/// Explicitly downloads or repairs one model and its shared VAD dependency when applicable.
#[tauri::command]
pub async fn download_model(
    app: tauri::AppHandle,
    model_id: String,
) -> Result<EngineSnapshot, String> {
    perform(app, ModelAction::Download(model_id)).await
}

/// Removes a catalog artifact after the frontend's confirmation, preserving the selected identity.
#[tauri::command]
pub async fn delete_model(
    app: tauri::AppHandle,
    model_id: String,
) -> Result<EngineSnapshot, String> {
    perform(app, ModelAction::Delete(model_id)).await
}

#[cfg(test)]
mod tests;
