use crate::error::AppResult;
use crate::storage::types::AppSettings;

/// Retrieve the current application settings.
pub fn get_settings() -> AppResult<AppSettings> {
    Ok(AppSettings::default())
}

/// Persist updated application settings.
pub fn update_settings(_settings: AppSettings) -> AppResult<()> {
    Ok(())
}
