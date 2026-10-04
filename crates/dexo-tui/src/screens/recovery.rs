#[derive(Clone, Debug, PartialEq)]
pub struct RecoveryScreen {
    pub open: bool,
    pub documents: Vec<String>,
    pub checkpoints: Vec<(String, String, String)>,
    pub transaction: String,
    pub confirm_discard: bool,
}

impl Default for RecoveryScreen {
    fn default() -> Self {
        Self {
            open: false,
            documents: Vec::new(),
            checkpoints: Vec::new(),
            transaction: "idle".into(),
            confirm_discard: false,
        }
    }
}

impl RecoveryScreen {
    pub fn fixture() -> Self {
        Self {
            open: true,
            documents: vec!["scratch.sql".into()],
            checkpoints: Vec::new(),
            transaction: "unknown".into(),
            confirm_discard: false,
        }
    }

    pub fn recover(&mut self) {
        self.open = false;
        self.confirm_discard = false;
    }

    pub fn restore_documents(&self) -> Vec<(String, String, String)> {
        self.checkpoints.clone()
    }

    pub fn discard(&mut self) {
        self.documents.clear();
        self.open = false;
        self.confirm_discard = false;
        self.transaction = "idle".into();
    }

    pub fn lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        if self.documents.is_empty() {
            lines.push("Nothing to recover: Dexo closed normally last time.".to_string());
        } else {
            lines.push("Dexo closed unexpectedly and kept these unsaved documents:".into());
            lines.extend(self.documents.iter().map(|doc| format!("  {doc}")));
        }
        if self.transaction != "idle" {
            lines.push(format!(
                "A transaction was open: its state is {}.",
                self.transaction
            ));
        }
        lines.push(String::new());
        lines.push(if self.confirm_discard {
            "Their text is lost. Press Discard again.".to_string()
        } else {
            "Keep leaves them open; Discard closes them.".to_string()
        });
        lines.push(" [Keep]   [Discard]".to_string());
        lines
    }
}
