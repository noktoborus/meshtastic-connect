use chrono::{DateTime, Utc};

pub fn format_timediff(
    timestamp: DateTime<Utc>,
    current_datetime: DateTime<Utc>,
) -> Option<String> {
    if timestamp < current_datetime {
        let timediff = current_datetime - timestamp;
        if timediff.num_hours() > 1 {
            Some(format!("{} h", timediff.num_hours()))
        } else if timediff.num_minutes() > 1 {
            Some(format!("{} m", timediff.num_minutes()))
        } else {
            Some(format!("{} s", timediff.num_seconds()))
        }
    } else {
        None
    }
}
