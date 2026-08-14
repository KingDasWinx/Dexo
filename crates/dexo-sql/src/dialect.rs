#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Dialect {
    Postgres,
    Mysql,
}

impl Dialect {
    pub fn name(self) -> &'static str {
        match self {
            Self::Postgres => "postgres",
            Self::Mysql => "mysql",
        }
    }
}
