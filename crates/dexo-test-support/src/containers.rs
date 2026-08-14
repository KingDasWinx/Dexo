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
    _postgres: ContainerAsync<Postgres>,
    _mysql: ContainerAsync<Mysql>,
    postgres_url: String,
    mysql_url: String,
    postgres_endpoint: String,
    mysql_endpoint: String,
}

impl DatabasePair {
    pub async fn start() -> anyhow::Result<Self> {
        let postgres = Postgres::default()
            .with_user(TEST_USER)
            .with_password(TEST_PASSWORD)
            .with_db_name(TEST_DATABASE)
            .with_tag(POSTGRES_IMAGE_TAG)
            .start()
            .await?;
        let mysql = Mysql::default()
            .with_tag(MYSQL_IMAGE_TAG)
            .with_env_var("MYSQL_USER", TEST_USER)
            .with_env_var("MYSQL_PASSWORD", TEST_PASSWORD)
            .with_env_var("MYSQL_DATABASE", TEST_DATABASE)
            .with_env_var("MYSQL_ROOT_PASSWORD", TEST_PASSWORD)
            .start()
            .await?;
        let postgres_host = postgres.get_host().await?;
        let postgres_port = postgres.get_host_port_ipv4(5432).await?;
        let mysql_host = mysql.get_host().await?;
        let mysql_port = mysql.get_host_port_ipv4(3306).await?;
        Ok(Self {
            _postgres: postgres,
            _mysql: mysql,
            postgres_url: format_url("postgres", &postgres_host, postgres_port),
            mysql_url: format_url("mysql", &mysql_host, mysql_port),
            postgres_endpoint: format!("{postgres_host}:{postgres_port}"),
            mysql_endpoint: format!("{mysql_host}:{mysql_port}"),
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
