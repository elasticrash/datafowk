use std::fs;
use std::io::Write;

use chrono::{DateTime, Datelike, NaiveDate, NaiveDateTime, NaiveTime, Timelike, Utc};
use mysql::Value;
use postgres::{
    types::{FromSql, ToSql, Type},
    Row as PostgresRow,
};

use crate::{
    config::ConnectionProperties,
    models::{DataValue, Rules},
    transforms::{apply_transform, geometry::parse_ewkb, is_row_transform},
};

const DUPLICATE_LOG_PATH: &str = "datafowk-skipped-duplicates.log";

/// Raw EWKB bytes received from PostgreSQL for geometry columns.
struct WkbBytes(Vec<u8>);

impl<'a> FromSql<'a> for WkbBytes {
    fn from_sql(_ty: &Type, raw: &'a [u8]) -> Result<Self, Box<dyn std::error::Error + Sync + Send>> {
        Ok(WkbBytes(raw.to_vec()))
    }

    fn accepts(ty: &Type) -> bool {
        ty.name() == "geometry"
    }
}

pub(super) type PgParam = Box<dyn ToSql + Sync>;

pub(super) fn transform_values(
    rule: &Rules,
    source_properties: &ConnectionProperties,
    mut values: Vec<DataValue>,
) -> Result<Vec<DataValue>, String> {
    if values.len() != rule.source_fields.len() {
        return Err(format!(
            "source query for tables `{:?}` returned {} columns but the rule expects {}",
            rule.source_tables,
            values.len(),
            rule.source_fields.len()
        ));
    }

    for transform in &rule.function_chain {
        if is_row_transform(transform) {
            continue;
        }
        for value in &mut values {
            apply_transform(value, transform, source_properties.kind)?;
        }
    }

    Ok(values)
}

fn data_value_to_log_string(value: &DataValue) -> String {
    match value {
        DataValue::Null => String::from("null"),
        DataValue::String(text) => text.clone(),
        DataValue::I64(value) => value.to_string(),
        DataValue::U64(value) => value.to_string(),
        DataValue::F64(value) => value.to_string(),
        DataValue::Bool(value) => value.to_string(),
        DataValue::Bytes(bytes) => format!("{bytes:?}"),
        DataValue::Date(value) => value.to_string(),
        DataValue::Time(value) => value.to_string(),
        DataValue::DateTime(value) => value.to_string(),
        DataValue::Geometry(polygons) => {
            format!("<geometry: {} polygon(s)>", polygons.len())
        }
    }
}

pub(super) fn append_duplicate_log(
    rule: &Rules,
    row: &[DataValue],
    unique_indexes: &[usize],
) -> Result<(), String> {
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(DUPLICATE_LOG_PATH)
        .map_err(|error| {
            format!(
                "failed to open duplicate log `{}`: {error}",
                DUPLICATE_LOG_PATH
            )
        })?;

    let unique_values = unique_indexes
        .iter()
        .map(|index| {
            format!(
                "{}={}",
                rule.destination_fields[*index],
                data_value_to_log_string(&row[*index])
            )
        })
        .collect::<Vec<_>>()
        .join(", ");

    let full_row = rule
        .destination_fields
        .iter()
        .zip(row.iter())
        .map(|(field, value)| format!("{field}={}", data_value_to_log_string(value)))
        .collect::<Vec<_>>()
        .join(", ");

    writeln!(
        file,
        "{} table={} unique=[{}] row=[{}]",
        Utc::now().to_rfc3339(),
        rule.destination_table,
        unique_values,
        full_row
    )
    .map_err(|error| {
        format!(
            "failed to write duplicate log `{}`: {error}",
            DUPLICATE_LOG_PATH
        )
    })
}

