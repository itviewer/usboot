use std::time::Duration;

pub fn parse_timeout(s: &str) -> Result<Duration, String> {
    s.parse::<f64>()
        .map(Duration::from_secs_f64)
        .map_err(|_| format!("'{}' is not a valid timeout value", s))
}