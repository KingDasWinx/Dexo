use std::error::Error;
use std::fmt::Write as _;

use dexo_driver_api::{ColumnMeta, DbValue};
use fallible_iterator::FallibleIterator;
use postgres_protocol::types as wire;
use tokio_postgres::Row;
use tokio_postgres::types::{FromSql, Kind, Type};

pub fn column_meta(column: &tokio_postgres::Column) -> ColumnMeta {
    ColumnMeta {
        name: column.name().to_string(),
        type_name: column.type_().name().to_string(),
        nullable: true,
    }
}

/// The wire payload of one column, whatever its type. tokio-postgres asks the server for
/// binary format and every `FromSql` impl rejects the types it was not written for, so
/// asking it for a `String` and dropping the error -- which is what this file used to do
/// -- reported a decode failure as `Ok(None)`, indistinguishable from a SQL NULL. Every
/// type outside the handful listed below rendered as NULL, `numeric` and `timestamptz`
/// included. Taking the bytes ourselves keeps NULL meaning NULL.
struct Raw<'a>(&'a [u8]);

impl<'a> FromSql<'a> for Raw<'a> {
    fn from_sql(_: &Type, raw: &'a [u8]) -> Result<Self, Box<dyn Error + Sync + Send>> {
        Ok(Raw(raw))
    }

    fn accepts(_: &Type) -> bool {
        true
    }
}

pub fn decode_row(row: &Row) -> Vec<DbValue> {
    (0..row.len()).map(|idx| decode_at(row, idx)).collect()
}

pub fn decode_at(row: &Row, idx: usize) -> DbValue {
    match row.try_get::<_, Option<Raw<'_>>>(idx) {
        // `Raw` accepts every type, so `None` here is the server saying NULL.
        Ok(Some(Raw(raw))) => decode_value(row.columns()[idx].type_(), raw),
        _ => DbValue::Null,
    }
}

pub fn decode_value(ty: &Type, raw: &[u8]) -> DbValue {
    match ty.kind() {
        // A domain is its base type with a constraint bolted on; the wire format is the
        // base type's.
        Kind::Domain(inner) => return decode_value(inner, raw),
        // An enum is sent as its label, even in binary format.
        Kind::Enum(_) => return text(raw).unwrap_or_else(|| undecoded(ty, raw)),
        Kind::Array(inner) => {
            return array_text(inner, raw)
                .map(|text| native(ty, raw, text))
                .unwrap_or_else(|| undecoded(ty, raw));
        }
        Kind::Range(inner) => {
            return range_text(inner, raw)
                .map(|text| native(ty, raw, text))
                .unwrap_or_else(|| undecoded(ty, raw));
        }
        _ => {}
    }
    scalar(ty, raw).unwrap_or_else(|| undecoded(ty, raw))
}

