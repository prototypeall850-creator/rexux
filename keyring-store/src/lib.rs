use rexux_utils_home_dir::find_rexux_home;
use std::error::Error;
use std::fmt;
use std::fmt::Debug;
use std::fs;
use std::io;
use std::path::PathBuf;
use tracing::trace;

/// Error type mirroring the subset of keyring failures consumers rely on.
#[derive(Debug, Clone)]
pub enum KeyringError {
    NoEntry,
    Invalid(String, String),
    PlatformFailure(String),
}

impl fmt::Display for KeyringError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoEntry => write!(f, "no matching entry found in secure storage"),
            Self::Invalid(service, operation) => {
                write!(f, "invalid credential parameters for {operation} in {service}")
            }
            Self::PlatformFailure(message) => {
                write!(f, "platform secure storage failure: {message}")
            }
        }
    }
}

impl Error for KeyringError {}

#[derive(Debug)]
pub enum CredentialStoreError {
    Other(KeyringError),
}

impl CredentialStoreError {
    pub fn new(error: KeyringError) -> Self {
        Self::Other(error)
    }

    pub fn message(&self) -> String {
        match self {
            Self::Other(error) => error.to_string(),
        }
    }

    pub fn into_error(self) -> KeyringError {
        match self {
            Self::Other(error) => error,
        }
    }
}

impl fmt::Display for CredentialStoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Other(error) => write!(f, "{error}"),
        }
    }
}

impl Error for CredentialStoreError {}

/// Shared credential store abstraction for keyring-backed implementations.
///
/// The default implementation persists credentials as 0600 JSON files under
/// `$REXUX_HOME/keyring-store/` so no OS keyring service is required. This
/// keeps BYOK deployments (e.g. Termux/Android) free of secret-service/dbus
/// dependencies.
pub trait KeyringStore: Debug + Send + Sync {
    fn load(&self, service: &str, account: &str) -> Result<Option<String>, CredentialStoreError>;
    fn save(&self, service: &str, account: &str, value: &str) -> Result<(), CredentialStoreError>;
    fn delete(&self, service: &str, account: &str) -> Result<bool, CredentialStoreError>;
}

fn credential_path(service: &str, account: &str) -> Result<PathBuf, CredentialStoreError> {
    let rexux_home = find_rexux_home()
        .map_err(|err| CredentialStoreError::new(KeyringError::PlatformFailure(err.to_string())))?;
    Ok(rexux_home
        .into_path_buf()
        .join("keyring-store")
        .join(encode_component(service))
        .join(format!("{}.json", encode_component(account))))
}

/// Percent-encode anything outside `[A-Za-z0-9._-]` so the value is a safe
/// single path component.
fn encode_component(component: &str) -> String {
    let mut encoded = String::with_capacity(component.len());
    for byte in component.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'.' | b'_' | b'-' => {
                encoded.push(byte as char);
            }
            _ => {
                encoded.push_str(&format!("%{byte:02X}"));
            }
        }
    }
    if encoded.is_empty() {
        encoded.push_str("%00");
    }
    encoded
}

fn write_secret_file(path: &PathBuf, value: &str) -> Result<(), CredentialStoreError> {
    let parent = path.parent().ok_or_else(|| {
        CredentialStoreError::new(KeyringError::PlatformFailure(
            "credential path has no parent directory".to_string(),
        ))
    })?;
    fs::create_dir_all(parent).map_err(|err| {
        CredentialStoreError::new(KeyringError::PlatformFailure(format!(
            "failed to create credential directory {}: {err}",
            parent.display()
        )))
    })?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(parent, fs::Permissions::from_mode(0o700)).map_err(|err| {
            CredentialStoreError::new(KeyringError::PlatformFailure(err.to_string()))
        })?;
    }

    let payload = serde_json::json!({ "value": value }).to_string();
    let temp_name = path
        .file_name()
        .map(|name| format!(".{}.tmp", name.to_string_lossy()))
        .unwrap_or_else(|| ".credential.tmp".to_string());
    let temp_path = parent.join(temp_name);
    fs::write(&temp_path, payload)
        .map_err(|err| CredentialStoreError::new(KeyringError::PlatformFailure(err.to_string())))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&temp_path, fs::Permissions::from_mode(0o600)).map_err(|err| {
            let _ = fs::remove_file(&temp_path);
            CredentialStoreError::new(KeyringError::PlatformFailure(err.to_string()))
        })?;
    }

    fs::rename(&temp_path, path).map_err(|err| {
        let _ = fs::remove_file(&temp_path);
        CredentialStoreError::new(KeyringError::PlatformFailure(err.to_string()))
    })?;

    Ok(())
}

fn read_secret_file(path: &PathBuf) -> Result<Option<String>, CredentialStoreError> {
    let payload = match fs::read_to_string(path) {
        Ok(payload) => payload,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(err) => {
            return Err(CredentialStoreError::new(KeyringError::PlatformFailure(
                err.to_string(),
            )))
        }
    };
    let parsed: serde_json::Value = serde_json::from_str(&payload).map_err(|err| {
        CredentialStoreError::new(KeyringError::PlatformFailure(format!(
            "corrupt credential file {}: {err}",
            path.display()
        )))
    })?;
    let value = parsed
        .get("value")
        .and_then(|value| value.as_str())
        .ok_or_else(|| {
            CredentialStoreError::new(KeyringError::PlatformFailure(format!(
                "credential file {} is missing its value field",
                path.display()
            )))
        })?;
    Ok(Some(value.to_string()))
}

