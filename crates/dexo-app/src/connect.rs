//! The one way a saved profile is dialled -- by the TUI, the CLI and the MCP server --
//! so none of them can skip the profile's pre-connect command.

use std::time::Duration;

use dexo_driver_api::{ConnectionFactory, ConnectionSecrets, DriverError, Session};

use crate::connection_policy::ConnectionPolicy;
use crate::connection_profile::ConnectionProfile;
use crate::error::{AppError, ErrorCategory};
use crate::query_service::map_driver_error;

/// A dialled profile: the session, kept together with the pre-connect command that
/// opened its way; the profile as dialled (the tunnel's host and port, `pre_connect`
/// taken out), for the side connections that should ride the same tunnel; its policy.
pub struct Opened {
    pub session: Box<dyn Session>,
    pub profile: ConnectionProfile,
    pub policy: ConnectionPolicy,
}

/// Why a profile could not be dialled.
#[derive(Debug)]
pub enum ConnectError {
    /// The profile's pre-connect command did not open the way; the message quotes the
    /// command and what it printed.
    PreConnect(AppError),
    /// The profile kept it from being dialled.
    Setup(AppError),
    /// The driver turned it down, as it said.
    Driver(DriverError),
    /// The driver did not answer within the time given.
    TimedOut(Duration),
}

impl From<ConnectError> for AppError {
    fn from(error: ConnectError) -> Self {
        match error {
            ConnectError::PreConnect(error) | ConnectError::Setup(error) => error,
            ConnectError::Driver(error) => map_driver_error(error),
            ConnectError::TimedOut(limit) => AppError::new(
                ErrorCategory::Timeout,
                format!("the database did not answer within {}s", limit.as_secs()),
            ),
        }
    }
}

/// Runs the profile's pre-connect command, then connects through the way it opened,
/// waiting `connect_timeout` at most for the driver (the command has its own limit).
/// A connect that fails stops the command.
pub async fn open(
    factory: &dyn ConnectionFactory,
    profile: &ConnectionProfile,
    secrets: impl Into<ConnectionSecrets>,
    connect_timeout: Option<Duration>,
) -> Result<Opened, ConnectError> {
    let (profile, process) = crate::pre_connect::prepare(profile, crate::pre_connect::TIMEOUT)
        .await
        .map_err(ConnectError::PreConnect)?;
    let (request, policy) = profile
        .connect_request(secrets)
        .map_err(ConnectError::Setup)?;
    let connecting = factory.connect(request);
    let session = match connect_timeout {
        Some(limit) => tokio::time::timeout(limit, connecting)
            .await
            .map_err(|_| ConnectError::TimedOut(limit))?,
        None => connecting.await,
    }
    .map_err(ConnectError::Driver)?;
    Ok(Opened {
        session: crate::pre_connect::attach(session, process),
        profile,
        policy,
    })
}

#[cfg(all(test, unix))]
mod tests {
    use std::time::Duration;

    use dexo_driver_api::{
        ConnectRequest, ConnectionFactory, DriverDescriptor, DriverError, DriverErrorCategory,
        Session,
    };
    use dexo_test_support::FakeSession;
    use secrecy::SecretString;

    use super::{ConnectError, open};
    use crate::connection_profile::{ConnectionId, ConnectionProfile, SecretRef};

    struct Factory(bool);

    #[async_trait::async_trait]
    impl ConnectionFactory for Factory {
        fn descriptor(&self) -> DriverDescriptor {
            DriverDescriptor::postgres()
        }

        async fn connect(&self, _request: ConnectRequest) -> Result<Box<dyn Session>, DriverError> {
            if self.0 {
                Ok(Box::new(FakeSession::default()))
            } else {
                Err(DriverError::new(
                    DriverErrorCategory::Authentication,
                    "password authentication failed",
                ))
            }
        }
    }

    fn tunnelled(marker: &str) -> ConnectionProfile {
        let command = format!(
            "python3 -c 'import socket,sys,time; s=socket.socket(); \
             s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1); \
             s.bind((\"127.0.0.1\", int(sys.argv[1]))); s.listen(); time.sleep(60)' ${{port}} {marker}"
        );
        ConnectionProfile::new(
            ConnectionId(uuid::Uuid::nil()),
            None,
            "tunnelled",
            "postgres",
            "local",
            serde_json::json!({
                "host": "db.internal", "port": 5432, "username": "u", "pre_connect": command
            }),
            SecretRef::new("ref".into()),
        )
    }

    fn running(marker: &str) -> bool {
        std::process::Command::new("pgrep")
            .args(["-f", marker])
            .output()
            .is_ok_and(|output| !output.stdout.is_empty())
    }

    /// Whether the command is gone, given a loaded machine a few seconds to reap it.
    fn gone(marker: &str) -> bool {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while running(marker) {
            if std::time::Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        true
    }

    /// A connect the driver turns down stops the command it started, and keeps the
    /// driver's error for the caller to read.
    #[tokio::test]
    async fn a_failed_connect_stops_its_command() {
        let marker = "dexo-connect-refused-9b3c";
        let error = open(
            &Factory(false),
            &tunnelled(marker),
            SecretString::from("pw"),
            Some(Duration::from_secs(5)),
        )
        .await
        .err()
        .unwrap();
        assert!(
            matches!(&error, ConnectError::Driver(error) if error.category() == DriverErrorCategory::Authentication)
        );
        assert!(gone(marker), "the command outlived the failed connect");
    }

    /// Through the tunnel's wrapper, the session keeps every facet it has.
    #[tokio::test]
    async fn the_wrapped_session_keeps_its_facets() {
        let opened = open(
            &Factory(true),
            &tunnelled("dexo-connect-facets-4e1d"),
            SecretString::from("pw"),
            None,
        )
        .await
        .unwrap();
        let session = &opened.session;
        assert!(session.catalog().is_some());
        assert!(session.transactions().is_some());
        assert!(session.data().is_some());
        assert!(session.ddl().is_some());
        assert_eq!(opened.profile.config["host"], "127.0.0.1");
        assert!(opened.profile.config.get("pre_connect").is_none());
    }
}
