//! Values as DuckDB writes them when cast to VARCHAR, read off the Arrow arrays a result
//! arrives in. Arrow's own formatter wrote a struct as `{a: 1}`, left the strings in a
//! list unquoted, wrapped a UHUGEINT past 2^127 negative and failed on an infinite date;
//! what it wrote did not always cast back to the value, and an edit keyed by it missed
//! its row.

use chrono::{Datelike, NaiveDate};
use duckdb::arrow::array::{
    Array, AsArray, FixedSizeBinaryArray, LargeListArray, ListArray, MapArray, StructArray,
    UnionArray,
};
use duckdb::arrow::compute::cast;
use duckdb::arrow::datatypes::{
    DataType, Date32Type, Decimal128Type, Field, Float16Type, Float32Type, Float64Type, Int8Type,
    Int16Type, Int32Type, Int64Type, IntervalDayTimeType, IntervalMonthDayNanoType, IntervalUnit,
    IntervalYearMonthType, Time32MillisecondType, Time32SecondType, Time64MicrosecondType,
    Time64NanosecondType, TimeUnit, TimestampMicrosecondType, TimestampMillisecondType,
    TimestampNanosecondType, TimestampSecondType, UInt8Type, UInt16Type, UInt32Type, UInt64Type,
};
use duckdb::arrow::util::display::{ArrayFormatter, FormatOptions};

/// A DuckDB type Arrow has no type for, which the lossless conversion hands over as raw
/// bytes named in the field's metadata -- and a BOOLEAN, which it sends as a byte.
pub fn opaque_type(field: &Field) -> Option<&str> {
    let metadata = field.metadata();
    match metadata.get("ARROW:extension:name").map(String::as_str) {
        Some("arrow.uuid") => Some("uuid"),
        Some("arrow.bool8") => Some("bool8"),
        Some("arrow.opaque") => {
            let details = metadata.get("ARROW:extension:metadata")?;
            let start = details.find("\"type_name\":\"")? + "\"type_name\":\"".len();
            let end = start + details[start..].find('"')?;
            Some(&details[start..end])
        }
        _ => None,
    }
}

