use std::collections::{BTreeSet, HashMap};
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
    decode_row_named(row, &RegNames::default())
}

/// A row whose reg* values show the names `names` holds for them.
pub fn decode_row_named(row: &Row, names: &RegNames) -> Vec<DbValue> {
    (0..row.len())
        .map(|idx| match row.try_get::<_, Option<Raw<'_>>>(idx) {
            Ok(Some(Raw(raw))) => decode_with(row.columns()[idx].type_(), raw, names),
            _ => DbValue::Null,
        })
        .collect()
}

pub fn decode_at(row: &Row, idx: usize) -> DbValue {
    match row.try_get::<_, Option<Raw<'_>>>(idx) {
        // `Raw` accepts every type, so `None` here is the server saying NULL.
        Ok(Some(Raw(raw))) => decode_value(row.columns()[idx].type_(), raw),
        _ => DbValue::Null,
    }
}

/// What psql prints for reg* values -- `pg_class` for a regclass, `integer` for a
/// regtype -- by the type's OID and the value's. Their binary form is the OID alone, and
/// only the server can name it: as it does for psql, against this session's search_path.
#[derive(Default)]
pub struct RegNames(HashMap<(u32, u32), String>);

fn is_reg(ty: &Type) -> bool {
    matches!(
        *ty,
        Type::REGCLASS
            | Type::REGTYPE
            | Type::REGPROC
            | Type::REGPROCEDURE
            | Type::REGOPER
            | Type::REGOPERATOR
            | Type::REGNAMESPACE
            | Type::REGROLE
            | Type::REGCONFIG
            | Type::REGDICTIONARY
            | Type::REGCOLLATION
    )
}

/// Whether a column holds reg* values, alone, in an array or under a domain.
fn holds_reg(ty: &Type) -> bool {
    match ty.kind() {
        Kind::Domain(inner) | Kind::Array(inner) => holds_reg(inner),
        _ => is_reg(ty),
    }
}

pub fn needs_names(columns: &[tokio_postgres::Column]) -> bool {
    columns.iter().any(|column| holds_reg(column.type_()))
}

/// The names of the reg* values in `rows`, one query per reg type for the OIDs the rows
/// hold. The rows must all have arrived: a query sent while a result still streams on
/// the connection waits behind it. A lookup that fails leaves the OIDs showing.
pub async fn reg_names(client: &tokio_postgres::Client, rows: &[Row]) -> RegNames {
    fn collect(ty: &Type, raw: &[u8], wanted: &mut HashMap<Type, BTreeSet<u32>>) {
        match ty.kind() {
            Kind::Domain(inner) => collect(inner, raw, wanted),
            Kind::Array(inner) => {
                let Ok(array) = wire::array_from_sql(raw) else {
                    return;
                };
                let mut values = array.values();
                while let Ok(Some(value)) = values.next() {
                    if let Some(bytes) = value {
                        collect(inner, bytes, wanted);
                    }
                }
            }
            _ if is_reg(ty) => {
                if let Ok(oid) = wire::oid_from_sql(raw) {
                    wanted.entry(ty.clone()).or_default().insert(oid);
                }
            }
            _ => {}
        }
    }
    let mut wanted = HashMap::new();
    for row in rows {
        for (idx, column) in row.columns().iter().enumerate() {
            if holds_reg(column.type_())
                && let Ok(Some(Raw(raw))) = row.try_get::<_, Option<Raw<'_>>>(idx)
            {
                collect(column.type_(), raw, &mut wanted);
            }
        }
    }
    let mut names = RegNames::default();
    for (ty, oids) in wanted {
        let oids: Vec<u32> = oids.into_iter().collect();
        let sql = format!(
            "SELECT o, o::{}::text FROM unnest($1::oid[]) AS o",
            ty.name()
        );
        if let Ok(found) = client.query(&sql, &[&oids]).await {
            for row in found {
                names.0.insert((ty.oid(), row.get(0)), row.get(1));
            }
        }
    }
    names
}

pub fn decode_value(ty: &Type, raw: &[u8]) -> DbValue {
    decode_with(ty, raw, &RegNames::default())
}

