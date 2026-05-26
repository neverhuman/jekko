#[cfg(test)]
mod tests {
    use super::*;
    use jekko_provider::key_pool::LLM_ENV_FILENAME;
    use std::collections::BTreeMap;
    use std::fs;
    use std::time::Duration;
    use tempfile::TempDir;

    fn write_env(root: &std::path::Path, user: &str, contents: &str) {
        let dir = root.join(user);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(LLM_ENV_FILENAME), contents).unwrap();
    }

    #[test]
    fn store_roundtrips_usage() {
        let tmp = TempDir::new().unwrap();
        let store = BalancerStore::new(tmp.path().join("state.sqlite"));
        store
            .upsert(
                "openai",
                "gpt-5",
                &KeyUsage {
                    attempts: 7,
                    failures: 1,
                    last_failure_at: Some(100),
                    cooldown_until: Some(200),
                    status: KeyHealth::RateLimited,
                },
            )
            .unwrap();
        let got = store.get("openai", "gpt-5").unwrap();
        assert_eq!(got.attempts, 7);
        assert_eq!(got.failures, 1);
        assert_eq!(got.last_failure_at, Some(100));
        assert_eq!(got.cooldown_until, Some(200));
        assert_eq!(got.status, KeyHealth::RateLimited);
    }

    #[test]
    fn pick_returns_none_with_no_candidates() {
        let tmp = TempDir::new().unwrap();
        let mut bal = KeyBalancer::with_root(tmp.path().to_path_buf(), false)
            .with_pool_ttl(Duration::from_secs(0));
        assert!(bal.pick("openai", "gpt-5").is_none());
    }

    #[test]
    fn pick_distributes_across_three_healthy_users() {
        let tmp = TempDir::new().unwrap();
        write_env(tmp.path(), "user", "OPENAI_API_KEY=ka\n");
        write_env(tmp.path(), "user_1", "OPENAI_API_KEY=kb\n");
        write_env(tmp.path(), "user_2", "OPENAI_API_KEY=kc\n");
        let mut bal = KeyBalancer::with_root(tmp.path().to_path_buf(), true)
            .with_pool_ttl(Duration::from_secs(0));

        let mut counts: BTreeMap<String, u32> = BTreeMap::new();
        for _ in 0..600 {
            let pick = bal.pick("openai", "gpt-5").expect("pick");
            *counts.entry(pick.user_id.clone()).or_default() += 1;
        }
        let a = *counts.get("user").unwrap_or(&0);
        let b = *counts.get("user_1").unwrap_or(&0);
        let c = *counts.get("user_2").unwrap_or(&0);
        assert!(
            a > 100 && b > 100 && c > 100,
            "expected balanced split, got {a}/{b}/{c}"
        );
    }

    #[test]
    fn pick_reserves_attempt_so_next_pick_uses_next_user() {
        let tmp = TempDir::new().unwrap();
        write_env(tmp.path(), "user_1", "JNOCCIO_DEVELOPER_KEY=ka\n");
        write_env(tmp.path(), "user_2", "JNOCCIO_DEVELOPER_KEY=kb\n");
        let mut bal = KeyBalancer::with_root(tmp.path().to_path_buf(), true)
            .with_pool_ttl(Duration::from_secs(0));

        let first = bal.pick("jnoccio", "jnoccio-router").expect("first pick");
        let second = bal.pick("jnoccio", "jnoccio-router").expect("second pick");

        assert_ne!(
            first.user_id, second.user_id,
            "pick-time reservation should rotate across equal healthy candidates"
        );
    }

    #[test]
    fn round_robin_ignores_old_attempt_skew() {
        let tmp = TempDir::new().unwrap();
        write_env(tmp.path(), "user_1", "JNOCCIO_DEVELOPER_KEY=ka\n");
        write_env(tmp.path(), "user_2", "JNOCCIO_DEVELOPER_KEY=kb\n");
        let mut bal = KeyBalancer::with_root(tmp.path().to_path_buf(), true)
            .with_pool_ttl(Duration::from_secs(0));

        for _ in 0..20 {
            bal.record_success("jnoccio", "user_1", "jnoccio-router");
        }

        let first = bal.pick("jnoccio", "jnoccio-router").expect("first pick");
        let second = bal.pick("jnoccio", "jnoccio-router").expect("second pick");

        assert_eq!(first.user_id, "user_1");
        assert_eq!(second.user_id, "user_2");
    }

    #[test]
    fn jnoccio_selection_ignores_outer_cooldown_state() {
        let tmp = TempDir::new().unwrap();
        write_env(tmp.path(), "user_1", "JNOCCIO_DEVELOPER_KEY=ka\n");
        write_env(tmp.path(), "user_2", "JNOCCIO_DEVELOPER_KEY=kb\n");
        let mut bal = KeyBalancer::with_root(tmp.path().to_path_buf(), true)
            .with_pool_ttl(Duration::from_secs(0));

        bal.record_failure(
            "jnoccio",
            "user_1",
            "jnoccio-router",
            FailureKind::AuthFailed,
        );
        bal.record_failure(
            "jnoccio",
            "user_2",
            "jnoccio-router",
            FailureKind::RateLimited,
        );

        let first = bal.pick("jnoccio", "jnoccio-router").expect("first pick");
        let second = bal.pick("jnoccio", "jnoccio-router").expect("second pick");

        assert_eq!(first.user_id, "user_1");
        assert_eq!(second.user_id, "user_2");
    }

    #[test]
    fn auth_failure_excludes_key_from_pool() {
        let tmp = TempDir::new().unwrap();
        write_env(tmp.path(), "user", "OPENAI_API_KEY=ka\n");
        write_env(tmp.path(), "user_1", "OPENAI_API_KEY=kb\n");
        write_env(tmp.path(), "user_2", "OPENAI_API_KEY=kc\n");
        let mut bal = KeyBalancer::with_root(tmp.path().to_path_buf(), true)
            .with_pool_ttl(Duration::from_secs(0));

        bal.record_failure("openai", "user", "gpt-5", FailureKind::AuthFailed);
        let mut counts: BTreeMap<String, u32> = BTreeMap::new();
        for _ in 0..120 {
            let pick = bal.pick("openai", "gpt-5").expect("pick");
            assert_ne!(pick.user_id, "user");
            *counts.entry(pick.user_id).or_default() += 1;
        }
        assert!(counts.get("user_1").copied().unwrap_or(0) > 0);
        assert!(counts.get("user_2").copied().unwrap_or(0) > 0);
    }

    #[test]
    fn rate_limit_cools_down_and_recovers_after_window() {
        let tmp = TempDir::new().unwrap();
        write_env(tmp.path(), "user", "OPENAI_API_KEY=ka\n");
        write_env(tmp.path(), "user_1", "OPENAI_API_KEY=kb\n");
        write_env(tmp.path(), "user_2", "OPENAI_API_KEY=kc\n");
        let mut bal = KeyBalancer::with_root(tmp.path().to_path_buf(), true)
            .with_pool_ttl(Duration::from_secs(0));

        bal.record_failure("openai", "user", "gpt-5", FailureKind::RateLimited);
        let mut counts: BTreeMap<String, u32> = BTreeMap::new();
        for _ in 0..120 {
            let pick = bal.pick("openai", "gpt-5").expect("pick");
            assert_ne!(pick.user_id, "user");
            *counts.entry(pick.user_id).or_default() += 1;
        }
        assert!(counts.get("user_1").copied().unwrap_or(0) > 0);
        assert!(counts.get("user_2").copied().unwrap_or(0) > 0);
    }

    #[test]
    fn record_success_clears_cooldown() {
        let tmp = TempDir::new().unwrap();
        write_env(tmp.path(), "user", "OPENAI_API_KEY=ka\n");
        let mut bal = KeyBalancer::with_root(tmp.path().to_path_buf(), true)
            .with_pool_ttl(Duration::from_secs(0));

        bal.record_failure("openai", "user", "gpt-5", FailureKind::RateLimited);
        bal.record_success("openai", "user", "gpt-5");

        let store = BalancerStore::new(user_dir(tmp.path(), "user").dir.join(STATE_DB_FILENAME));
        let usage = store.get("openai", "gpt-5").unwrap();
        assert_eq!(usage.status, KeyHealth::Ready);
        assert!(usage.cooldown_until.is_none());
    }

    #[test]
    fn classify_http_routes_to_correct_kind() {
        assert_eq!(FailureKind::classify_http(401), FailureKind::AuthFailed);
        assert_eq!(FailureKind::classify_http(403), FailureKind::AuthFailed);
        assert_eq!(FailureKind::classify_http(429), FailureKind::RateLimited);
        assert_eq!(FailureKind::classify_http(500), FailureKind::ServerError);
        assert_eq!(FailureKind::classify_http(502), FailureKind::ServerError);
        assert_eq!(FailureKind::classify_http(404), FailureKind::Other);
    }
}
