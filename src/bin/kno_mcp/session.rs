use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use rmcp::model::Implementation;

use crate::runner::KnoRunner;

const SESSION_NICKNAME: &str = "mcp-session";

#[derive(Debug, Clone, Default)]
pub struct LeaseRegistry {
    inner: Arc<Mutex<LeaseState>>,
}

#[derive(Debug, Default)]
struct LeaseState {
    leases: HashMap<String, String>,
    current: Option<String>,
}

impl LeaseRegistry {
    pub fn get_or_create(
        &self,
        runner: &KnoRunner,
        client: &Implementation,
        timeout_seconds: u64,
    ) -> Result<String, String> {
        let key = client_key(client);
        let mut state = self.inner.lock().expect("lease map poisoned");
        if let Some(id) = state.leases.get(&key).cloned() {
            state.current = Some(id.clone());
            return Ok(id);
        }
        drop(state);

        let lease = create_session_lease(runner, client, timeout_seconds)?;
        let mut state = self.inner.lock().expect("lease map poisoned");
        state.leases.insert(key, lease.clone());
        state.current = Some(lease.clone());
        Ok(lease)
    }

    pub fn current(&self) -> Option<String> {
        self.inner
            .lock()
            .expect("lease map poisoned")
            .current
            .clone()
    }
}

fn client_key(client: &Implementation) -> String {
    format!(
        "{}\0{}\0{}",
        client.name,
        client.version,
        client.title.as_deref().unwrap_or_default()
    )
}

fn create_session_lease(
    runner: &KnoRunner,
    client: &Implementation,
    timeout_seconds: u64,
) -> Result<String, String> {
    let mut args = vec![
        "create".to_string(),
        "--nickname".to_string(),
        SESSION_NICKNAME.to_string(),
        "--type".to_string(),
        "agent".to_string(),
        "--agent-type".to_string(),
        "api".to_string(),
        "--agent-name".to_string(),
        client.name.clone(),
        "--model".to_string(),
        client.name.clone(),
        "--model-version".to_string(),
        client.version.clone(),
        "--timeout-seconds".to_string(),
        timeout_seconds.to_string(),
    ];
    if let Some(provider) = client.title.as_ref().filter(|title| !title.is_empty()) {
        args.push("--provider".to_string());
        args.push(provider.clone());
    }
    runner
        .run("lease", &args)
        .map_err(|err| err.stderr)
        .and_then(|value| {
            value
                .get("id")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
                .ok_or_else(|| "lease create output did not include id".to_string())
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn current_is_empty_before_initialize() {
        assert_eq!(LeaseRegistry::default().current(), None);
    }

    #[test]
    fn creates_and_caches_session_lease() {
        let registry = LeaseRegistry::default();
        let runner = KnoRunner::new(fixture_kno(), PathBuf::from("/example/repo"));
        let mut client = Implementation::new("test-client", "1.2.3");
        client.title = Some("test-provider".to_string());

        let first = registry
            .get_or_create(&runner, &client, 30)
            .expect("lease should be created");
        let second = registry
            .get_or_create(&runner, &client, 30)
            .expect("cached lease should be returned");

        assert_eq!(first, "L1");
        assert_eq!(second, "L1");
        assert_eq!(registry.current(), Some("L1".to_string()));
    }

    #[test]
    fn current_tracks_latest_initialized_client() {
        let registry = LeaseRegistry::default();
        let runner = KnoRunner::new(fixture_kno(), PathBuf::from("/example/repo"));
        let first = Implementation::new("test-client", "1.2.3");
        let second = Implementation::new("other-client", "1.2.3");

        assert_eq!(registry.get_or_create(&runner, &first, 30), Ok("L1".into()));
        assert_eq!(
            registry.get_or_create(&runner, &second, 30),
            Ok("L2".into())
        );
        assert_eq!(registry.current(), Some("L2".to_string()));
        assert_eq!(registry.get_or_create(&runner, &first, 30), Ok("L1".into()));
        assert_eq!(registry.current(), Some("L1".to_string()));
    }

    fn fixture_kno() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/kno-stub.sh")
    }
}
