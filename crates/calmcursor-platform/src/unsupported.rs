//! Fallback for operating systems without a backend.

use crate::{PermissionStatus, PlatformError, Shared};
use std::sync::Arc;

pub const NAME: &str = "unsupported";
pub const PERMISSION_URL: Option<&str> = None;

#[derive(Debug)]
pub struct Backend;

impl Backend {
    pub fn start(_shared: Arc<Shared>) -> Result<Self, PlatformError> {
        Err(PlatformError::Unsupported)
    }

    pub fn stop(self) {}
}

pub fn permission_status() -> PermissionStatus {
    PermissionStatus::Unknown
}

pub fn request_permission() -> bool {
    false
}
