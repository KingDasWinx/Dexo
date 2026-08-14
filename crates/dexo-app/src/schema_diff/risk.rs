use dexo_driver_api::{ChangeRisk, LockLevel, ObjectKind};

use crate::schema_diff::diff::SchemaDifference;

pub fn classify_difference(difference: &SchemaDifference) -> ChangeRisk {
    match difference {
        SchemaDifference::Added(_) => ChangeRisk {
            destructive: false,
            data_loss: false,
            lock_level: LockLevel::AccessExclusive,
            reversible: true,
        },
        SchemaDifference::Removed(object) => ChangeRisk {
            destructive: true,
            data_loss: object.kind == ObjectKind::Table,
            lock_level: LockLevel::AccessExclusive,
            reversible: false,
        },
        SchemaDifference::Changed { before, after } => {
            let type_changed = before.attributes.get("type") != after.attributes.get("type");
            ChangeRisk {
                destructive: false,
                data_loss: type_changed,
                lock_level: LockLevel::AccessExclusive,
                reversible: !type_changed,
            }
        }
    }
}

pub fn comment(risk: ChangeRisk, manual: bool) -> String {
    format!(
        "-- dexo:risk destructive={} data_loss={} lock={:?} reversible={}{}",
        risk.destructive,
        risk.data_loss,
        risk.lock_level,
        risk.reversible,
        if manual { " manual=cycle" } else { "" }
    )
}
