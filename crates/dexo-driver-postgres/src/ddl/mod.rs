mod execute;
mod render;

pub use render::{PgDialect, render};

pub fn plan_ddl(
    change: &dexo_driver_api::SchemaChange,
) -> Result<dexo_driver_api::DdlPlan, dexo_driver_api::DriverError> {
    execute::plan_or_unsupported("postgres", change, render(change)?)
}
