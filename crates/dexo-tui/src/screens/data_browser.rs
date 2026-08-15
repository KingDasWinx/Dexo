//! Table-data browser state lives on `DataScreen`; this module keeps paging helpers.

use dexo_driver_api::{DataPage, Filter};

use super::data::DataScreen;

impl DataScreen {
    pub fn apply_page(&mut self, page: DataPage) {
        self.page_offset = page.offset;
        self.has_more = page.has_more;
        self.loading = false;
        self.last_error = None;
    }

    pub fn filter_chips(&self) -> Vec<String> {
        match &self.filter {
            Some(filter) => vec![format!("{filter:?}")],
            None => Vec::new(),
        }
    }
}

pub fn describe_filter(filter: &Filter) -> String {
    format!("{filter:?}")
}
