fn score(usage: &KeyUsage, now: i64) -> f64 {
    if usage
        .cooldown_until
        .map(|until| until > now)
        .unwrap_or(false)
    {
        return 0.0;
    }
    let health = usage.status.weight();
    if health == 0.0 {
        return 0.0;
    }
    let load = 1.0 / (1.0 + usage.attempts as f64 / 20.0);
    let penalty = if usage.failures == 0 {
        1.0
    } else {
        (1.0 / (1.0 + usage.failures as f64 * 6.0)).clamp(0.01, 1.0)
    };
    (health * load * penalty).max(0.0001)
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