/// The value at `row`, as DuckDB casts it to VARCHAR; `None` for NULL.
pub fn render(array: &dyn Array, field: &Field, row: usize) -> Option<String> {
    if array.is_null(row) {
        return None;
    }
    if let Some(opaque) = opaque_type(field) {
        return opaque_text(array, opaque, row);
    }
    macro_rules! number {
        ($kind:ty) => {
            array.as_primitive::<$kind>().value(row).to_string()
        };
    }
    Some(match array.data_type() {
        DataType::Null => return None,
        DataType::Boolean => array.as_boolean().value(row).to_string(),
        DataType::Int8 => number!(Int8Type),
        DataType::Int16 => number!(Int16Type),
        DataType::Int32 => number!(Int32Type),
        DataType::Int64 => number!(Int64Type),
        DataType::UInt8 => number!(UInt8Type),
        DataType::UInt16 => number!(UInt16Type),
        DataType::UInt32 => number!(UInt32Type),
        DataType::UInt64 => number!(UInt64Type),
        DataType::Float16 => float_text(f64::from(array.as_primitive::<Float16Type>().value(row))),
        DataType::Float32 => float32_text(array.as_primitive::<Float32Type>().value(row)),
        DataType::Float64 => float_text(array.as_primitive::<Float64Type>().value(row)),
        DataType::Decimal128(_, scale) => {
            decimal_text(array.as_primitive::<Decimal128Type>().value(row), *scale)
        }
        DataType::Utf8 => array.as_string::<i32>().value(row).to_string(),
        DataType::LargeUtf8 => array.as_string::<i64>().value(row).to_string(),
        DataType::Utf8View => array.as_string_view().value(row).to_string(),
        DataType::Binary => blob_text(array.as_binary::<i32>().value(row)),
        DataType::LargeBinary => blob_text(array.as_binary::<i64>().value(row)),
        DataType::BinaryView => blob_text(array.as_binary_view().value(row)),
        DataType::Date32 => date_text(array.as_primitive::<Date32Type>().value(row)),
        DataType::Time32(TimeUnit::Second) => time_text(
            i64::from(array.as_primitive::<Time32SecondType>().value(row)) * 1_000_000_000,
        ),
        DataType::Time32(_) => time_text(
            i64::from(array.as_primitive::<Time32MillisecondType>().value(row)) * 1_000_000,
        ),
        DataType::Time64(TimeUnit::Nanosecond) => {
            time_text(array.as_primitive::<Time64NanosecondType>().value(row))
        }
        DataType::Time64(_) => {
            time_text(array.as_primitive::<Time64MicrosecondType>().value(row) * 1_000)
        }
        DataType::Timestamp(unit, zone) => {
            let value = match unit {
                TimeUnit::Second => array.as_primitive::<TimestampSecondType>().value(row),
                TimeUnit::Millisecond => {
                    array.as_primitive::<TimestampMillisecondType>().value(row)
                }
                TimeUnit::Microsecond => {
                    array.as_primitive::<TimestampMicrosecondType>().value(row)
                }
                TimeUnit::Nanosecond => array.as_primitive::<TimestampNanosecondType>().value(row),
            };
            timestamp_text(value, *unit, zone.is_some())
        }
        DataType::Interval(IntervalUnit::MonthDayNano) => {
            let value = array.as_primitive::<IntervalMonthDayNanoType>().value(row);
            interval_text(value.months, value.days, value.nanoseconds / 1_000)
        }
        DataType::Interval(IntervalUnit::DayTime) => {
            let value = array.as_primitive::<IntervalDayTimeType>().value(row);
            interval_text(0, value.days, i64::from(value.milliseconds) * 1_000)
        }
        DataType::Interval(IntervalUnit::YearMonth) => interval_text(
            array.as_primitive::<IntervalYearMonthType>().value(row),
            0,
            0,
        ),
        DataType::List(child) => {
            let list = array.as_any().downcast_ref::<ListArray>()?;
            elements(list.value(row).as_ref(), child)
        }
        DataType::LargeList(child) => {
            let list = array.as_any().downcast_ref::<LargeListArray>()?;
            elements(list.value(row).as_ref(), child)
        }
        DataType::FixedSizeList(child, _) => {
            elements(array.as_fixed_size_list().value(row).as_ref(), child)
        }
        DataType::Struct(fields) => {
            let values = array.as_any().downcast_ref::<StructArray>()?;
            let parts = fields
                .iter()
                .zip(values.columns())
                .map(|(field, column)| {
                    format!(
                        "{}: {}",
                        quoted(field.name()),
                        element_at(column.as_ref(), field, row)
                    )
                })
                .collect::<Vec<_>>();
            format!("{{{}}}", parts.join(", "))
        }
        DataType::Map(entries, _) => {
            let map = array.as_any().downcast_ref::<MapArray>()?;
            let DataType::Struct(fields) = entries.data_type() else {
                return None;
            };
            let pairs = map.value(row);
            let (keys, values) = (pairs.column(0), pairs.column(1));
            let parts = (0..pairs.len())
                .map(|index| {
                    format!(
                        "{}={}",
                        element_at(keys.as_ref(), &fields[0], index),
                        element_at(values.as_ref(), &fields[1], index)
                    )
                })
                .collect::<Vec<_>>();
            format!("{{{}}}", parts.join(", "))
        }
        // A union is the value of the member it holds; a NULL member is a NULL union.
        DataType::Union(fields, _) => {
            let union = array.as_any().downcast_ref::<UnionArray>()?;
            let kind = union.type_id(row);
            let (_, member) = fields.iter().find(|(id, _)| *id == kind)?;
            return render(union.child(kind).as_ref(), member, union.value_offset(row));
        }
        DataType::Dictionary(..) => {
            let text = cast(&array.slice(row, 1), &DataType::Utf8).ok()?;
            text.as_string::<i32>().value(0).to_string()
        }
        _ => {
            let formatter = ArrayFormatter::try_new(array, &FormatOptions::default()).ok()?;
            formatter.value(row).to_string()
        }
    })
}

