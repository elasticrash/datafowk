use nom::bytes::complete::{tag, take_until};
use nom::sequence::tuple;
use nom::IResult;

#[derive(Debug)]
pub struct Rules {
    pub source_db: String,
    pub source_table: String,
    pub source_fields: Vec<String>,
    pub function_chain: Vec<String>,
    pub destination_db: String,
    pub destination_table: String,
    pub destination_fields: Vec<String>,
}

fn parse(input: &str) -> IResult<&str, Rules> {
    let (rem, (_, source_db, _, source_table, _)) = tuple((
        tag("("),
        take_until(":"),
        tag(":"),
        take_until(")"),
        tag(")"),
    ))(input)?;

    let (rem, (_, source_fields, _)) = tuple((tag("["), take_until("]"), tag("]")))(rem)?;

    let (rem, (_, fn_names, _)) = tuple((tag("<"), take_until(">"), tag(">")))(rem)?;

    let (rem, (_, destination_db, _, destination_table, _)) = tuple((
        tag("("),
        take_until(":"),
        tag(":"),
        take_until(")"),
        tag(")"),
    ))(rem)?;

    let (rem, (_, destination_fields, _)) = tuple((tag("["), take_until("]"), tag("]")))(rem)?;

    return Ok((
        rem,
        Rules {
            source_db: source_db.to_string(),
            source_table: source_table.to_string(),
            source_fields: source_fields.split(',').map(|s| s.to_string()).collect(),
            function_chain: fn_names.split(',').map(|s| s.to_string()).collect(),
            destination_db: destination_db.to_string(),
            destination_table: destination_table.to_string(),
            destination_fields: destination_fields
                .split(',')
                .map(|s| s.to_string())
                .collect(),
        },
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parser() {
        let input = "(db1:table1)[field1,field2]<fn,fn2,fn3>(db2:table2)[field3,field4]";
        let result = parse(input).unwrap().1;

        assert_eq!(result.source_db, "db1");
        assert_eq!(result.source_table, "table1");
        assert_eq!(result.source_fields, vec!["field1", "field2"]);
        assert_eq!(result.function_chain, vec!["fn", "fn2", "fn3"]);
        assert_eq!(result.destination_db, "db2");
        assert_eq!(result.destination_table, "table2");
        assert_eq!(result.destination_fields, vec!["field3", "field4"]);
    }
}
