#!/bin/bash

set -euo pipefail

usage() {
    echo "Usage: $0 [start|stop]"
}

if [ $# -ne 1 ]; then
    usage
    exit 1
fi

if docker compose version >/dev/null 2>&1; then
    compose_cmd=(docker compose)
elif command -v docker-compose >/dev/null 2>&1; then
    compose_cmd=(docker-compose)
elif podman compose version >/dev/null 2>&1; then
    compose_cmd=(podman compose)
elif command -v podman-compose >/dev/null 2>&1; then
    compose_cmd=(podman-compose)
else
    compose_cmd=()
fi

case "$1" in
    start)
        if [ ${#compose_cmd[@]} -gt 0 ]; then
            "${compose_cmd[@]}" -f ./ops/postgis_mysql.compose.yaml up --build -d
        elif command -v podman >/dev/null 2>&1; then
            podman build -t datafowk-postgis-source -f ./ops/postgis/Dockerfile.source.postgis ./ops
            podman build -t datafowk-postgis-dest-mysql -f ./ops/postgis/Dockerfile.destination.mysql ./ops
            podman rm -f postgis_source_net postgis_mysql_dest_net >/dev/null 2>&1 || true
            podman run -d --name postgis_source_net \
                -e POSTGRES_USER=local \
                -e POSTGRES_PASSWORD=password \
                -e POSTGRES_DB=public \
                -p 5433:5432 \
                datafowk-postgis-source
            podman run -d --name postgis_mysql_dest_net \
                -e MYSQL_USER=local \
                -e MYSQL_ROOT_PASSWORD=password \
                -e MYSQL_DATABASE=regions_dest \
                -e MYSQL_PASSWORD=password \
                -p 3309:3306 \
                datafowk-postgis-dest-mysql
        else
            echo "Neither Docker Compose nor Podman is available."
            exit 1
        fi
        ;;
    stop)
        if [ ${#compose_cmd[@]} -gt 0 ]; then
            "${compose_cmd[@]}" -f ./ops/postgis_mysql.compose.yaml down
        elif command -v podman >/dev/null 2>&1; then
            podman rm -f postgis_source_net postgis_mysql_dest_net >/dev/null 2>&1 || true
        else
            echo "Neither Docker Compose nor Podman is available."
            exit 1
        fi
        ;;
    *)
        usage
        exit 1
        ;;
esac
