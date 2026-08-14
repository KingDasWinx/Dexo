use uuid::Uuid;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct TaskId(pub Uuid);

#[derive(Clone, Debug, PartialEq)]
pub enum AppEvent {
    TaskStarted(TaskId),
    TaskProgress {
        id: TaskId,
        completed: u64,
        total: Option<u64>,
    },
    TaskFinished(TaskId),
    TaskFailed {
        id: TaskId,
        message: String,
    },
}