#[derive(Debug, Clone, Copy)]
pub struct DefaultKeyringStore;

impl KeyringStore for DefaultKeyringStore {
    fn load(&self, service: &str, account: &str) -> Result<Option<String>, CredentialStoreError> {
        trace!("file-keyring.load start, service={service}, account={account}");
        let path = credential_path(service, account)?;
        let value = read_secret_file(&path);
        match &value {
            Ok(Some(_)) => {
                trace!("file-keyring.load success, service={service}, account={account}");
            }
            Ok(None) => {
                trace!("file-keyring.load no entry, service={service}, account={account}");
            }
            Err(err) => {
                trace!("file-keyring.load error, service={service}, account={account}, error={err}");
            }
        }
        value
    }

    fn save(&self, service: &str, account: &str, value: &str) -> Result<(), CredentialStoreError> {
        trace!(
            "file-keyring.save start, service={service}, account={account}, value_len={}",
            value.len()
        );
        let path = credential_path(service, account)?;
        let result = write_secret_file(&path, value);
        match &result {
            Ok(()) => {
                trace!("file-keyring.save success, service={service}, account={account}");
            }
            Err(err) => {
                trace!("file-keyring.save error, service={service}, account={account}, error={err}");
            }
        }
        result
    }

    fn delete(&self, service: &str, account: &str) -> Result<bool, CredentialStoreError> {
        trace!("file-keyring.delete start, service={service}, account={account}");
        let path = credential_path(service, account)?;
        match fs::remove_file(&path) {
            Ok(()) => {
                trace!("file-keyring.delete success, service={service}, account={account}");
                Ok(true)
            }
            Err(err) if err.kind() == io::ErrorKind::NotFound => {
                trace!("file-keyring.delete no entry, service={service}, account={account}");
                Ok(false)
            }
            Err(err) => {
                trace!("file-keyring.delete error, service={service}, account={account}, error={err}");
                Err(CredentialStoreError::new(KeyringError::PlatformFailure(
                    err.to_string(),
                )))
            }
        }
    }
}

pub mod tests {
    use super::CredentialStoreError;
    use super::KeyringError;
    use super::KeyringStore;
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::sync::Mutex;
    use std::sync::PoisonError;

    #[derive(Default, Clone, Debug)]
    pub struct MockKeyringStore {
        credentials: Arc<Mutex<HashMap<String, Result<String, KeyringError>>>>,
    }

    impl MockKeyringStore {
        pub fn credential(&self, account: &str) -> MockCredentialGuard {
            MockCredentialGuard {
                store: self.clone(),
                account: account.to_string(),
            }
        }

        pub fn saved_value(&self, account: &str) -> Option<String> {
            let guard = self
                .credentials
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            guard.get(account).and_then(|result| result.clone().ok())
        }

        pub fn set_error(&self, account: &str, error: KeyringError) {
            let mut guard = self
                .credentials
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            guard.insert(account.to_string(), Err(error));
        }

        pub fn contains(&self, account: &str) -> bool {
            let guard = self
                .credentials
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            guard.contains_key(account)
        }
    }

    /// Placeholder mirroring the previous mock API surface.
    #[derive(Debug)]
    pub struct MockCredentialGuard {
        store: MockKeyringStore,
        account: String,
    }

    impl MockCredentialGuard {
        pub fn get_password(&self) -> Result<String, KeyringError> {
            let guard = self
                .store
                .credentials
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            match guard.get(&self.account) {
                Some(Ok(value)) => Ok(value.clone()),
                Some(Err(error)) => Err(error.clone()),
                None => Err(KeyringError::NoEntry),
            }
        }

        pub fn set_password(&self, value: &str) -> Result<(), KeyringError> {
            let mut guard = self
                .store
                .credentials
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            guard.insert(self.account.clone(), Ok(value.to_string()));
            Ok(())
        }

        pub fn delete_credential(&self) -> Result<(), KeyringError> {
            let mut guard = self
                .store
                .credentials
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            match guard.remove(&self.account) {
                Some(Ok(_)) | None => Ok(()),
                Some(Err(error)) => Err(error),
            }
        }
    }

    impl KeyringStore for MockKeyringStore {
        fn load(
            &self,
            _service: &str,
            account: &str,
        ) -> Result<Option<String>, CredentialStoreError> {
            let guard = self
                .credentials
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            match guard.get(account) {
                Some(Ok(value)) => Ok(Some(value.clone())),
                Some(Err(KeyringError::NoEntry)) | None => Ok(None),
                Some(Err(error)) => Err(CredentialStoreError::new(error.clone())),
            }
        }

        fn save(
            &self,
            _service: &str,
            account: &str,
            value: &str,
        ) -> Result<(), CredentialStoreError> {
            let mut guard = self
                .credentials
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            guard.insert(account.to_string(), Ok(value.to_string()));
            Ok(())
        }

        fn delete(&self, _service: &str, account: &str) -> Result<bool, CredentialStoreError> {
            let mut guard = self
                .credentials
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            match guard.remove(account) {
                Some(Ok(_)) => Ok(true),
                Some(Err(KeyringError::NoEntry)) | None => Ok(false),
                Some(Err(error)) => Err(CredentialStoreError::new(error)),
            }
        }
    }
}
