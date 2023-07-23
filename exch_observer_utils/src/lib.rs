use std::time::{SystemTimeError, UNIX_EPOCH};

/// Returns current system time as a UNIX timestamp
/// or error if there are issues with system clock.
pub fn get_current_timestamp() -> Result<u64, SystemTimeError> {
    let rv = UNIX_EPOCH.elapsed()?.as_secs();
    Ok(rv)
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_get_current_timestamp() {
        let rv = get_current_timestamp();
        assert!(rv.is_ok());
        assert!(rv.unwrap() > 0);
    }
}