fn scalar(ty: &Type, raw: &[u8]) -> Option<DbValue> {
    let value = match *ty {
        Type::BOOL => DbValue::Bool(wire::bool_from_sql(raw).ok()?),
        Type::INT2 => DbValue::I64(wire::int2_from_sql(raw).ok()?.into()),
        Type::INT4 => DbValue::I64(wire::int4_from_sql(raw).ok()?.into()),
        Type::INT8 => DbValue::I64(wire::int8_from_sql(raw).ok()?),
        Type::OID | Type::REGCLASS | Type::REGPROC | Type::REGTYPE => {
            DbValue::U64(wire::oid_from_sql(raw).ok()?.into())
        }
        Type::FLOAT4 => native(ty, raw, wire::float4_from_sql(raw).ok()?.to_string()),
        Type::FLOAT8 => native(ty, raw, wire::float8_from_sql(raw).ok()?.to_string()),
        Type::NUMERIC => DbValue::Decimal(numeric_text(raw)?),
        Type::MONEY => native(ty, raw, money_text(wire::int8_from_sql(raw).ok()?)),
        Type::CHAR => DbValue::Text((wire::char_from_sql(raw).ok()? as u8 as char).to_string()),
        Type::TEXT | Type::VARCHAR | Type::BPCHAR | Type::NAME | Type::UNKNOWN | Type::XML => {
            DbValue::Text(wire::text_from_sql(raw).ok()?.to_string())
        }
        Type::BYTEA => DbValue::Bytes(wire::bytea_from_sql(raw).to_vec()),
        Type::JSON => DbValue::Json(wire::text_from_sql(raw).ok()?.to_string()),
        // jsonb prefixes the document with a format version byte.
        Type::JSONB => match raw.split_first() {
            Some((1, rest)) => DbValue::Json(std::str::from_utf8(rest).ok()?.to_string()),
            _ => return None,
        },
        Type::UUID => DbValue::Text(uuid::Uuid::from_slice(raw).ok()?.to_string()),
        Type::DATE => native(ty, raw, date_text(wire::date_from_sql(raw).ok()?)),
        Type::TIME => native(ty, raw, time_text(wire::time_from_sql(raw).ok()?)),
        Type::TIMETZ => native(ty, raw, timetz_text(raw)?),
        Type::TIMESTAMP => native(
            ty,
            raw,
            timestamp_text(wire::timestamp_from_sql(raw).ok()?, false),
        ),
        Type::TIMESTAMPTZ => native(
            ty,
            raw,
            timestamp_text(wire::timestamp_from_sql(raw).ok()?, true),
        ),
        Type::INTERVAL => native(ty, raw, interval_text(raw)?),
        Type::INET | Type::CIDR => native(ty, raw, inet_text(ty, raw)?),
        Type::MACADDR => native(
            ty,
            raw,
            wire::macaddr_from_sql(raw)
                .ok()?
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<Vec<_>>()
                .join(":"),
        ),
        Type::BIT | Type::VARBIT => native(ty, raw, varbit_text(raw)?),
        Type::POINT => {
            let point = wire::point_from_sql(raw).ok()?;
            native(ty, raw, format!("({},{})", point.x(), point.y()))
        }
        Type::PG_LSN => {
            let lsn = wire::lsn_from_sql(raw).ok()?;
            native(ty, raw, format!("{:X}/{:X}", lsn >> 32, lsn as u32))
        }
        _ => return None,
    };
    Some(value)
}

fn text(raw: &[u8]) -> Option<DbValue> {
    Some(DbValue::Text(wire::text_from_sql(raw).ok()?.to_string()))
}

fn native(ty: &Type, raw: &[u8], text: String) -> DbValue {
    DbValue::Native {
        type_name: ty.name().to_string(),
        bytes: raw.to_vec(),
        text,
    }
}

/// A type whose binary form this driver cannot read yet. Hex is not useful, but it is
/// true, and it keeps NULL meaning NULL -- which is the whole point of this file.
fn undecoded(ty: &Type, raw: &[u8]) -> DbValue {
    let mut hex = String::with_capacity(2 + raw.len() * 2);
    hex.push_str("\\x");
    for byte in raw {
        let _ = write!(hex, "{byte:02x}");
    }
    native(ty, raw, hex)
}

/// The display form of a decoded value, for the types that nest others: arrays and
/// ranges hold element payloads, not text.
fn element_text(ty: &Type, raw: &[u8]) -> String {
    match decode_value(ty, raw) {
        DbValue::Null => "NULL".into(),
        DbValue::Bool(value) => value.to_string(),
        DbValue::I64(value) => value.to_string(),
        DbValue::U64(value) => value.to_string(),
        DbValue::Decimal(value) | DbValue::Text(value) | DbValue::Json(value) => value,
        DbValue::Bytes(bytes) => {
            let DbValue::Native { text, .. } = undecoded(ty, &bytes) else {
                unreachable!("undecoded is always Native")
            };
            text
        }
        DbValue::Native { text, .. } => text,
    }
}

fn array_text(inner: &Type, raw: &[u8]) -> Option<String> {
    let array = wire::array_from_sql(raw).ok()?;
    let dimensions: Vec<usize> = array
        .dimensions()
        .map(|dimension| Ok(dimension.len.max(0) as usize))
        .collect()
        .ok()?;
    let mut elements = Vec::new();
    let mut values = array.values();
    while let Some(element) = values.next().ok()? {
        elements.push(match element {
            None => "NULL".to_string(),
            Some(bytes) => quote_element(&element_text(inner, bytes)),
        });
    }
    Some(nest(&dimensions, &elements))
}

