# Datafowk

Terminal ETL tool for MySQL and PostgreSQL.

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

```toml
[connection_properties_origin]
kind = "mysql"
address = "127.0.0.1"
port = 3306
user = "root"
password = "password"
schema = "test"

[connection_properties_destination]
kind = "mysql"
address = "127.0.0.1"
port = 3308
user = "root"
password = "password"
schema = "test"
```

engine types (kind)

* "mysql"
* "postgres"

Rules live under `[[rules]]` in `mysql_config.toml`:

```toml
[[rules]]
expression = "(origin:users,address){users.address_id=address.id}[users.firstname,users.lastname,address.address,address.number]<trim>(destination:spot)[name,surname,address,number]"

[[rules]]
expression = "(origin:order_totals)[amount]<sum(10)>(destination:order_totals_plus_ten)[amount]"

[[rules]]
expression = "(origin:sensor_weights)[weight]<multiply(5)>(destination:sensor_weights_scaled)[weight]"

[[rules]]
expression = "(origin:customer_aliases)[email,label]<trim,unique(email)>(destination:customer_aliases_unique)[email,label]"
```

Rule format:

```text
(database_alias:table1[,table2...]){table1.column=table2.column[,table2.column=table3.column...]}[field1,table2.field2]<copy,trim,lowercase,uppercase,sum(10),multiply(5),unique(field1,field2)>(database_alias:table)[field1,field2]
```

Supported database aliases:

* `origin` for `connection_properties_origin`
* `destination` for `connection_properties_destination`

When you use multiple source tables, source fields must be written as `table.column` and the join conditions should describe the 1-1 relationship path.

Supported transforms:

* `copy`
* `trim`
* `lowercase`
* `uppercase`
* `sum(number)` / `add(number)` for numeric values
* `multiply(number)` / `mul(number)` for numeric values
* `unique(destination_field[,destination_field...])` to skip duplicate destination rows and log them to `datafowk-skipped-duplicates.log`

## running it

1. Start the sample databases:

   ```bash
   ./docker.sh start
   ```

   The helper prefers Docker Compose, then Podman Compose, and finally plain Podman containers.

2. Open the interactive terminal UI and design a pipeline:

   ```bash
   cargo run --
   ```

   The footer keeps a single `? shortcuts` hint; press `?` to open the shortcuts popup.

   Main keys:

   * `n` create a rule
   * `c` clone the selected rule so one source flow can target another destination table
   * `e` edit the selected rule
   * `d` delete the selected rule
   * `o` / `p` edit origin or destination connection
   * `v` preview origin and destination schemas
   * `s` save config
   * `t` dry-run simulation
   * `r` run
   * `x` run with destination truncation
   * `q` quit

   Inside schema preview:

   * arrow keys pan horizontally and vertically
   * `1` shows table names only
   * `2` shows table names and column names
   * `3` shows table names, column names, and column types
   * `+` / `-` cycle zoom levels
   * `esc` closes the preview

3. Preview the load without writing rows:

   ```bash
   cargo run -- run --dry-run
   ```

4. Load the data into the destination table:

   ```bash
   cargo run -- run --truncate-destination
   ```

`dry-run` now performs a full simulation: it reads source rows and attempts destination inserts inside a transaction that is rolled back, so missing tables, missing columns, and destination constraints surface without persisting changes.

The bundled sample schema seeds:

* `users` + `address` into destination `spot`
* `order_totals` into `order_totals_plus_ten` with `sum(10)`
* `sensor_weights` into `sensor_weights_scaled` with `multiply(5)`
* `customer_aliases` into `customer_aliases_unique` with `unique(email)`
