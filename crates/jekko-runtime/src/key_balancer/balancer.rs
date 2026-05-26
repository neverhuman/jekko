/// Decision returned by [`KeyBalancer::pick`].
#[derive(Debug, Clone)]
pub struct KeyPick {
    /// Selected user id.
    pub user_id: String,
    /// Selected env-var name.
    pub env_name: String,
    /// Credential to hand to the adapter.
    pub credential: ProviderCredential,
}

/// Smart balancer over the per-user key pool.
pub struct KeyBalancer {
    pool: KeyPool,
    users_root: PathBuf,
    stores: BTreeMap<String, BalancerStore>,
}

impl KeyBalancer {
    /// Build a balancer rooted at `users_root` (typically `~/.jekko/users/`).
    pub fn with_root(users_root: PathBuf, unlocked: bool) -> Self {
        Self {
            pool: KeyPool::with_root(users_root.clone(), unlocked),
            users_root,
            stores: BTreeMap::new(),
        }
    }

    /// Build a balancer using `JEKKO_HOME` / `HOME` for the users root.
    pub fn new(unlocked: bool) -> Option<Self> {
        let root = jekko_provider::key_pool::users_root()?;
        Some(Self::with_root(root, unlocked))
    }

    /// Replace the pool TTL (tests use `Duration::ZERO` to force rescans).
    pub fn with_pool_ttl(mut self, ttl: std::time::Duration) -> Self {
        self.pool = self.pool.with_ttl(ttl);
        self
    }

    /// Pick the best candidate for `(provider_id, model_id)`. Returns `None`
    /// when there are no candidates with non-zero weight.
    pub fn pick(&mut self, provider_id: &str, model_id: &str) -> Option<KeyPick> {
        let candidates = self.pool.candidates(provider_id);
        if candidates.is_empty() {
            return None;
        }
        let now = unix_now();
        let mut eligible = Vec::with_capacity(candidates.len());
        for (index, cand) in candidates.iter().enumerate() {
            let store = self.store_for(&cand.user_id);
            let usage = stored_usage(store.get(provider_id, model_id));
            let weight = pick_score(provider_id, &usage, now);
            if weight > 0.0 {
                eligible.push(index);
            }
        }
        let index = self.round_robin_index(provider_id, model_id, &eligible)?;
        let pick = candidates[index].clone();
        let store = self.store_for(&pick.user_id);
        let mut usage = stored_usage(store.get(provider_id, model_id));
        usage.attempts = usage.attempts.saturating_add(1);
        let _ = store.upsert(provider_id, model_id, &usage);
        Some(KeyPick {
            user_id: pick.user_id,
            env_name: pick.env_name,
            credential: ProviderCredential::ApiKey { key: pick.key },
        })
    }

    /// Record a successful turn against `(provider, user, model)`.
    pub fn record_success(&mut self, provider_id: &str, user_id: &str, model_id: &str) {
        let store = self.store_for(user_id);
        let mut usage = stored_usage(store.get(provider_id, model_id));
        usage.status = KeyHealth::Ready;
        usage.cooldown_until = None;
        let _ = store.upsert(provider_id, model_id, &usage);
    }

    /// Record a failure against `(provider, user, model)`.
    pub fn record_failure(
        &mut self,
        provider_id: &str,
        user_id: &str,
        model_id: &str,
        kind: FailureKind,
    ) {
        let store = self.store_for(user_id);
        let mut usage = stored_usage(store.get(provider_id, model_id));
        let now = unix_now();
        usage.failures = usage.failures.saturating_add(1);
        usage.last_failure_at = Some(now);
        usage.cooldown_until = Some(now + kind.cooldown_seconds(usage.failures));
        usage.status = kind.health();
        let _ = store.upsert(provider_id, model_id, &usage);
    }

    /// Convenience: classify an HTTP status code and record it.
    pub fn record_http(&mut self, provider_id: &str, user_id: &str, model_id: &str, status: u16) {
        if (200..300).contains(&status) {
            self.record_success(provider_id, user_id, model_id);
        } else {
            self.record_failure(
                provider_id,
                user_id,
                model_id,
                FailureKind::classify_http(status),
            );
        }
    }

    fn store_for(&mut self, user_id: &str) -> &BalancerStore {
        self.stores.entry(user_id.to_string()).or_insert_with(|| {
            let dir = user_dir(&self.users_root, user_id);
            BalancerStore::new(dir.dir.join(STATE_DB_FILENAME))
        })
    }

    fn round_robin_index(
        &self,
        provider_id: &str,
        model_id: &str,
        eligible: &[usize],
    ) -> Option<usize> {
        if eligible.is_empty() {
            return None;
        }
        if eligible.len() == 1 {
            return eligible.first().copied();
        }
        self.persisted_round_robin_index(provider_id, model_id, eligible)
            .or_else(|| eligible.first().copied())
    }

    fn persisted_round_robin_index(
        &self,
        provider_id: &str,
        model_id: &str,
        eligible: &[usize],
    ) -> Option<usize> {
        let db_path = self.users_root.join(".balancer.sqlite");
        let mut conn = Connection::open(db_path).ok()?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS round_robin_cursor (
               provider TEXT NOT NULL,
               model TEXT NOT NULL,
               cursor INTEGER NOT NULL DEFAULT 0,
               PRIMARY KEY (provider, model)
             );",
        )
        .ok()?;
        let tx = conn.transaction().ok()?;
        let cursor: i64 = tx
            .query_row(
                "SELECT cursor FROM round_robin_cursor WHERE provider = ?1 AND model = ?2",
                rusqlite::params![provider_id, model_id],
                |row| row.get(0),
            )
            .optional()
            .ok()
            .flatten()
            .unwrap_or(0);
        let selected = eligible[cursor.rem_euclid(eligible.len() as i64) as usize];
        let next_cursor = cursor.saturating_add(1);
        tx.execute(
            "INSERT INTO round_robin_cursor (provider, model, cursor)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(provider, model) DO UPDATE SET cursor = excluded.cursor",
            rusqlite::params![provider_id, model_id, next_cursor],
        )
        .ok()?;
        tx.commit().ok()?;
        Some(selected)
    }
}

fn pick_score(provider_id: &str, usage: &KeyUsage, now: i64) -> f64 {
    if provider_owns_endpoint_health(provider_id) {
        return score(&KeyUsage::default(), now);
    }
    score(usage, now)
}

fn provider_owns_endpoint_health(provider_id: &str) -> bool {
    provider_id == "jnoccio"
}

#[allow(clippy::manual_unwrap_or_default)]
fn stored_usage(result: Result<KeyUsage>) -> KeyUsage {
    match result {
        Ok(value) => value,
        Err(_) => KeyUsage {
            attempts: 0,
            failures: 0,
            last_failure_at: None,
            cooldown_until: None,
            status: KeyHealth::Ready,
        },
    }
}