/// Elements arrive in storage order, with the shape kept separately -- so a
/// two-dimensional array is `{{1,2},{3,4}}` rather than four values in a row.
fn nest(dimensions: &[usize], elements: &[String]) -> String {
    match dimensions.split_first() {
        None | Some((_, [])) => format!("{{{}}}", elements.join(",")),
        Some((_, rest)) => {
            let stride = rest.iter().product::<usize>().max(1);
            let groups: Vec<String> = elements
                .chunks(stride)
                .map(|group| nest(rest, group))
                .collect();
            format!("{{{}}}", groups.join(","))
        }
    }
}

fn quote_element(text: &str) -> String {
    let needs_quotes = text.is_empty()
        || text.eq_ignore_ascii_case("null")
        || text
            .chars()
            .any(|c| matches!(c, '{' | '}' | ',' | '"' | '\\') || c.is_whitespace());
    if !needs_quotes {
        return text.to_string();
    }
    let escaped = text.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

fn range_text(inner: &Type, raw: &[u8]) -> Option<String> {
    use wire::{Range, RangeBound};

    let bound_text = |bound: &RangeBound<Option<&[u8]>>| match bound {
        RangeBound::Inclusive(value) | RangeBound::Exclusive(value) => value
            .map(|bytes| element_text(inner, bytes))
            .unwrap_or_default(),
        RangeBound::Unbounded => String::new(),
    };
    match wire::range_from_sql(raw).ok()? {
        Range::Empty => Some("empty".into()),
        Range::Nonempty(lower, upper) => {
            let open = match lower {
                RangeBound::Inclusive(_) => '[',
                _ => '(',
            };
            let close = match upper {
                RangeBound::Inclusive(_) => ']',
                _ => ')',
            };
            Some(format!(
                "{open}{},{}{close}",
                bound_text(&lower),
                bound_text(&upper)
            ))
        }
    }
}

fn inet_text(ty: &Type, raw: &[u8]) -> Option<String> {
    let inet = wire::inet_from_sql(raw).ok()?;
    let full = match inet.addr() {
        std::net::IpAddr::V4(_) => 32,
        std::net::IpAddr::V6(_) => 128,
    };
    if *ty == Type::INET && inet.netmask() == full {
        Some(inet.addr().to_string())
    } else {
        Some(format!("{}/{}", inet.addr(), inet.netmask()))
    }
}

fn varbit_text(raw: &[u8]) -> Option<String> {
    let varbit = wire::varbit_from_sql(raw).ok()?;
    Some(
        (0..varbit.len())
            .map(|bit| {
                let byte = varbit.bytes()[bit / 8];
                char::from(b'0' + ((byte >> (7 - bit % 8)) & 1))
            })
            .collect(),
    )
}

fn money_text(cents: i64) -> String {
    let sign = if cents < 0 { "-" } else { "" };
    let cents = cents.unsigned_abs();
    format!("{sign}{}.{:02}", cents / 100, cents % 100)
}

/// Postgres stores `numeric` as base-10000 digits with a separate scale, so it survives
/// values no float can hold. Rebuilding the decimal string keeps that guarantee -- going
/// through `f64` would not.
fn numeric_text(raw: &[u8]) -> Option<String> {
    const SIGN_NEG: u16 = 0x4000;
    const SIGN_NAN: u16 = 0xC000;
    const SIGN_PINF: u16 = 0xD000;
    const SIGN_NINF: u16 = 0xF000;

    let read_u16 = |at: usize| -> Option<u16> {
        Some(u16::from_be_bytes(raw.get(at..at + 2)?.try_into().ok()?))
    };
    let ndigits = read_u16(0)? as usize;
    let weight = read_u16(2)? as i16 as i32;
    let sign = read_u16(4)?;
    let dscale = read_u16(6)? as usize;
    match sign {
        SIGN_NAN => return Some("NaN".into()),
        SIGN_PINF => return Some("Infinity".into()),
        SIGN_NINF => return Some("-Infinity".into()),
        _ => {}
    }
    let digits: Vec<u16> = (0..ndigits)
        .map(|i| read_u16(8 + i * 2))
        .collect::<Option<_>>()?;

    let mut out = String::new();
    if sign == SIGN_NEG {
        out.push('-');
    }
    if weight < 0 {
        out.push('0');
    } else {
        for i in 0..=weight {
            let digit = digits.get(i as usize).copied().unwrap_or(0);
            if i == 0 {
                let _ = write!(out, "{digit}");
            } else {
                let _ = write!(out, "{digit:04}");
            }
        }
    }
    if dscale > 0 {
        out.push('.');
        let mut written = 0;
        let mut group = weight + 1;
        while written < dscale {
            let digit = if group >= 0 {
                digits.get(group as usize).copied().unwrap_or(0)
            } else {
                0
            };
            let rendered = format!("{digit:04}");
            let take = (dscale - written).min(4);
            out.push_str(&rendered[..take]);
            written += take;
            group += 1;
        }
    }
    Some(out)
}

/// Postgres counts from 2000-01-01, in days or microseconds. Turning that back into a
/// calendar is the only thing between the integer and a readable cell.
const PG_EPOCH_UNIX_DAYS: i64 = 10_957;

/// Days since the unix epoch to a civil date, by Howard Hinnant's `civil_from_days`.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let year = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if month <= 2 { year + 1 } else { year }, month, day)
}

