use crate::transfer::codec::FormatOptions;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InferredType {
    Bool,
    Int,
    Decimal,
    Date,
    Text,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ColumnPreview {
    pub name: String,
    pub inferred: InferredType,
    pub nullable: bool,
    pub confidence: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Detection {
    pub encoding: &'static encoding_rs::Encoding,
    pub delimiter: u8,
    pub header: bool,
    pub columns: Vec<ColumnPreview>,
    pub sample: Vec<Vec<String>>,
    pub confidence: f32,
}

pub const PREFIX_LIMIT: usize = 64 * 1024;

pub fn detect(bytes: &[u8]) -> Detection {
    let prefix = &bytes[..bytes.len().min(PREFIX_LIMIT)];
    let (encoding, rest) = encoding_of(prefix);
    let (text, _, _) = encoding.decode(rest);
    let delimiter = delimiter_of(&text);
    let lines: Vec<&str> = text.lines().take(20).collect();
    let header = looks_like_header(lines.first().copied().unwrap_or(""), delimiter);
    let mut sample = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        if header && index == 0 {
            continue;
        }
        sample.push(split_line(line, delimiter));
        if sample.len() == 8 {
            break;
        }
    }
    let names = if header {
        split_line(lines.first().copied().unwrap_or(""), delimiter)
    } else {
        (0..sample.first().map(Vec::len).unwrap_or(0))
            .map(|i| format!("c{i}"))
            .collect()
    };
    let columns = names
        .iter()
        .enumerate()
        .map(|(index, name)| {
            let values: Vec<&str> = sample
                .iter()
                .filter_map(|row| row.get(index).map(String::as_str))
                .collect();
            infer_column(name, &values)
        })
        .collect();
    Detection {
        encoding,
        delimiter,
        header,
        columns,
        sample,
        confidence: if encoding == encoding_rs::UTF_8 && (delimiter == b',' || delimiter == b'\t') {
            0.9
        } else {
            0.6
        },
    }
}

pub fn apply_overrides(detection: Detection, options: &FormatOptions) -> Detection {
    let mut detection = detection;
    detection.delimiter = options.delimiter;
    detection.header = options.header;
    detection.encoding = options.encoding;
    detection
}

fn encoding_of(bytes: &[u8]) -> (&'static encoding_rs::Encoding, &[u8]) {
    if bytes.starts_with(&[0xef, 0xbb, 0xbf]) {
        (encoding_rs::UTF_8, &bytes[3..])
    } else if bytes.starts_with(&[0xff, 0xfe]) {
        (encoding_rs::UTF_16LE, &bytes[2..])
    } else if bytes.starts_with(&[0xfe, 0xff]) {
        (encoding_rs::UTF_16BE, &bytes[2..])
    } else {
        (encoding_rs::UTF_8, bytes)
    }
}

fn delimiter_of(text: &str) -> u8 {
    let first = text.lines().next().unwrap_or("");
    let comma = first.bytes().filter(|b| *b == b',').count();
    let tab = first.bytes().filter(|b| *b == b'\t').count();
    let semi = first.bytes().filter(|b| *b == b';').count();
    if tab >= comma && tab >= semi && tab > 0 {
        b'\t'
    } else if semi > comma {
        b';'
    } else {
        b','
    }
}

fn looks_like_header(line: &str, delimiter: u8) -> bool {
    let fields = split_line(line, delimiter);
    !fields.is_empty()
        && fields.iter().all(|field| {
            !field.is_empty()
                && field
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == ' ')
                && field.parse::<f64>().is_err()
        })
}

fn split_line(line: &str, delimiter: u8) -> Vec<String> {
    line.split(delimiter as char).map(str::to_string).collect()
}

fn infer_column(name: &str, values: &[&str]) -> ColumnPreview {
    let nullable = values.iter().any(|value| value.is_empty());
    let inferred = if values
        .iter()
        .all(|value| value.is_empty() || *value == "true" || *value == "false")
    {
        InferredType::Bool
    } else if values
        .iter()
        .all(|value| value.is_empty() || value.parse::<i64>().is_ok())
    {
        InferredType::Int
    } else if values
        .iter()
        .all(|value| value.is_empty() || value.parse::<f64>().is_ok())
    {
        InferredType::Decimal
    } else if values.iter().all(|value| {
        value.is_empty()
            || (value.len() == 10
                && value.as_bytes().get(4) == Some(&b'-')
                && value.as_bytes().get(7) == Some(&b'-'))
    }) {
        InferredType::Date
    } else {
        InferredType::Text
    };
    ColumnPreview {
        name: name.to_string(),
        inferred,
        nullable,
        confidence: 0.8,
    }
}

#[cfg(test)]
mod tests {
    use super::{InferredType, detect};
    use crate::transfer::codec::FormatOptions;
    use crate::transfer::detect::apply_overrides;

    #[test]
    fn detects_utf8_csv_header_and_types() {
        let csv = b"id,active,amount,when\n1,true,1.5,2026-08-14\n2,false,2.0,2026-08-15\n";
        let detected = detect(csv);
        assert_eq!(detected.encoding, encoding_rs::UTF_8);
        assert_eq!(detected.delimiter, b',');
        assert!(detected.header);
        assert_eq!(detected.columns[0].inferred, InferredType::Int);
        assert_eq!(detected.columns[1].inferred, InferredType::Bool);
        assert_eq!(detected.columns[2].inferred, InferredType::Decimal);
        assert_eq!(detected.columns[3].inferred, InferredType::Date);
        let overridden = apply_overrides(detected, &FormatOptions::default());
        assert!(overridden.header);
    }

    #[test]
    fn detects_utf16_tab_and_semicolon() {
        let utf16: Vec<u8> = {
            let mut out = vec![0xff, 0xfe];
            for unit in "a\tb\n1\t2".encode_utf16() {
                out.extend_from_slice(&unit.to_le_bytes());
            }
            out
        };
        let detected = detect(&utf16);
        assert_eq!(detected.encoding, encoding_rs::UTF_16LE);
        assert_eq!(detected.delimiter, b'\t');
        let semi = detect(b"a;b\n1;x");
        assert_eq!(semi.delimiter, b';');
    }

    #[test]
    fn malformed_line_is_still_previewed() {
        let detected = detect(b"a,b\n1,2,3\n");
        assert_eq!(detected.sample[0].len(), 3);
    }
}
