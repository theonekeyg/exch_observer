use std::{
    time::{UNIX_EPOCH, SystemTimeError}
};

/// Returns current system time as a UNIX timestamp
/// or error if there are issues with system clock.
pub fn get_current_timestamp() -> Result<u64, SystemTimeError> {
    let rv = UNIX_EPOCH.elapsed()?.as_secs();
    Ok(rv)
}