fn opaque_text(array: &dyn Array, opaque: &str, row: usize) -> Option<String> {
    if opaque == "bool8" {
        return Some((array.as_primitive::<Int8Type>().value(row) != 0).to_string());
    }
    let bytes = match array.data_type() {
        DataType::FixedSizeBinary(_) => array
            .as_any()
            .downcast_ref::<FixedSizeBinaryArray>()?
            .value(row),
        DataType::Binary => array.as_binary::<i32>().value(row),
        DataType::LargeBinary => array.as_binary::<i64>().value(row),
        _ => return None,
    };
    Some(match opaque {
        "hugeint" => i128::from_le_bytes(bytes.try_into().ok()?).to_string(),
        "uhugeint" => u128::from_le_bytes(bytes.try_into().ok()?).to_string(),
        "uuid" => uuid_text(bytes)?,
        "time_tz" => time_tz_text(u64::from_le_bytes(bytes.try_into().ok()?)),
        "bignum" => bignum_text(bytes)?,
        "bit" => bit_text(bytes),
        _ => blob_text(bytes),
    })
}

/// The elements of a list in brackets, each as an element of a nested value.
fn elements(values: &dyn Array, field: &Field) -> String {
    let parts = (0..values.len())
        .map(|index| element_at(values, field, index))
        .collect::<Vec<_>>();
    format!("[{}]", parts.join(", "))
}

/// A value inside a list, struct or map: `NULL` for NULL, and quoted where its text
/// would otherwise read as part of the nesting -- a comma, a bracket, a colon, a word
/// `NULL`, space at either end. A nested value and a union's member are not quoted.
fn element_at(array: &dyn Array, field: &Field, index: usize) -> String {
    let Some(text) = render(array, field, index) else {
        return "NULL".into();
    };
    let nested = matches!(
        array.data_type(),
        DataType::List(_)
            | DataType::LargeList(_)
            | DataType::FixedSizeList(..)
            | DataType::Struct(_)
            | DataType::Map(..)
            | DataType::Union(..)
    );
    let json = field
        .metadata()
        .get("ARROW:extension:name")
        .map(String::as_str)
        == Some("arrow.json");
    if nested || json || !needs_quotes(&text) {
        text
    } else {
        quoted(&text)
    }
}

fn needs_quotes(text: &str) -> bool {
    text.is_empty()
        || text.starts_with(char::is_whitespace)
        || text.ends_with(char::is_whitespace)
        || text.eq_ignore_ascii_case("null")
        || text.contains([',', '\'', '"', '(', ')', '{', '}', '[', ']', ':', '='])
}

fn quoted(text: &str) -> String {
    format!("'{}'", text.replace('\\', "\\\\").replace('\'', "\\'"))
}

/// The shortest text that reads back as the same number, the way DuckDB writes it:
/// `1.0`, `0.0001`, `1e-05`, `1e+16`, `nan`, `-inf`.
pub fn float_text(value: f64) -> String {
    if value.is_nan() {
        return "nan".into();
    }
    if value.is_infinite() {
        return if value > 0.0 { "inf" } else { "-inf" }.into();
    }
    if value == 0.0 {
        return "0.0".into();
    }
    exponent_signed(format!("{value:?}"))
}

pub fn float32_text(value: f32) -> String {
    if value.is_nan() || value.is_infinite() || value == 0.0 {
        return float_text(f64::from(value));
    }
    exponent_signed(format!("{value:?}"))
}

/// `1e16` as `1e+16`, `1e-5` as `1e-05`.
fn exponent_signed(text: String) -> String {
    let Some((mantissa, exponent)) = text.split_once('e') else {
        return text;
    };
    let (sign, digits) = match exponent.strip_prefix('-') {
        Some(digits) => ("-", digits),
        None => ("+", exponent),
    };
    format!("{mantissa}e{sign}{digits:0>2}")
}

/// `value` with `scale` digits after the point. Arrow's formatter cuts a value to the
/// type's precision, and a HUGEINT's largest values have 39 digits.
pub fn decimal_text(value: i128, scale: i8) -> String {
    let sign = if value < 0 { "-" } else { "" };
    let digits = value.unsigned_abs().to_string();
    let Ok(scale) = usize::try_from(scale) else {
        return format!(
            "{sign}{digits}{}",
            "0".repeat(usize::from(scale.unsigned_abs()))
        );
    };
    if scale == 0 {
        return format!("{sign}{digits}");
    }
    let padded = format!("{digits:0>width$}", width = scale + 1);
    let (whole, fraction) = padded.split_at(padded.len() - scale);
    format!("{sign}{whole}.{fraction}")
}

/// DuckDB's infinite dates and timestamps are the largest values their type holds.
const DATE_INFINITY: i32 = i32::MAX;
const TIMESTAMP_INFINITY: i64 = i64::MAX;

