use crossterm::event::{Event, EventStream, KeyEventKind};
use dexo_app::{DriverRegistry, SecretPersist, create_connection, map_driver_error};
use dexo_secrets::{KeyringSecretStore, MemorySecretStore, SecretError, SecretStore};
use dexo_storage::{AppPaths, ConnectionRepository, Database};
use futures_util::StreamExt;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use secrecy::SecretString;

use crate::action::{Action, Effect};
use crate::model::Model;
use crate::terminal::{CrosstermTerminal, TerminalGuard, TuiError};

pub fn action_from_event(event: Event) -> Option<Action> {
    match event {
        Event::Key(key) if key.kind == KeyEventKind::Press => Some(Action::Key(key)),
        Event::Mouse(mouse) => Some(Action::Mouse(mouse)),
        Event::Resize(width, height) => Some(Action::Resize { width, height }),
        _ => None,
    }
}

pub fn run(registry: DriverRegistry) -> Result<(), TuiError> {
    crate::terminal::install_panic_hook();
    tokio::runtime::Runtime::new()?.block_on(run_async(registry))
}

async fn run_async(registry: DriverRegistry) -> Result<(), TuiError> {
    let mut guard = TerminalGuard::start(CrosstermTerminal)?;
    let result = run_loop(registry).await;
    guard.restore();
    result
}

async fn run_loop(registry: DriverRegistry) -> Result<(), TuiError> {
    let mut terminal = Terminal::new(CrosstermBackend::new(std::io::stdout()))?;
    let mut model = Model::default();
    let mut events = EventStream::new();
    let secrets = SessionSecrets::default();
    loop {
        terminal.draw(|frame| crate::render::render(frame, &model))?;
        let Some(event) = events.next().await else {
            break;
        };
        let Some(action) = action_from_event(event?) else {
            continue;
        };
        let effects = crate::update::update(&mut model, action);
        for effect in effects {
            if matches!(effect, Effect::Quit) {
                return Ok(());
            }
            if let Some(follow_up) = apply_effect(&registry, &secrets, effect).await {
                let _ = crate::update::update(&mut model, follow_up);
            }
        }
    }
    Ok(())
}

async fn apply_effect(
    registry: &DriverRegistry,
    secrets: &SessionSecrets,
    effect: Effect,
) -> Option<Action> {
    match effect {
        Effect::CreateConnection { input, password } => {
            match save_and_connect(registry, secrets, input, &password).await {
                Ok(action) => Some(action),
                Err(message) => Some(Action::ConnectionFormError { message }),
            }
        }
        // ponytail: query/tx still need a live session handle; connect only flips status for now
        _ => None,
    }
}

async fn save_and_connect(
    registry: &DriverRegistry,
    secrets: &SessionSecrets,
    input: dexo_app::NewConnection,
    password: &str,
) -> Result<Action, String> {
    let paths = AppPaths::discover().map_err(|error| error.to_string())?;
    let db = Database::open(&paths.database).map_err(|error| error.to_string())?;
    let repo = ConnectionRepository::new(db.connection());
    let (profile, persist) =
        create_connection(input, password, secrets, &repo).map_err(|error| error.to_string())?;
    if persist == SecretPersist::SessionOnly {
        secrets
            .memory
            .put(profile.secret_ref.as_str(), password)
            .map_err(|error| error.to_string())?;
    }
    connect_session(registry, secrets, &profile).await?;
    Ok(Action::ConnectionChanged {
        name: profile.name,
        ready: true,
        environment: profile.environment,
    })
}

async fn connect_session(
    registry: &DriverRegistry,
    secrets: &SessionSecrets,
    profile: &dexo_app::ConnectionProfile,
) -> Result<(), String> {
    let secret = secrets
        .get(profile.secret_ref.as_str())
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "secret is missing for this connection".to_string())?;
    let factory = registry
        .get(&profile.driver)
        .map_err(|error| error.to_string())?;
    let (connect, _) = profile
        .connect_request(secret)
        .map_err(|error| error.to_string())?;
    factory
        .connect(connect)
        .await
        .map_err(map_driver_error)
        .map_err(|error| error.to_string())?;
    Ok(())
}

#[derive(Default)]
struct SessionSecrets {
    keyring: KeyringSecretStore,
    memory: MemorySecretStore,
}

impl SecretStore for SessionSecrets {
    fn put(&self, key: &str, value: &str) -> Result<(), SecretError> {
        match self.keyring.put(key, value) {
            Ok(()) => {
                let _ = self.memory.put(key, value);
                Ok(())
            }
            Err(SecretError::Unavailable) => self.memory.put(key, value),
            Err(error) => Err(error),
        }
    }

    fn get(&self, key: &str) -> Result<Option<SecretString>, SecretError> {
        if let Ok(Some(secret)) = self.memory.get(key) {
            return Ok(Some(secret));
        }
        self.keyring.get(key)
    }

    fn delete(&self, key: &str) -> Result<(), SecretError> {
        let _ = self.memory.delete(key);
        self.keyring.delete(key)
    }
}
