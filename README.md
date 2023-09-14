# Datafowk

ETL tool

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

nothing yet


## configuration
(db:table)[field(s)]-(transformation)-(db:table)[field(s)]

