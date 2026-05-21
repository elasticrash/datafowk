use nom::bytes::complete::{tag, take_until};
use nom::sequence::tuple;
use nom::IResult;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rules {
    pub source_db: String,
    pub source_table: String,
    pub source_fields: Vec<String>,
    pub function_chain: Vec<String>,
    pub destination_db: String,
    pub destination_table: String,
    pub destination_fields: Vec<String>,
}

fn split_csv_values(values: &str) -> Vec<String> {
    values
        .split(',')
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(|value| value.to_string())
        .collect()
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
            source_db: source_db.trim().to_string(),
            source_table: source_table.trim().to_string(),
            source_fields: split_csv_values(source_fields),
            function_chain: split_csv_values(fn_names),
            destination_db: destination_db.trim().to_string(),
            destination_table: destination_table.trim().to_string(),
            destination_fields: split_csv_values(destination_fields),
        },
    ));
}

pub fn parse_rule(input: &str) -> Result<Rules, String> {
    let (remaining, rule) =
        parse(input).map_err(|error| format!("failed to parse rule `{input}`: {error:?}"))?;

    if !remaining.trim().is_empty() {
        return Err(format!(
            "rule `{input}` has unexpected trailing content `{remaining}`"
        ));
    }

    if rule.source_fields.is_empty() {
        return Err(format!(
            "rule `{input}` must contain at least one source field"
        ));
    }

    if rule.destination_fields.is_empty() {
        return Err(format!(
            "rule `{input}` must contain at least one destination field"
        ));
    }

    Ok(rule)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parser() {
        let input = "(db1:table1)[field1,field2]<fn,fn2,fn3>(db2:table2)[field3,field4]";
        let result = parse_rule(input).unwrap();

        assert_eq!(result.source_db, "db1");
        assert_eq!(result.source_table, "table1");
        assert_eq!(result.source_fields, vec!["field1", "field2"]);
        assert_eq!(result.function_chain, vec!["fn", "fn2", "fn3"]);
        assert_eq!(result.destination_db, "db2");
        assert_eq!(result.destination_table, "table2");
        assert_eq!(result.destination_fields, vec!["field3", "field4"]);
    }

    #[test]
    fn test_parser_trims_values() {
        let input = "(origin:users)[ firstname , lastname ]< trim , uppercase >(destination:spot)[ name , surname ]";
        let result = parse_rule(input).unwrap();

        assert_eq!(result.source_fields, vec!["firstname", "lastname"]);
        assert_eq!(result.function_chain, vec!["trim", "uppercase"]);
        assert_eq!(result.destination_fields, vec!["name", "surname"]);
    }
}
