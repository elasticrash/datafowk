use crate::{
    config::DatabaseKind,
    etl::{
        load_config_or_default,
        sql::{build_insert_statement, build_select_statement, quote_identifier},
    },
    etl_rule_parser::parser::parse_rule,
    models::DataValue,
    transforms::apply_transform,
};

#[test]
fn build_mysql_sql_statements_quote_identifiers() {
    let rule =
        parse_rule("(origin:users)[firstname,lastname]<copy>(destination:spot)[name,surname]")
            .unwrap();

    assert_eq!(
        build_select_statement(DatabaseKind::Mysql, &rule).unwrap(),
        "SELECT `firstname`, `lastname` FROM `users`"
    );
    assert_eq!(
        build_insert_statement(DatabaseKind::Mysql, &rule).unwrap(),
        "INSERT INTO `spot` (`name`, `surname`) VALUES (?, ?)"
    );
}

#[test]
fn build_postgres_sql_statements_use_postgres_quoting() {
    let rule =
        parse_rule("(origin:users)[firstname,lastname]<copy>(destination:spot)[name,surname]")
            .unwrap();

    assert_eq!(
        build_select_statement(DatabaseKind::Postgres, &rule).unwrap(),
        "SELECT \"firstname\", \"lastname\" FROM \"users\""
    );
    assert_eq!(
        build_insert_statement(DatabaseKind::Postgres, &rule).unwrap(),
        "INSERT INTO \"spot\" (\"name\", \"surname\") VALUES ($1, $2)"
    );
}

#[test]
fn build_sql_statements_support_joined_sources() {
    let rule = parse_rule(
        "(origin:users,address){users.address_id=address.id}[users.firstname,address.address,address.number]<trim>(destination:spot)[name,address,number]",
    )
    .unwrap();

    assert_eq!(
        build_select_statement(DatabaseKind::Mysql, &rule).unwrap(),
        "SELECT `users`.`firstname`, `address`.`address`, `address`.`number` FROM `users` JOIN `address` ON `users`.`address_id` = `address`.`id`"
    );
}

#[test]
fn apply_function_transforms_string_values() {
    let mut value = DataValue::String(String::from("  Alice  "));

    apply_transform(
        &mut value,
        &crate::models::RuleTransform {
            name: String::from("trim"),
            arguments: Vec::new(),
        },
        DatabaseKind::Mysql,
    )
    .unwrap();
    apply_transform(
        &mut value,
        &crate::models::RuleTransform {
            name: String::from("uppercase"),
            arguments: Vec::new(),
        },
        DatabaseKind::Mysql,
    )
    .unwrap();

    assert_eq!(value, DataValue::String(String::from("ALICE")));
}

#[test]
fn quote_identifier_rejects_invalid_names() {
    let error = quote_identifier(DatabaseKind::Mysql, "users; DROP TABLE spot").unwrap_err();
    assert!(error.contains("unsupported identifier"));
}

#[test]
fn load_config_or_default_returns_default_for_missing_file() {
    let config = load_config_or_default("/tmp/datafowk-missing-config.toml").unwrap();

    assert_eq!(config.connection_properties_origin.user, "root");
    assert_eq!(
        config.connection_properties_origin.kind,
        DatabaseKind::Mysql
    );
    assert!(config.rules.is_empty());
}