pub(super) fn mysql_value_to_data_value(value: Value) -> Result<DataValue, String> {
    match value {
        Value::NULL => Ok(DataValue::Null),
        Value::Bytes(bytes) => match String::from_utf8(bytes.clone()) {
            Ok(text) => Ok(DataValue::String(text)),
            Err(_) => Ok(DataValue::Bytes(bytes)),
        },
        Value::Int(value) => Ok(DataValue::I64(value)),
        Value::UInt(value) => Ok(DataValue::U64(value)),
        Value::Float(value) => Ok(DataValue::F64(value as f64)),
        Value::Double(value) => Ok(DataValue::F64(value)),
        Value::Date(year, month, day, hour, minute, second, micros) => {
            if hour == 0 && minute == 0 && second == 0 && micros == 0 {
                NaiveDate::from_ymd_opt(year as i32, month as u32, day as u32)
                    .map(DataValue::Date)
                    .ok_or_else(|| String::from("invalid MySQL date value"))
            } else {
                let date = NaiveDate::from_ymd_opt(year as i32, month as u32, day as u32)
                    .ok_or_else(|| String::from("invalid MySQL date value"))?;
                let time = NaiveTime::from_hms_micro_opt(
                    hour as u32,
                    minute as u32,
                    second as u32,
                    micros,
                )
                .ok_or_else(|| String::from("invalid MySQL datetime value"))?;
                Ok(DataValue::DateTime(NaiveDateTime::new(date, time)))
            }
        }
        Value::Time(negative, days, hours, minutes, seconds, micros) => {
            if negative || days > 0 {
                Err(String::from(
                    "MySQL TIME values with negative or multi-day durations are not supported yet",
                ))
            } else {
                NaiveTime::from_hms_micro_opt(hours as u32, minutes as u32, seconds as u32, micros)
                    .map(DataValue::Time)
                    .ok_or_else(|| String::from("invalid MySQL time value"))
            }
        }
    }
}

pub(super) fn postgres_row_to_data_values(row: &PostgresRow) -> Result<Vec<DataValue>, String> {
    row.columns()
        .iter()
        .enumerate()
        .map(|(index, column)| postgres_cell_to_data_value(row, index, column.type_()))
        .collect()
}

fn postgres_cell_to_data_value(
    row: &PostgresRow,
    index: usize,
    ty: &Type,
) -> Result<DataValue, String> {
    match *ty {
        Type::BOOL => Ok(row
            .try_get::<_, Option<bool>>(index)
            .map_err(|error| error.to_string())?
            .map(DataValue::Bool)
            .unwrap_or(DataValue::Null)),
        Type::INT2 => Ok(row
            .try_get::<_, Option<i16>>(index)
            .map_err(|error| error.to_string())?
            .map(|value| DataValue::I64(value as i64))
            .unwrap_or(DataValue::Null)),
        Type::INT4 | Type::OID => Ok(row
            .try_get::<_, Option<i32>>(index)
            .map_err(|error| error.to_string())?
            .map(|value| DataValue::I64(value as i64))
            .unwrap_or(DataValue::Null)),
        Type::INT8 => Ok(row
            .try_get::<_, Option<i64>>(index)
            .map_err(|error| error.to_string())?
            .map(DataValue::I64)
            .unwrap_or(DataValue::Null)),
        Type::FLOAT4 => Ok(row
            .try_get::<_, Option<f32>>(index)
            .map_err(|error| error.to_string())?
            .map(|value| DataValue::F64(value as f64))
            .unwrap_or(DataValue::Null)),
        Type::FLOAT8 => Ok(row
            .try_get::<_, Option<f64>>(index)
            .map_err(|error| error.to_string())?
            .map(DataValue::F64)
            .unwrap_or(DataValue::Null)),
        Type::TEXT | Type::VARCHAR | Type::BPCHAR | Type::NAME => Ok(row
            .try_get::<_, Option<String>>(index)
            .map_err(|error| error.to_string())?
            .map(DataValue::String)
            .unwrap_or(DataValue::Null)),
        Type::BYTEA => Ok(row
            .try_get::<_, Option<Vec<u8>>>(index)
            .map_err(|error| error.to_string())?
            .map(DataValue::Bytes)
            .unwrap_or(DataValue::Null)),
        Type::DATE => Ok(row
            .try_get::<_, Option<NaiveDate>>(index)
            .map_err(|error| error.to_string())?
            .map(DataValue::Date)
            .unwrap_or(DataValue::Null)),
        Type::TIME => Ok(row
            .try_get::<_, Option<NaiveTime>>(index)
            .map_err(|error| error.to_string())?
            .map(DataValue::Time)
            .unwrap_or(DataValue::Null)),
        Type::TIMESTAMP => Ok(row
            .try_get::<_, Option<NaiveDateTime>>(index)
            .map_err(|error| error.to_string())?
            .map(DataValue::DateTime)
            .unwrap_or(DataValue::Null)),
        Type::TIMESTAMPTZ => Ok(row
            .try_get::<_, Option<DateTime<Utc>>>(index)
            .map_err(|error| error.to_string())?
            .map(|value| DataValue::DateTime(value.naive_utc()))
            .unwrap_or(DataValue::Null)),
        _ if ty.name() == "geometry" => {
            let bytes = row
                .try_get::<_, Option<WkbBytes>>(index)
                .map_err(|error| error.to_string())?;
            match bytes {
                None => Ok(DataValue::Null),
                Some(WkbBytes(raw)) => parse_ewkb(&raw),
            }
        }
        _ => Err(format!(
            "unsupported PostgreSQL source type `{}`",
            ty.name()
        )),
    }
}