fn ymd_text(unix_days: i64) -> String {
    let (year, month, day) = civil_from_days(unix_days);
    if year <= 0 {
        // Postgres has no year zero: 0 is 1 BC.
        format!("{:04}-{month:02}-{day:02} BC", 1 - year)
    } else {
        format!("{year:04}-{month:02}-{day:02}")
    }
}

fn date_text(days: i32) -> String {
    match days {
        i32::MAX => "infinity".into(),
        i32::MIN => "-infinity".into(),
        days => ymd_text(days as i64 + PG_EPOCH_UNIX_DAYS),
    }
}

fn clock_text(secs_of_day: i64, micros: i64) -> String {
    let mut out = format!(
        "{:02}:{:02}:{:02}",
        secs_of_day / 3600,
        (secs_of_day / 60) % 60,
        secs_of_day % 60
    );
    if micros > 0 {
        let fraction = format!("{micros:06}");
        let _ = write!(out, ".{}", fraction.trim_end_matches('0'));
    }
    out
}

fn time_text(micros: i64) -> String {
    clock_text(micros.div_euclid(1_000_000), micros.rem_euclid(1_000_000))
}

/// `timetz` is a time followed by the zone, in seconds *west* of UTC.
fn timetz_text(raw: &[u8]) -> Option<String> {
    let micros = i64::from_be_bytes(raw.get(0..8)?.try_into().ok()?);
    let west = i32::from_be_bytes(raw.get(8..12)?.try_into().ok()?);
    let offset = -west;
    let sign = if offset < 0 { '-' } else { '+' };
    let offset = offset.unsigned_abs();
    let mut out = time_text(micros);
    let _ = write!(out, "{sign}{:02}", offset / 3600);
    if offset % 3600 != 0 {
        let _ = write!(out, ":{:02}", (offset % 3600) / 60);
    }
    Some(out)
}

fn timestamp_text(micros: i64, zoned: bool) -> String {
    match micros {
        i64::MAX => return "infinity".into(),
        i64::MIN => return "-infinity".into(),
        _ => {}
    }
    let secs = micros.div_euclid(1_000_000);
    let fraction = micros.rem_euclid(1_000_000);
    let days = secs.div_euclid(86_400);
    let secs_of_day = secs.rem_euclid(86_400);
    let mut out = format!(
        "{} {}",
        ymd_text(days + PG_EPOCH_UNIX_DAYS),
        clock_text(secs_of_day, fraction)
    );
    if zoned {
        // The server sends UTC microseconds; rendering the zone it would have used for
        // the session would mean tracking the session's TimeZone setting.
        out.push_str("+00");
    }
    out
}

/// `interval` is three independent counts -- months, days, microseconds -- because none
/// of them converts into another without a calendar.
fn interval_text(raw: &[u8]) -> Option<String> {
    let micros = i64::from_be_bytes(raw.get(0..8)?.try_into().ok()?);
    let days = i32::from_be_bytes(raw.get(8..12)?.try_into().ok()?);
    let months = i32::from_be_bytes(raw.get(12..16)?.try_into().ok()?);

    let mut parts = Vec::new();
    // Postgres pluralizes on the signed count, so -1 day prints as "-1 days".
    let plural = |count: i32, unit: &str| {
        if count == 1 {
            format!("{count} {unit}")
        } else {
            format!("{count} {unit}s")
        }
    };
    if months / 12 != 0 {
        parts.push(plural(months / 12, "year"));
    }
    if months % 12 != 0 {
        parts.push(plural(months % 12, "mon"));
    }
    if days != 0 {
        parts.push(plural(days, "day"));
    }
    if micros != 0 || parts.is_empty() {
        let sign = if micros < 0 { "-" } else { "" };
        let micros = micros.abs();
        parts.push(format!(
            "{sign}{}",
            clock_text(micros / 1_000_000, micros % 1_000_000)
        ));
    }
    Some(parts.join(" "))
}

