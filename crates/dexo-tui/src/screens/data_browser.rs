//! Table-data browser state lives on `DataScreen`; this module keeps paging helpers.

use dexo_driver_api::{DataPage, Filter};

use super::data::DataScreen;

impl DataScreen {
    pub fn apply_page(&mut self, page: DataPage) {
        // A later page is not asked for the estimate again: it keeps the first page's.
        self.estimated_total = if page.offset == 0 {
            page.estimated_total
        } else {
            page.estimated_total.or(self.estimated_total)
        };
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

/// `customer_id = 1 AND name = 'O''Brien'`: the filter as a person reads it, each text
/// a literal as SQL writes it -- the values themselves go to the server bound.
pub fn describe_filter(filter: &Filter) -> String {
    let value = |value: &dexo_driver_api::DbValue| match value {
        dexo_driver_api::DbValue::Text(text) => format!("'{}'", text.replace('\'', "''")),
        other => dexo_app::data::display_value(other),
    };
    let join = |parts: &[Filter], with: &str| {
        parts
            .iter()
            .map(|part| match part {
                Filter::And(_) | Filter::Or(_) => format!("({})", describe_filter(part)),
                _ => describe_filter(part),
            })
            .collect::<Vec<_>>()
            .join(with)
    };
    match filter {
        Filter::Eq(column, v) => format!("{} = {}", column.0, value(v)),
        Filter::Ne(column, v) => format!("{} <> {}", column.0, value(v)),
        Filter::Gt(column, v) => format!("{} > {}", column.0, value(v)),
        Filter::Gte(column, v) => format!("{} >= {}", column.0, value(v)),
        Filter::Lt(column, v) => format!("{} < {}", column.0, value(v)),
        Filter::Lte(column, v) => format!("{} <= {}", column.0, value(v)),
        Filter::IsNull(column) => format!("{} IS NULL", column.0),
        Filter::IsNotNull(column) => format!("{} IS NOT NULL", column.0),
        Filter::And(parts) => join(parts, " AND "),
        Filter::Or(parts) => join(parts, " OR "),
        Filter::Not(inner) => format!("NOT ({})", describe_filter(inner)),
    }
}

#[cfg(test)]
mod tests {
    /// The second page keeps the estimate the first was given; a first page takes its own.
    #[test]
    fn later_pages_keep_the_first_pages_estimate() {
        let mut screen = super::DataScreen::default();
        let page = |offset: u64, estimate: Option<u64>| {
            let mut page =
                dexo_driver_api::DataPage::from_fetched(Vec::new(), Vec::new(), offset, 100);
            page.estimated_total = estimate;
            page
        };
        screen.apply_page(page(0, Some(5_000)));
        screen.apply_page(page(100, None));
        assert_eq!(screen.estimated_total, Some(5_000));
        screen.apply_page(page(0, None));
        assert_eq!(screen.estimated_total, None);
    }

    /// A text value reads as the SQL literal it stands for.
    #[test]
    fn a_quote_in_a_value_is_doubled() {
        let filter = dexo_driver_api::Filter::Eq(
            dexo_driver_api::ColumnId("name".into()),
            dexo_driver_api::DbValue::Text("O'Brien".into()),
        );
        assert_eq!(super::describe_filter(&filter), "name = 'O''Brien'");
    }
}