fn decode_with(ty: &Type, raw: &[u8], names: &RegNames) -> DbValue {
    match ty.kind() {
        // A domain is its base type with a constraint bolted on; the wire format is the
        // base type's.
        Kind::Domain(inner) => return decode_with(inner, raw, names),
        // An enum is sent as its label, even in binary format.
        Kind::Enum(_) => return text(raw).unwrap_or_else(|| undecoded(ty, raw)),
        Kind::Array(inner) => {
            return array_text(inner, raw, names)
                .map(|text| native(ty, raw, text))
                .unwrap_or_else(|| undecoded(ty, raw));
        }
        Kind::Range(inner) => {
            return range_text(inner, raw)
                .map(|text| native(ty, raw, text))
                .unwrap_or_else(|| undecoded(ty, raw));
        }
        Kind::Multirange(inner) => {
            return multirange_text(inner, raw)
                .map(|text| native(ty, raw, text))
                .unwrap_or_else(|| undecoded(ty, raw));
        }
        _ => {}
    }
    scalar(ty, raw)
        .or_else(|| other(ty, raw, names))
        .unwrap_or_else(|| undecoded(ty, raw))
}

fn scalar(ty: &Type, raw: &[u8]) -> Option<DbValue> {
    let value = match *ty {
        Type::BOOL => DbValue::Bool(wire::bool_from_sql(raw).ok()?),
        Type::INT2 => DbValue::I64(wire::int2_from_sql(raw).ok()?.into()),
        Type::INT4 => DbValue::I64(wire::int4_from_sql(raw).ok()?.into()),
        Type::INT8 => DbValue::I64(wire::int8_from_sql(raw).ok()?),
        Type::OID => DbValue::U64(wire::oid_from_sql(raw).ok()?.into()),
        Type::FLOAT4 => native(ty, raw, float4_text(wire::float4_from_sql(raw).ok()?)),
        Type::FLOAT8 => native(ty, raw, float8_text(wire::float8_from_sql(raw).ok()?)),
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
            native(
                ty,
                raw,
                format!("({},{})", float8_text(point.x()), float8_text(point.y())),
            )
        }
        Type::PG_LSN => {
            let lsn = wire::lsn_from_sql(raw).ok()?;
            native(ty, raw, format!("{:X}/{:X}", lsn >> 32, lsn as u32))
        }
        _ => return None,
    };
    Some(value)
}

/// Types outside the common ones, which used to show as hex: the built-ins made of
/// plain numbers, known by their OID, and extensions' -- `citext` is sent as its text,
/// `ltree` and its queries as a version byte and their text, pgvector's `vector` as its
/// floats -- known by name, and only as the plain types they are: a composite type of
/// the user's called `vector` read as an empty one.
fn other(ty: &Type, raw: &[u8], names: &RegNames) -> Option<DbValue> {
    let utf8 = |bytes: &[u8]| std::str::from_utf8(bytes).ok().map(str::to_string);
    let versioned = || match raw.split_first() {
        Some((1, rest)) => utf8(rest),
        _ => None,
    };
    let u32_at = |at: usize| Some(u32::from_be_bytes(raw.get(at..at + 4)?.try_into().ok()?));
    let f8_at = |at: usize| {
        Some(float8_text(f64::from_be_bytes(
            raw.get(at..at + 8)?.try_into().ok()?,
        )))
    };
    let point_at = |at: usize| Some(format!("({},{})", f8_at(at)?, f8_at(at + 8)?));
    let points = |from: usize, count: usize| {
        (0..count)
            .map(|index| point_at(from + index * 16))
            .collect::<Option<Vec<_>>>()
            .map(|points| points.join(","))
    };
    let text = match *ty {
        Type::JSONPATH => versioned()?,
        // Numbers, but kept as the type they are: read as a plain integer, a value went
        // back as a bigint, and the grid's delete, which compares every column, failed
        // with "operator does not exist: xid = bigint".
        Type::XID | Type::CID => u32_at(0)?.to_string(),
        // The name the server gave it, or its OID where none was asked for.
        _ if is_reg(ty) => {
            let oid = u32_at(0)?;
            match names.0.get(&(ty.oid(), oid)) {
                Some(name) => name.clone(),
                None => oid.to_string(),
            }
        }
        Type::XID8 => u64::from_be_bytes(raw.try_into().ok()?).to_string(),
        Type::TID => format!(
            "({},{})",
            u32_at(0)?,
            u16::from_be_bytes(raw.get(4..6)?.try_into().ok()?)
        ),
        Type::MACADDR8 if raw.len() == 8 => raw
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<Vec<_>>()
            .join(":"),
        Type::LSEG => format!("[{},{}]", point_at(0)?, point_at(16)?),
        Type::BOX => format!("{},{}", point_at(0)?, point_at(16)?),
        Type::LINE => format!("{{{},{},{}}}", f8_at(0)?, f8_at(8)?, f8_at(16)?),
        Type::CIRCLE => format!("<{},{}>", point_at(0)?, f8_at(16)?),
        Type::PATH => {
            let closed = *raw.first()? == 1;
            let count = usize::try_from(u32_at(1)?).ok()?;
            let points = points(5, count)?;
            if closed {
                format!("({points})")
            } else {
                format!("[{points}]")
            }
        }
        Type::POLYGON => format!("({})", points(4, usize::try_from(u32_at(0)?).ok()?)?),
        Type::TS_VECTOR => tsvector_text(raw)?,
        Type::TSQUERY => tsquery_text(raw)?,
        // Its binary form is its text.
        Type::REFCURSOR => utf8(raw)?,
        Type::PG_SNAPSHOT | Type::TXID_SNAPSHOT => snapshot_text(raw)?,
        _ if matches!(ty.kind(), Kind::Simple) => match ty.name() {
            "citext" => return Some(DbValue::Text(utf8(raw)?)),
            "ltree" | "lquery" | "ltxtquery" => versioned()?,
            "vector" => {
                let dimensions = usize::from(u16::from_be_bytes(raw.get(0..2)?.try_into().ok()?));
                let values = (0..dimensions)
                    .map(|index| {
                        let at = 4 + index * 4;
                        Some(float4_text(f32::from_be_bytes(
                            raw.get(at..at + 4)?.try_into().ok()?,
                        )))
                    })
                    .collect::<Option<Vec<_>>>()?;
                format!("[{}]", values.join(","))
            }
            // pgvector's half-precision vector: its halves are printed as the float4s
            // they widen to.
            "halfvec" => {
                let dimensions = usize::from(u16::from_be_bytes(raw.get(0..2)?.try_into().ok()?));
                let values = (0..dimensions)
                    .map(|index| {
                        let at = 4 + index * 2;
                        let half = u16::from_be_bytes(raw.get(at..at + 2)?.try_into().ok()?);
                        Some(float4_text(half_to_f32(half)))
                    })
                    .collect::<Option<Vec<_>>>()?;
                format!("[{}]", values.join(","))
            }
            "sparsevec" => sparsevec_text(raw)?,
            "hstore" => hstore_text(raw)?,
            _ => return None,
        },
        _ => return None,
    };
    Some(native(ty, raw, text))
}