/// `2020-01-02`, `12345-01-01`, `0044-03-15 (BC)`, `infinity`.
fn date_text(days: i32) -> String {
    match days {
        DATE_INFINITY => return "infinity".into(),
        days if days == -DATE_INFINITY => return "-infinity".into(),
        _ => {}
    }
    let (date, bc) = date_parts(i64::from(days));
    format!("{date}{bc}")
}

/// The date `days` after 1970-01-01, and ` (BC)` for a year before 1.
fn date_parts(days: i64) -> (String, &'static str) {
    const UNIX_EPOCH_FROM_CE: i64 = 719_163;
    let Some(date) = i32::try_from(days + UNIX_EPOCH_FROM_CE)
        .ok()
        .and_then(NaiveDate::from_num_days_from_ce_opt)
    else {
        return (days.to_string(), "");
    };
    let (year, bc) = if date.year() <= 0 {
        (1 - date.year(), " (BC)")
    } else {
        (date.year(), "")
    };
    (
        format!("{year:04}-{:02}-{:02}", date.month(), date.day()),
        bc,
    )
}

/// `12:00:00`, `12:00:00.000001`, `12:00:00.123456789`: the fraction without its
/// trailing zeros.
fn time_text(nanoseconds: i64) -> String {
    let seconds = nanoseconds.div_euclid(1_000_000_000);
    let fraction = nanoseconds.rem_euclid(1_000_000_000);
    let mut text = format!(
        "{:02}:{:02}:{:02}",
        seconds / 3_600,
        (seconds / 60) % 60,
        seconds % 60
    );
    if fraction != 0 {
        text.push('.');
        text.push_str(format!("{fraction:09}").trim_end_matches('0'));
    }
    text
}

fn timestamp_text(value: i64, unit: TimeUnit, zoned: bool) -> String {
    match value {
        TIMESTAMP_INFINITY => return "infinity".into(),
        value if value == -TIMESTAMP_INFINITY => return "-infinity".into(),
        _ => {}
    }
    let per_second: i64 = match unit {
        TimeUnit::Second => 1,
        TimeUnit::Millisecond => 1_000,
        TimeUnit::Microsecond => 1_000_000,
        TimeUnit::Nanosecond => 1_000_000_000,
    };
    let seconds = value.div_euclid(per_second);
    let nanoseconds = value.rem_euclid(per_second) * (1_000_000_000 / per_second);
    let (date, bc) = date_parts(seconds.div_euclid(86_400));
    let time = time_text(seconds.rem_euclid(86_400) * 1_000_000_000 + nanoseconds);
    // DuckDB writes a TIMESTAMPTZ in its session's zone, UTC without its ICU extension;
    // the values are UTC instants, written here in UTC.
    let zone = if zoned { "+00" } else { "" };
    format!("{date} {time}{zone}{bc}")
}

/// `1 year 2 months 3 days 04:05:06.5`, `-1 day`, `36:00:00`, `00:00:00`.
fn interval_text(months: i32, days: i32, microseconds: i64) -> String {
    let plural = |count: i64, unit: &str| {
        format!("{count} {unit}{}", if count.abs() == 1 { "" } else { "s" })
    };
    let mut parts = Vec::new();
    let (years, months) = (i64::from(months / 12), i64::from(months % 12));
    if years != 0 {
        parts.push(plural(years, "year"));
    }
    if months != 0 {
        parts.push(plural(months, "month"));
    }
    if days != 0 {
        parts.push(plural(i64::from(days), "day"));
    }
    if microseconds != 0 || parts.is_empty() {
        let sign = if microseconds < 0 { "-" } else { "" };
        let total = microseconds.unsigned_abs();
        let mut time = format!(
            "{sign}{:02}:{:02}:{:02}",
            total / 3_600_000_000,
            (total / 60_000_000) % 60,
            (total / 1_000_000) % 60
        );
        let fraction = total % 1_000_000;
        if fraction != 0 {
            time.push('.');
            time.push_str(format!("{fraction:06}").trim_end_matches('0'));
        }
        parts.push(time);
    }
    parts.join(" ")
}

