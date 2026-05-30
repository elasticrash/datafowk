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
    /// PostGIS geometry: outer Vec = polygons, middle = rings (ring[0] = exterior),
    /// inner = (x, y) coordinate pairs.
    Geometry(Vec<Vec<Vec<(f64, f64)>>>),
}