/// A float8 as Postgres 12 on prints it: the shortest digits that read back as the
/// same value, written out from 1e-4 up to 1e15 and in exponent form outside it, the
/// way `%g` places it -- `1e+20`, `1e-07` -- and the non-numbers by name. Rust's own
/// form wrote 1e300 as 301 digits and infinity as `inf`.
fn float8_text(value: f64) -> String {
    float_text(&format!("{value}"), &format!("{value:e}"), 15)
}

/// A float4 the same way, written out up to 1e6.
fn float4_text(value: f32) -> String {
    float_text(&format!("{value}"), &format!("{value:e}"), 6)
}

/// `plain` and `scientific` are the same shortest digits, written out and as `1.5e20`.
fn float_text(plain: &str, scientific: &str, written_below: i32) -> String {
    match plain {
        "NaN" => return "NaN".into(),
        "inf" => return "Infinity".into(),
        "-inf" => return "-Infinity".into(),
        _ => {}
    }
    let Some((digits, exponent)) = scientific.split_once('e') else {
        return plain.to_string();
    };
    match exponent.parse::<i32>() {
        Ok(exponent) if !(-4..written_below).contains(&exponent) => {
            let sign = if exponent < 0 { '-' } else { '+' };
            format!("{digits}e{sign}{:02}", exponent.unsigned_abs())
        }
        _ => plain.to_string(),
    }
}

/// `'fat':2,4A 'cat':3`: each lexeme quoted, with its positions and their weights.
fn tsvector_text(raw: &[u8]) -> Option<String> {
    let count = u32::from_be_bytes(raw.get(0..4)?.try_into().ok()?);
    let mut at = 4;
    let mut lexemes = Vec::new();
    for _ in 0..count {
        let end = at + raw.get(at..)?.iter().position(|byte| *byte == 0)?;
        let word = std::str::from_utf8(&raw[at..end]).ok()?;
        at = end + 1;
        let positions = u16::from_be_bytes(raw.get(at..at + 2)?.try_into().ok()?);
        at += 2;
        let mut lexeme = format!("'{}'", word.replace('\\', "\\\\").replace('\'', "''"));
        for index in 0..positions {
            let entry = u16::from_be_bytes(raw.get(at..at + 2)?.try_into().ok()?);
            at += 2;
            lexeme.push(if index == 0 { ':' } else { ',' });
            let _ = write!(lexeme, "{}", entry & 0x3fff);
            match entry >> 14 {
                3 => lexeme.push('A'),
                2 => lexeme.push('B'),
                1 => lexeme.push('C'),
                _ => {}
            }
        }
        lexemes.push(lexeme);
    }
    Some(lexemes.join(" "))
}