#[cfg(test)]
mod tests {
    use super::{decode_value, numeric_text};
    use dexo_driver_api::DbValue;
    use tokio_postgres::types::Type;

    fn text_of(value: &DbValue) -> String {
        match value {
            DbValue::Decimal(text) | DbValue::Text(text) | DbValue::Native { text, .. } => {
                text.clone()
            }
            other => format!("{other:?}"),
        }
    }

    /// The bug this file was rewritten for: the server sends a value, the driver cannot
    /// read the type, and the cell reads NULL -- indistinguishable from a row that really
    /// has no value there. Every type must decode to *something*, and only a NULL from
    /// the server may produce `DbValue::Null`.
    #[test]
    fn no_payload_ever_decodes_to_null() {
        // A timestamptz, a numeric, and a type with no decoder at all (tsvector).
        for (ty, raw) in [
            (Type::TIMESTAMPTZ, &[0, 2, 254, 95, 158, 39, 95, 185][..]),
            (Type::NUMERIC, &[0, 2, 0, 0, 0, 0, 0, 2, 0, 1, 9, 196][..]),
            (Type::TS_VECTOR, &[0, 0, 0, 1, 60, 62, 0, 0, 0][..]),
            (Type::INT4, &[0, 0, 0, 7][..]),
        ] {
            assert_ne!(
                decode_value(&ty, raw),
                DbValue::Null,
                "{ty} reported a value as NULL"
            );
        }
    }

    #[test]
    fn timestamps_render_the_way_postgres_prints_them() {
        // 2026-09-03 14:55:07.700144+00, the row that surfaced this.
        let raw = 841_762_507_700_144i64.to_be_bytes();
        assert_eq!(
            text_of(&decode_value(&Type::TIMESTAMPTZ, &raw)),
            "2026-09-03 14:55:07.700144+00"
        );
        // Before the 2000-01-01 epoch the counts go negative, where truncating division
        // would land a day off.
        let raw = (-1i64).to_be_bytes();
        assert_eq!(
            text_of(&decode_value(&Type::TIMESTAMP, &raw)),
            "1999-12-31 23:59:59.999999"
        );
        assert_eq!(
            text_of(&decode_value(&Type::DATE, &(-1i32).to_be_bytes())),
            "1999-12-31"
        );
        assert_eq!(
            text_of(&decode_value(&Type::DATE, &i32::MAX.to_be_bytes())),
            "infinity"
        );
    }

    /// `numeric` carries more precision than an f64, which is the point of the type.
    #[test]
    fn numeric_keeps_every_digit_and_its_scale() {
        let encode = |ndigits: i16, weight: i16, sign: u16, dscale: u16, digits: &[u16]| {
            let mut raw = Vec::new();
            raw.extend(ndigits.to_be_bytes());
            raw.extend(weight.to_be_bytes());
            raw.extend(sign.to_be_bytes());
            raw.extend(dscale.to_be_bytes());
            for digit in digits {
                raw.extend(digit.to_be_bytes());
            }
            raw
        };
        // 1.25
        assert_eq!(
            numeric_text(&encode(2, 0, 0, 2, &[1, 2500])).unwrap(),
            "1.25"
        );
        // 100.00 -- trailing zeros are part of the scale, not noise.
        assert_eq!(numeric_text(&encode(1, 0, 0, 2, &[100])).unwrap(), "100.00");
        // 0.000001, where the value starts two base-10000 groups past the point.
        assert_eq!(
            numeric_text(&encode(1, -2, 0, 6, &[100])).unwrap(),
            "0.000001"
        );
        // NaN carries no digits at all.
        assert_eq!(numeric_text(&encode(0, 0, 0xC000, 0, &[])).unwrap(), "NaN");
    }
}
