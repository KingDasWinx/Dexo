use std::env;

use testcontainers_modules::mysql::Mysql;
use testcontainers_modules::postgres::Postgres;
use testcontainers_modules::testcontainers::ContainerAsync;
use testcontainers_modules::testcontainers::core::ImageExt;
use testcontainers_modules::testcontainers::runners::AsyncRunner;

pub const POSTGRES_IMAGE_TAG: &str = "16.9-alpine";
pub const MYSQL_IMAGE_TAG: &str = "8.4.5";
pub const TEST_USER: &str = "dexo";
pub const TEST_PASSWORD: &str = "dexo_test_only";
pub const TEST_DATABASE: &str = "dexo";

pub struct DatabasePair {
    _postgres: Option<ContainerAsync<Postgres>>,
    _mysql: Option<ContainerAsync<Mysql>>,
    postgres_url: String,
    mysql_url: String,
    postgres_endpoint: String,
    mysql_endpoint: String,
}

fn postgres_tag() -> String {
    env::var("DEXO_POSTGRES_TAG").unwrap_or_else(|_| match env::var("DEXO_IT_IMAGE") {
        Ok(image) if image.starts_with("postgres:") => image
            .split_once(':')
            .map(|(_, tag)| tag.to_string())
            .unwrap_or_else(|| POSTGRES_IMAGE_TAG.into()),
        _ => POSTGRES_IMAGE_TAG.into(),
    })
}

fn mysql_tag() -> String {
    env::var("DEXO_MYSQL_TAG").unwrap_or_else(|_| match env::var("DEXO_IT_IMAGE") {
        Ok(image) if image.starts_with("mysql:") => image
            .split_once(':')
            .map(|(_, tag)| tag.to_string())
            .unwrap_or_else(|| MYSQL_IMAGE_TAG.into()),
        _ => MYSQL_IMAGE_TAG.into(),
    })
}

fn skip_postgres() -> bool {
    env::var("DEXO_IT_IMAGE")
        .ok()
        .is_some_and(|image| image.starts_with("mysql:"))
}

fn skip_mysql() -> bool {
    env::var("DEXO_IT_IMAGE")
        .ok()
        .is_some_and(|image| image.starts_with("postgres:"))
}

impl DatabasePair {
    pub async fn start() -> anyhow::Result<Self> {
        let postgres = if skip_postgres() {
            None
        } else {
            Some(
                Postgres::default()
                    .with_user(TEST_USER)
                    .with_password(TEST_PASSWORD)
                    .with_db_name(TEST_DATABASE)
                    .with_tag(postgres_tag())
                    .start()
                    .await?,
            )
        };
        let mysql = if skip_mysql() {
            None
        } else {
            Some(
                Mysql::default()
                    .with_tag(mysql_tag())
                    .with_env_var("MYSQL_USER", TEST_USER)
                    .with_env_var("MYSQL_PASSWORD", TEST_PASSWORD)
                    .with_env_var("MYSQL_DATABASE", TEST_DATABASE)
                    .with_env_var("MYSQL_ROOT_PASSWORD", TEST_PASSWORD)
                    .start()
                    .await?,
            )
        };
        let (postgres_url, postgres_endpoint) = if let Some(postgres) = postgres.as_ref() {
            let host = postgres.get_host().await?;
            let port = postgres.get_host_port_ipv4(5432).await?;
            (
                format_url("postgres", &host, port),
                format!("{host}:{port}"),
            )
        } else {
            (String::new(), String::new())
        };
        let (mysql_url, mysql_endpoint) = if let Some(mysql) = mysql.as_ref() {
            let host = mysql.get_host().await?;
            let port = mysql.get_host_port_ipv4(3306).await?;
            (format_url("mysql", &host, port), format!("{host}:{port}"))
        } else {
            (String::new(), String::new())
        };
        Ok(Self {
            _postgres: postgres,
            _mysql: mysql,
            postgres_url,
            mysql_url,
            postgres_endpoint,
            mysql_endpoint,
        })
    }

    pub fn postgres_url(&self) -> &str {
        &self.postgres_url
    }

    pub fn mysql_url(&self) -> &str {
        &self.mysql_url
    }

    pub fn postgres_endpoint(&self) -> &str {
        &self.postgres_endpoint
    }

    pub fn mysql_endpoint(&self) -> &str {
        &self.mysql_endpoint
    }
}

fn format_url(scheme: &str, host: impl std::fmt::Display, port: u16) -> String {
    format!("{scheme}://{TEST_USER}:{TEST_PASSWORD}@{host}:{port}/{TEST_DATABASE}")
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    #[ignore = "requires Docker"]
    async fn databases_are_reachable() {
        let pair = super::DatabasePair::start().await.unwrap();
        assert!(pair.postgres_url().starts_with("postgres://"));
        assert!(pair.mysql_url().starts_with("mysql://"));
    }
}