/// `'fat' & ( 'rat' | !'cat' ) <-> 'a':*B`, from the items in the order the server keeps
/// them: an operator, then its right operand, then its left. Parentheses where the
/// operators' priority asks for them, as the server's own output puts them.
fn tsquery_text(raw: &[u8]) -> Option<String> {
    enum Item {
        Operand(String),
        Not,
        /// The operator, its priority, and for a phrase that it is one.
        Binary(String, i32, bool),
    }
    fn infix(items: &[Item], at: &mut usize, parent: i32, right_of_phrase: bool) -> Option<String> {
        let item = items.get(*at)?;
        *at += 1;
        match item {
            Item::Operand(text) => Some(text.clone()),
            // NOT binds tightest, so it never needs parentheses of its own.
            Item::Not => Some(format!("!{}", infix(items, at, 4, false)?)),
            Item::Binary(symbol, priority, phrase) => {
                let right = infix(items, at, *priority, *phrase)?;
                let left = infix(items, at, *priority, false)?;
                Some(if *priority < parent || (*phrase && right_of_phrase) {
                    format!("( {left} {symbol} {right} )")
                } else {
                    format!("{left} {symbol} {right}")
                })
            }
        }
    }
    let count = u32::from_be_bytes(raw.get(0..4)?.try_into().ok()?);
    let mut at = 4;
    let mut items = Vec::new();
    for _ in 0..count {
        let item = match raw.get(at..at + 2)? {
            [1, weight] => {
                let weight = *weight;
                let prefix = *raw.get(at + 2)? != 0;
                at += 3;
                let end = at + raw.get(at..)?.iter().position(|byte| *byte == 0)?;
                let word = std::str::from_utf8(&raw[at..end]).ok()?;
                at = end + 1;
                let mut operand = format!("'{}'", word.replace('\\', "\\\\").replace('\'', "''"));
                if weight != 0 || prefix {
                    operand.push(':');
                    if prefix {
                        operand.push('*');
                    }
                    for (bit, letter) in [(8, 'A'), (4, 'B'), (2, 'C'), (1, 'D')] {
                        if weight & bit != 0 {
                            operand.push(letter);
                        }
                    }
                }
                Item::Operand(operand)
            }
            [2, operator] => {
                let operator = *operator;
                at += 2;
                match operator {
                    1 => Item::Not,
                    2 => Item::Binary("&".into(), 2, false),
                    3 => Item::Binary("|".into(), 1, false),
                    4 => {
                        let distance = i16::from_be_bytes(raw.get(at..at + 2)?.try_into().ok()?);
                        at += 2;
                        let symbol = if distance == 1 {
                            "<->".to_string()
                        } else {
                            format!("<{distance}>")
                        };
                        Item::Binary(symbol, 3, true)
                    }
                    _ => return None,
                }
            }
            _ => return None,
        };
        items.push(item);
    }
    if items.is_empty() {
        return Some(String::new());
    }
    infix(&items, &mut 0, -1, false)
}

/// `10:20:12,15`: the oldest transaction still running, the first not yet started, and
/// the ones between them still in progress.
fn snapshot_text(raw: &[u8]) -> Option<String> {
    let u64_at = |at: usize| Some(u64::from_be_bytes(raw.get(at..at + 8)?.try_into().ok()?));
    let running = u32::from_be_bytes(raw.get(0..4)?.try_into().ok()?);
    let xips = (0..running as usize)
        .map(|index| u64_at(20 + index * 8).map(|xid| xid.to_string()))
        .collect::<Option<Vec<_>>>()?;
    Some(format!("{}:{}:{}", u64_at(4)?, u64_at(12)?, xips.join(",")))
}

