use dexo_app::Project;
use dexo_driver_api::TransactionState;

use crate::action::Effect;
use crate::model::Model;
use crate::runtime::OperationId;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProjectSwitchStage {
    ConfirmDirty,
    FlushDocuments,
    PersistLayout,
    CloseProjectSessions,
    LoadTarget,
    Complete,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ProjectSwitch {
    pub stage: ProjectSwitchStage,
    pub target: Project,
    pub operation: OperationId,
}

pub fn begin_switch(model: &Model, target: Project) -> Result<ProjectSwitch, String> {
    if model.transaction != TransactionState::Idle {
        return Err("commit or rollback the active transaction before switching projects".into());
    }
    let stage = if model.documents.iter().any(|document| document.is_dirty()) {
        ProjectSwitchStage::ConfirmDirty
    } else {
        ProjectSwitchStage::FlushDocuments
    };
    Ok(ProjectSwitch {
        stage,
        target,
        operation: OperationId::new(),
    })
}

pub fn advance(model: &Model, switch: &ProjectSwitch) -> Vec<Effect> {
    match switch.stage {
        ProjectSwitchStage::ConfirmDirty => Vec::new(),
        ProjectSwitchStage::FlushDocuments => vec![Effect::FlushDocuments {
            project_id: model.project_id.clone(),
            documents: model
                .documents
                .iter()
                .map(|document| crate::action::FlushedDocument {
                    id: document.id.clone(),
                    title: document.title.clone(),
                    content: document.text(),
                    path: document.path.clone(),
                })
                .collect(),
        }],
        ProjectSwitchStage::PersistLayout => vec![Effect::PersistLayout {
            project_id: model.project_id.clone(),
            layout: model.workbench_layout(),
        }],
        ProjectSwitchStage::CloseProjectSessions => model
            .active_session
            .map(|session| Effect::CloseSession { session })
            .into_iter()
            .chain(std::iter::once(Effect::CloseProjectSessions))
            .collect(),
        ProjectSwitchStage::LoadTarget => vec![Effect::LoadProject {
            id: switch.target.id.0.to_string(),
        }],
        ProjectSwitchStage::Complete => Vec::new(),
    }
}

pub fn next_stage(stage: ProjectSwitchStage) -> ProjectSwitchStage {
    match stage {
        ProjectSwitchStage::ConfirmDirty => ProjectSwitchStage::FlushDocuments,
        ProjectSwitchStage::FlushDocuments => ProjectSwitchStage::PersistLayout,
        ProjectSwitchStage::PersistLayout => ProjectSwitchStage::CloseProjectSessions,
        ProjectSwitchStage::CloseProjectSessions => ProjectSwitchStage::LoadTarget,
        ProjectSwitchStage::LoadTarget => ProjectSwitchStage::Complete,
        ProjectSwitchStage::Complete => ProjectSwitchStage::Complete,
    }
}
