mod execute;
mod render;

pub use render::{MysqlDialect, render};

pub fn plan_ddl(
    change: &dexo_driver_api::SchemaChange,
) -> Result<dexo_driver_api::DdlPlan, dexo_driver_api::DriverError> {
    execute::plan_or_unsupported("mysql", change, render(change)?)
}
