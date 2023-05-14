use std::{
    time::{UNIX_EPOCH, SystemTimeError}
};

pub fn get_current_timestamp() -> Result<u64, SystemTimeError> {
    let rv = UNIX_EPOCH.elapsed()?.as_secs();
    Ok(rv)
}
