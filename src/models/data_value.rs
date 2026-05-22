use chrono::{NaiveDate, NaiveDateTime, NaiveTime};

#[derive(Debug, Clone, PartialEq)]
pub enum DataValue {
    Null,
    String(String),
    I64(i64),
    U64(u64),
    F64(f64),
    Bool(bool),
    Bytes(Vec<u8>),
    Date(NaiveDate),
    Time(NaiveTime),
    DateTime(NaiveDateTime),
}