pub(super) fn data_values_to_mysql_values(values: Vec<DataValue>) -> Result<Vec<Value>, String> {
    values.into_iter().map(data_value_to_mysql_value).collect()
}

fn data_value_to_mysql_value(value: DataValue) -> Result<Value, String> {
    match value {
        DataValue::Null => Ok(Value::NULL),
        DataValue::String(text) => Ok(Value::Bytes(text.into_bytes())),
        DataValue::I64(value) => Ok(Value::Int(value)),
        DataValue::U64(value) => Ok(Value::UInt(value)),
        DataValue::F64(value) => Ok(Value::Double(value)),
        DataValue::Bool(value) => Ok(Value::Int(if value { 1 } else { 0 })),
        DataValue::Bytes(bytes) => Ok(Value::Bytes(bytes)),
        DataValue::Date(value) => Ok(Value::Date(
            value.year() as u16,
            value.month() as u8,
            value.day() as u8,
            0,
            0,
            0,
            0,
        )),
        DataValue::Time(value) => Ok(Value::Time(
            false,
            0,
            value.hour() as u8,
            value.minute() as u8,
            value.second() as u8,
            value.nanosecond() / 1_000,
        )),
        DataValue::DateTime(value) => Ok(Value::Date(
            value.date().year() as u16,
            value.date().month() as u8,
            value.date().day() as u8,
            value.time().hour() as u8,
            value.time().minute() as u8,
            value.time().second() as u8,
            value.time().nanosecond() / 1_000,
        )),
        DataValue::Geometry(_) => Err(String::from(
            "geometry values must be converted with `area` or `perimeter` before writing to MySQL",
        )),
    }
}

pub(super) fn data_values_to_postgres_params(values: Vec<DataValue>) -> Result<Vec<PgParam>, String> {
    values
        .into_iter()
        .map(|value| -> Result<PgParam, String> {
            match value {
                DataValue::Null => Ok(Box::new(Option::<String>::None) as PgParam),
                DataValue::String(text) => Ok(Box::new(text) as PgParam),
                DataValue::I64(value) => Ok(Box::new(value) as PgParam),
                DataValue::U64(value) => {
                    if value <= i64::MAX as u64 {
                        Ok(Box::new(value as i64) as PgParam)
                    } else {
                        Ok(Box::new(value.to_string()) as PgParam)
                    }
                }
                DataValue::F64(value) => Ok(Box::new(value) as PgParam),
                DataValue::Bool(value) => Ok(Box::new(value) as PgParam),
                DataValue::Bytes(bytes) => Ok(Box::new(bytes) as PgParam),
                DataValue::Date(value) => Ok(Box::new(value) as PgParam),
                DataValue::Time(value) => Ok(Box::new(value) as PgParam),
                DataValue::DateTime(value) => Ok(Box::new(value) as PgParam),
                DataValue::Geometry(_) => Err(String::from(
                    "geometry values must be converted with `area` or `perimeter` before writing to PostgreSQL",
                )),
            }
        })
        .collect()
}
