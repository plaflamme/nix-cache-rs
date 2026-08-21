use std::time::{SystemTime as StdSystemTime, UNIX_EPOCH as STD_UNIX_EPOCH};
use web_time::{SystemTime as WebSystemTime, UNIX_EPOCH as WEB_UNIX_EPOCH};

pub fn now() -> StdSystemTime {
    // 1. Get the current web-safe time
    let web_now = WebSystemTime::now();

    // 2. Safely get the duration since the Unix Epoch
    let duration_since_epoch = web_now
        .duration_since(WEB_UNIX_EPOCH)
        .unwrap_or_else(|_| web_time::Duration::from_secs(0));

    // 3. Reconstruct a genuine std::time::SystemTime using the standard Epoch anchor
    STD_UNIX_EPOCH + duration_since_epoch
}
