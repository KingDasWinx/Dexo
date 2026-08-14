use dexo_app::schema_diff::{
    OrderedChange, SchemaDifference, classify_difference, generate_script, render_unquoted,
};

#[derive(Clone, Debug, PartialEq)]
pub struct DiffEntry {
    pub kind: &'static str,
    pub object: String,
    pub risk: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SchemaDiffScreen {
    pub open: bool,
    pub from_label: String,
    pub to_label: String,
    pub show_added: bool,
    pub show_removed: bool,
    pub show_changed: bool,
    pub entries: Vec<DiffEntry>,
    pub selected: usize,
    pub script: String,
    pub confirmed: bool,
    pub applied: bool,
}

impl Default for SchemaDiffScreen {
    fn default() -> Self {
        Self {
            open: false,
            from_label: String::new(),
            to_label: String::new(),
            show_added: true,
            show_removed: true,
            show_changed: true,
            entries: Vec::new(),
            selected: 0,
            script: String::new(),
            confirmed: false,
            applied: false,
        }
    }
}

impl SchemaDiffScreen {
    pub fn from_ordered(
        from_label: impl Into<String>,
        to_label: impl Into<String>,
        ordered: &[OrderedChange],
    ) -> Self {
        let script = generate_script(ordered, render_unquoted).forward;
        let entries = ordered
            .iter()
            .map(|item| {
                let risk = classify_difference(&item.difference);
                let (kind, object) = match &item.difference {
                    SchemaDifference::Added(object) => {
                        ("added", object.qualified_name.display_unquoted())
                    }
                    SchemaDifference::Removed(object) => {
                        ("removed", object.qualified_name.display_unquoted())
                    }
                    SchemaDifference::Changed { after, .. } => {
                        ("changed", after.qualified_name.display_unquoted())
                    }
                };
                DiffEntry {
                    kind,
                    object,
                    risk: format!(
                        "destructive={} data_loss={} reversible={}",
                        risk.destructive, risk.data_loss, risk.reversible
                    ),
                }
            })
            .collect();
        Self {
            open: true,
            from_label: from_label.into(),
            to_label: to_label.into(),
            show_added: true,
            show_removed: true,
            show_changed: true,
            entries,
            selected: 0,
            script,
            confirmed: false,
            applied: false,
        }
    }

    pub fn fixture() -> Self {
        Self {
            open: true,
            from_label: "prod@v1".into(),
            to_label: "prod@v2".into(),
            show_added: true,
            show_removed: true,
            show_changed: true,
            entries: vec![
                DiffEntry {
                    kind: "added",
                    object: "db.public.orders_new".into(),
                    risk: "destructive=false data_loss=false reversible=true".into(),
                },
                DiffEntry {
                    kind: "removed",
                    object: "db.public.gone".into(),
                    risk: "destructive=true data_loss=true reversible=false".into(),
                },
                DiffEntry {
                    kind: "changed",
                    object: "db.public.users".into(),
                    risk: "destructive=false data_loss=true reversible=false".into(),
                },
            ],
            selected: 0,
            script: "-- dexo:risk destructive=true data_loss=true lock=AccessExclusive reversible=false\nDROP TABLE db.public.gone;\n".into(),
            confirmed: false,
            applied: false,
        }
    }

    pub fn filtered(&self) -> Vec<&DiffEntry> {
        self.entries
            .iter()
            .filter(|entry| match entry.kind {
                "added" => self.show_added,
                "removed" => self.show_removed,
                "changed" => self.show_changed,
                _ => true,
            })
            .collect()
    }

    pub fn toggle_added(&mut self) {
        self.show_added = !self.show_added;
    }

    pub fn toggle_removed(&mut self) {
        self.show_removed = !self.show_removed;
    }

    pub fn toggle_changed(&mut self) {
        self.show_changed = !self.show_changed;
    }

    pub fn confirm(&mut self) {
        self.confirmed = true;
    }

    pub fn apply(&mut self) {
        if self.confirmed {
            self.applied = true;
        }
    }

    pub fn lines(&self) -> Vec<String> {
        let mut lines = vec![
            format!("{} -> {}", self.from_label, self.to_label),
            format!(
                "filters added={} removed={} changed={}",
                self.show_added, self.show_removed, self.show_changed
            ),
            format!(
                "confirm={} apply={}",
                self.confirmed,
                if self.applied { "done" } else { "blocked" }
            ),
        ];
        for (index, entry) in self.filtered().into_iter().enumerate() {
            let marker = if index == self.selected { ">" } else { " " };
            lines.push(format!(
                "{marker} {} {} {}",
                entry.kind, entry.object, entry.risk
            ));
        }
        lines.push("--- script ---".into());
        lines.extend(self.script.lines().map(str::to_string));
        lines
    }
}

#[cfg(test)]
mod tests {
    use super::SchemaDiffScreen;

    #[test]
    fn filters_hide_removed_and_apply_stays_blocked() {
        let mut screen = SchemaDiffScreen::fixture();
        screen.toggle_removed();
        let visible: Vec<_> = screen
            .filtered()
            .iter()
            .map(|entry| entry.object.as_str())
            .collect();
        assert!(!visible.iter().any(|object| object.contains("gone")));
        assert!(visible.iter().any(|object| object.contains("orders_new")));
        let dump = screen.lines().join("\n");
        assert!(dump.contains("apply=blocked"));
        assert!(dump.contains("destructive=true"));
        assert!(dump.contains("DROP TABLE"));
        screen.apply();
        assert!(!screen.applied);
        screen.confirm();
        screen.apply();
        assert!(screen.applied);
    }
}