/// A TIME WITH TIME ZONE: the time of day in its upper 40 bits, and in its lower 24 the
/// zone's offset in seconds, stored as how far it is below the largest one, 15:59:59.
fn time_tz_text(bits: u64) -> String {
    const MAX_OFFSET: i64 = 16 * 60 * 60 - 1;
    let microseconds = i64::try_from(bits >> 24).unwrap_or(0);
    let offset = MAX_OFFSET - i64::try_from(bits & 0xFF_FFFF).unwrap_or(0);
    let sign = if offset < 0 { '-' } else { '+' };
    let offset = offset.abs();
    let mut zone = format!("{sign}{:02}", offset / 3_600);
    if offset % 3_600 != 0 {
        zone.push_str(&format!(":{:02}", (offset / 60) % 60));
        if offset % 60 != 0 {
            zone.push_str(&format!(":{:02}", offset % 60));
        }
    }
    format!("{}{zone}", time_text(microseconds * 1_000))
}

/// A BIGNUM: three bytes of header -- the top bit set for a number not below zero, the
/// rest the count of bytes that follow -- then the magnitude, big-endian. A negative
/// number has every bit flipped, header included.
fn bignum_text(bytes: &[u8]) -> Option<String> {
    let negative = bytes.first()? & 0x80 == 0;
    let magnitude: Vec<u8> = bytes
        .get(3..)?
        .iter()
        .map(|byte| if negative { !byte } else { *byte })
        .collect();
    // Repeated division by 10^9, most significant base-10^9 digit last.
    let mut digits: Vec<u32> = Vec::new();
    let mut number = magnitude;
    while number.iter().any(|byte| *byte != 0) {
        let mut remainder: u64 = 0;
        for byte in &mut number {
            let value = (remainder << 8) | u64::from(*byte);
            *byte = u8::try_from(value / 1_000_000_000).unwrap_or(0);
            remainder = value % 1_000_000_000;
        }
        digits.push(u32::try_from(remainder).ok()?);
    }
    let Some((last, rest)) = digits.split_last() else {
        return Some("0".into());
    };
    let mut text = String::from(if negative { "-" } else { "" });
    text.push_str(&last.to_string());
    for chunk in rest.iter().rev() {
        text.push_str(&format!("{chunk:09}"));
    }
    Some(text)
}

fn uuid_text(bytes: &[u8]) -> Option<String> {
    if bytes.len() != 16 {
        return None;
    }
    let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    Some(format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    ))
}

/// A BLOB as DuckDB writes it: printable ASCII as itself, every other byte -- quotes and
/// the backslash included -- as `\xHH`.
fn blob_text(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len());
    for byte in bytes {
        if (32..=126).contains(byte) && !matches!(byte, b'\\' | b'\'' | b'"') {
            text.push(char::from(*byte));
        } else {
            text.push_str(&format!("\\x{byte:02X}"));
        }
    }
    text
}

/// DuckDB keeps a BIT as one byte counting the unused leading bits, then the bits.
pub fn bit_text(raw: &[u8]) -> String {
    let Some((&padding, bytes)) = raw.split_first() else {
        return String::new();
    };
    bytes
        .iter()
        .flat_map(|byte| (0..8).rev().map(move |bit| (byte >> bit) & 1))
        .skip(usize::from(padding))
        .map(|bit| if bit == 1 { '1' } else { '0' })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{
        bignum_text, bit_text, date_text, decimal_text, float_text, interval_text, time_tz_text,
    };

    #[test]
    fn scalars_read_as_duckdb_writes_them() {
        assert_eq!(decimal_text(150, 2), "1.50");
        assert_eq!(decimal_text(-5, 2), "-0.05");
        assert_eq!(decimal_text(i128::MAX, 0), i128::MAX.to_string());
        assert_eq!(bit_text(&[4, 0b0000_0101]), "0101");
        assert_eq!(float_text(1e16), "1e+16");
        assert_eq!(float_text(1e-5), "1e-05");
        assert_eq!(float_text(-0.0), "0.0");
        assert_eq!(date_text(i32::MAX), "infinity");
        assert_eq!(interval_text(25, 0, 0), "2 years 1 month");
        assert_eq!(interval_text(0, 1, -3_600_000_000), "1 day -01:00:00");
        // 12:00:00+05:30, as DuckDB stores it.
        assert_eq!(time_tz_text(0x0A0E_EBB0_0000_93A7), "12:00:00+05:30");
        assert_eq!(bignum_text(&[127, 255, 253, 254, 255]).unwrap(), "-256");
        assert_eq!(bignum_text(&[128, 0, 1, 0]).unwrap(), "0");
    }
}
