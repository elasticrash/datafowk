# Datafowk

Small MySQL-to-MySQL ETL tool.

## files
### root
* mysql_to_mysql.sh - runs 2 mysql dbs with different schemas
### ops

* mysql.compose.yaml - brings up the 2 mysql instances

* /mysql/Dockerfile.source.mysql - Dockerfile for source db 
* /mysql/Dockerfile.destination.mysql - Dockefile for destination db
* /mysql/mysql_schema_source.sql - sql schema for source db
* /mysql/mysql_schema_destination.sql - sql schema for destination db 

### src

* Rust CLI that reads rules from `mysql_config.toml`, pulls rows from the source DB, applies a small transformation chain, and inserts them into the destination DB.


## configuration

Rules live under `[[rules]]` in `mysql_config.toml`:

```toml
[[rules]]
expression = "(origin:users)[firstname,lastname]<trim>(destination:spot)[name,surname]"
```

Rule format:

```text
(database_alias:table)[field1,field2]<copy,trim,lowercase,uppercase>(database_alias:table)[field1,field2]
```

Supported database aliases:

* `origin` for `connection_properties_origin`
* `destination` for `connection_properties_destination`

The number of source and destination fields must match.

## running it

1. Start the sample databases:

   ```bash
   ./docker.sh start
   ```

   The helper prefers Docker Compose, then Podman Compose, and finally plain Podman containers.

2. Open the interactive terminal UI and design a pipeline:

   ```bash
   cargo run -- ui
   ```

   The UI is closer to `cc_counter`: a persistent full-screen TUI with a rules list on the left, a live rule diagram on the right, rule details below it, and popup editors for new or existing rules.

   Main keys:

   * `n` create a rule
   * `e` edit the selected rule
   * `d` delete the selected rule
   * `o` / `p` edit origin or destination connection
   * `s` save config
   * `t` dry-run
   * `r` run
   * `x` run with destination truncation
   * `q` quit

3. Preview the load without writing rows:

   ```bash
   cargo run -- --dry-run
   ```

4. Load the data into the destination table:

   ```bash
   cargo run -- --truncate-destination
   ```

The terminal UI can edit both connections, add or remove rules, show a small visual depiction of the selected rule, save the config, and run the pipeline directly.

The bundled sample schema seeds `users` in the source DB and loads `name` / `surname` rows into `spot` in the destination DB.