/// `"k"=>"v", "n"=>NULL`, a quote or a backslash in either escaped.
fn hstore_text(raw: &[u8]) -> Option<String> {
    let quoted = |bytes: &[u8]| {
        let text = std::str::from_utf8(bytes).ok()?;
        Some(format!(
            "\"{}\"",
            text.replace('\\', "\\\\").replace('"', "\\\"")
        ))
    };
    let count = u32::from_be_bytes(raw.get(0..4)?.try_into().ok()?);
    let mut at = 4;
    let mut pairs = Vec::new();
    for _ in 0..count {
        let mut next = || {
            let len = i32::from_be_bytes(raw.get(at..at + 4)?.try_into().ok()?);
            at += 4;
            let Ok(len) = usize::try_from(len) else {
                return Some(None);
            };
            let bytes = raw.get(at..at + len)?;
            at += len;
            Some(Some(bytes))
        };
        let key = quoted(next()??)?;
        let value = match next()? {
            Some(bytes) => quoted(bytes)?,
            None => "NULL".into(),
        };
        pairs.push(format!("{key}=>{value}"));
    }
    Some(pairs.join(", "))
}

/// pgvector's `{1:0.5,3:2}/5`: the non-zero elements, counted from one, and the length.
fn sparsevec_text(raw: &[u8]) -> Option<String> {
    let i32_at = |at: usize| Some(i32::from_be_bytes(raw.get(at..at + 4)?.try_into().ok()?));
    let dimensions = i32_at(0)?;
    let stored = usize::try_from(i32_at(4)?).ok()?;
    let values_at = 12 + stored * 4;
    let elements = (0..stored)
        .map(|index| {
            let position = i32_at(12 + index * 4)?;
            let at = values_at + index * 4;
            let value = f32::from_be_bytes(raw.get(at..at + 4)?.try_into().ok()?);
            Some(format!("{}:{}", position + 1, float4_text(value)))
        })
        .collect::<Option<Vec<_>>>()?;
    Some(format!("{{{}}}/{dimensions}", elements.join(",")))
}

