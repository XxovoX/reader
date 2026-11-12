use crate::api::commands::decode_upload_body;
use crate::api::AppState;
use crate::error::error::{ApiResponse, AppError};
use crate::service::legado_reading_time::{installation_device_id, merge_backup};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MergedReadingTimeBackup {
    pub filename: String,
    pub base64: String,
    pub mac_seconds: i64,
    pub device_id: String,
}

pub async fn merge_legado_reading_time_core(
    state: &AppState,
    bytes: Vec<u8>,
) -> Result<ApiResponse<MergedReadingTimeBackup>, AppError> {
    let rows = state
        .reading_stats_service
        .get_all_book_rows("default")
        .await?;
    let root = std::path::PathBuf::from(&state.config.storage_dir);
    let result = tokio::task::spawn_blocking(move || {
        let device_id = installation_device_id(&root)?;
        let output = merge_backup(&bytes, &device_id, &rows)?;
        let mac_seconds = rows
            .iter()
            .fold(0i64, |sum, row| sum.saturating_add(row.seconds));
        Ok::<_, AppError>(MergedReadingTimeBackup {
            filename: format!(
                "backup-mac-{}.zip",
                chrono::Local::now().format("%Y-%m-%d-%H-%M-%S")
            ),
            base64: STANDARD.encode(output),
            mac_seconds,
            device_id,
        })
    })
    .await
    .map_err(|e| AppError::Internal(e.into()))??;
    Ok(ApiResponse::ok(result))
}

#[tauri::command]
pub async fn merge_legado_reading_time(
    state: tauri::State<'_, AppState>,
    request: tauri::ipc::Request<'_>,
) -> Result<ApiResponse<MergedReadingTimeBackup>, AppError> {
    let bytes = decode_upload_body(request.body())?;
    merge_legado_reading_time_core(&state, bytes.into_owned()).await
}

pub async fn inspect_legado_library_core(
    _state: &AppState,
    bytes: Vec<u8>,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    let library = tokio::task::spawn_blocking(move || {
        crate::service::legado_reading_time::inspect_library(&bytes)
    })
    .await
    .map_err(|e| AppError::Internal(e.into()))??;
    Ok(ApiResponse::ok(library))
}

#[tauri::command]
pub async fn inspect_legado_library(
    state: tauri::State<'_, AppState>,
    request: tauri::ipc::Request<'_>,
) -> Result<ApiResponse<serde_json::Value>, AppError> {
    inspect_legado_library_core(&state, decode_upload_body(request.body())?.into_owned()).await
}
