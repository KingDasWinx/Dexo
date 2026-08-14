use dexo_driver_api::{DataRequest, QualifiedName};

#[derive(Clone, Debug, PartialEq)]
pub struct DataSource {
    pub object: QualifiedName,
}

impl DataSource {
    pub fn request(&self, request: DataRequest) -> DataRequest {
        DataRequest {
            object: self.object.clone(),
            ..request
        }
    }
}
