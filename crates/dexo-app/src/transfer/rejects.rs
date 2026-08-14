use std::path::Path;

#[derive(Clone, Debug, PartialEq)]
pub struct RejectedRow {
    pub line: usize,
    pub safe_error: String,
    pub original_fields: Vec<String>,
}

pub fn write_rejects(path: &Path, rows: &[RejectedRow]) -> Result<(), String> {
    let mut body = String::from("line,error,fields\n");
    for row in rows {
        body.push_str(&format!(
            "{},{},{}\n",
            row.line,
            row.safe_error.replace(',', " "),
            row.original_fields.join("|")
        ));
    }
    let tmp = path.with_extension("part");
    std::fs::write(&tmp, body).map_err(|error| error.to_string())?;
    std::fs::rename(tmp, path).map_err(|error| error.to_string())
}