/// An IEEE half-precision float, exactly, as the float4 it widens to.
fn half_to_f32(bits: u16) -> f32 {
    let sign = if bits & 0x8000 == 0 { 1.0 } else { -1.0 };
    let exponent = i32::from((bits >> 10) & 0x1f);
    let fraction = f32::from(bits & 0x3ff);
    match exponent {
        0 => sign * fraction * 2f32.powi(-24),
        31 if fraction == 0.0 => sign * f32::INFINITY,
        31 => f32::NAN,
        _ => sign * (1.0 + fraction / 1024.0) * 2f32.powi(exponent - 15),
    }
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
fn element_text(ty: &Type, raw: &[u8], names: &RegNames) -> String {
    match decode_with(ty, raw, names) {
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

fn array_text(inner: &Type, raw: &[u8], names: &RegNames) -> Option<String> {
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
            Some(bytes) => quote_element(&element_text(inner, bytes, names)),
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
            .map(|bytes| element_text(inner, bytes, &RegNames::default()))
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

/// `{[1,3),[5,7)}`: a count, then each range with its length before it.
fn multirange_text(inner: &Type, raw: &[u8]) -> Option<String> {
    let count = u32::from_be_bytes(raw.get(0..4)?.try_into().ok()?);
    let mut at = 4;
    let mut ranges = Vec::new();
    for _ in 0..count {
        let len =
            usize::try_from(u32::from_be_bytes(raw.get(at..at + 4)?.try_into().ok()?)).ok()?;
        at += 4;
        ranges.push(range_text(inner, raw.get(at..at + len)?)?);
        at += len;
    }
    Some(format!("{{{}}}", ranges.join(",")))
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

    /// pgvector's `vector` is not in the image the live tests use; its wire form is a
    /// dimension count, an unused word, and the floats.
    #[test]
    fn a_pgvector_reads_as_its_floats() {
        let ty = Type::new(
            "vector".into(),
            99_999,
            tokio_postgres::types::Kind::Simple,
            "public".into(),
        );
        let mut raw = vec![0, 3, 0, 0];
        for value in [1.0f32, 0.5, -2.0] {
            raw.extend(value.to_be_bytes());
        }
        assert_eq!(text_of(&decode_value(&ty, &raw)), "[1,0.5,-2]");
    }

    fn named(name: &str) -> Type {
        Type::new(
            name.into(),
            99_997,
            tokio_postgres::types::Kind::Simple,
            "public".into(),
        )
    }

    /// pgvector's half and sparse vectors are not in the image the live tests use.
    #[test]
    fn pgvector_half_and_sparse_vectors_read_as_their_values() {
        let mut half = vec![0, 4, 0, 0];
        for bits in [0x3C00u16, 0x3800, 0xC000, 0x2E66] {
            half.extend(bits.to_be_bytes());
        }
        assert_eq!(
            text_of(&decode_value(&named("halfvec"), &half)),
            "[1,0.5,-2,0.099975586]"
        );
        let mut sparse = Vec::new();
        for word in [5i32, 2, 0, 0, 2] {
            sparse.extend(word.to_be_bytes());
        }
        for value in [1.5f32, -2.0] {
            sparse.extend(value.to_be_bytes());
        }
        assert_eq!(
            text_of(&decode_value(&named("sparsevec"), &sparse)),
            "{1:1.5,3:-2}/5"
        );
    }

    #[test]
    fn hstore_tsquery_snapshots_and_cursors_read_as_postgres_prints_them() {
        let mut hstore = vec![0, 0, 0, 2];
        for (len, bytes) in [(1i32, &b"a"[..]), (1, b"1"), (3, b"b\"q"), (-1, b"")] {
            hstore.extend(len.to_be_bytes());
            hstore.extend(bytes);
        }
        assert_eq!(
            text_of(&decode_value(&named("hstore"), &hstore)),
            r#""a"=>"1", "b\"q"=>NULL"#
        );
        // 'fat' & ( 'rat' | 'cat' ): each operator before its right operand, then its left.
        let mut query = vec![0, 0, 0, 5, 2, 2, 2, 3];
        for word in ["cat", "rat", "fat"] {
            query.extend([1, 0, 0]);
            query.extend(word.as_bytes());
            query.push(0);
        }
        assert_eq!(
            text_of(&decode_value(&Type::TSQUERY, &query)),
            "'fat' & ( 'rat' | 'cat' )"
        );
        let mut snapshot = 2u32.to_be_bytes().to_vec();
        for xid in [10u64, 20, 12, 15] {
            snapshot.extend(xid.to_be_bytes());
        }
        assert_eq!(
            text_of(&decode_value(&Type::PG_SNAPSHOT, &snapshot)),
            "10:20:12,15"
        );
        assert_eq!(text_of(&decode_value(&Type::REFCURSOR, b"cur")), "cur");
    }

    /// Floats read as psql prints them, in every type made of them.
    #[test]
    fn floats_read_as_postgres_prints_them() {
        use super::{float4_text, float8_text};
        for (value, text) in [
            (1e300, "1e+300"),
            (1e-7, "1e-07"),
            (1e20, "1e+20"),
            (1e15, "1e+15"),
            (1e14, "100000000000000"),
            (1.5e-5, "1.5e-05"),
            (0.0001, "0.0001"),
            (0.1, "0.1"),
            (-2.5, "-2.5"),
            (-0.0, "-0"),
            (1.2345678901234567e19, "1.2345678901234567e+19"),
            (f64::INFINITY, "Infinity"),
            (f64::NEG_INFINITY, "-Infinity"),
            (f64::NAN, "NaN"),
        ] {
            assert_eq!(float8_text(value), text);
        }
        for (value, text) in [
            (1e6f32, "1e+06"),
            (100_000.0, "100000"),
            (1_234_567.0, "1.234567e+06"),
            (0.1, "0.1"),
            (f32::INFINITY, "Infinity"),
        ] {
            assert_eq!(float4_text(value), text);
        }
        let mut point = Vec::new();
        point.extend(1e300f64.to_be_bytes());
        point.extend(1e-7f64.to_be_bytes());
        assert_eq!(
            text_of(&decode_value(&Type::POINT, &point)),
            "(1e+300,1e-07)"
        );
    }

    /// An extension's type is known by name only as the plain type it is: a composite
    /// of the user's called `vector (a int, b int)` read as an empty vector, `[]`.
    #[test]
    fn a_composite_named_like_an_extension_type_is_not_read_as_one() {
        use tokio_postgres::types::{Field, Kind};
        let ty = Type::new(
            "vector".into(),
            99_998,
            Kind::Composite(vec![
                Field::new("a".into(), Type::INT4),
                Field::new("b".into(), Type::INT4),
            ]),
            "public".into(),
        );
        // Two fields: a = 1, b = 2.
        let raw = [
            0, 0, 0, 2, 0, 0, 0, 23, 0, 0, 0, 4, 0, 0, 0, 1, 0, 0, 0, 23, 0, 0, 0, 4, 0, 0, 0, 2,
        ];
        assert_ne!(text_of(&decode_value(&ty, &raw)), "[]");
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
