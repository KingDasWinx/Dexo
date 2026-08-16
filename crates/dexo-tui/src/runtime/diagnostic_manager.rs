use dexo_app::diagnostic_service::DiagnosticBundle;

#[derive(Default)]
pub struct DiagnosticManager {
    pub preview: Option<DiagnosticBundle>,
}
