#[derive(Clone, Debug, PartialEq)]
pub struct RecoveryScreen {
    pub open: bool,
    pub documents: Vec<String>,
    pub transaction: String,
    pub confirm_discard: bool,
}

impl Default for RecoveryScreen {
    fn default() -> Self {
        Self {
            open: false,
            documents: Vec::new(),
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
            transaction: "unknown".into(),
            confirm_discard: false,
        }
    }

    pub fn recover(&mut self) {
        self.open = false;
        self.confirm_discard = false;
    }

    pub fn discard(&mut self) {
        self.documents.clear();
        self.open = false;
        self.confirm_discard = false;
        self.transaction = "idle".into();
    }

    pub fn lines(&self) -> Vec<String> {
        let mut lines = vec![
            format!("recovery open={}", self.open),
            format!("transaction={}", self.transaction),
            format!("confirm_discard={}", self.confirm_discard),
        ];
        for doc in &self.documents {
            lines.push(format!("document {doc}"));
        }
        if self.transaction == "active" {
            lines.push("BUG active transaction after crash".into());
        }
        lines
    }
}
