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
			"${compose_cmd[@]}" -f ./ops/mysql.compose.yaml up --build -d
		elif command -v podman >/dev/null 2>&1; then
			podman build -t datafowk-mysql-source -f ./ops/mysql/Dockerfile.source.mysql ./ops
			podman build -t datafowk-mysql-destination -f ./ops/mysql/Dockerfile.destination.mysql ./ops
			podman rm -f mysql_source_net mysql_destination_net >/dev/null 2>&1 || true
			podman run -d --name mysql_source_net \
				-e MYSQL_USER=local \
				-e MYSQL_ROOT_PASSWORD=password \
				-e MYSQL_DATABASE=test \
				-e MYSQL_PASSWORD=password \
				-p 3306:3306 \
				datafowk-mysql-source
			podman run -d --name mysql_destination_net \
				-e MYSQL_USER=local \
				-e MYSQL_ROOT_PASSWORD=password \
				-e MYSQL_DATABASE=test \
				-e MYSQL_PASSWORD=password \
				-p 3308:3306 \
				datafowk-mysql-destination
		else
			echo "Neither Docker Compose nor Podman is available."
			exit 1
		fi
		;;
	stop)
		if [ ${#compose_cmd[@]} -gt 0 ]; then
			"${compose_cmd[@]}" -f ./ops/mysql.compose.yaml down
		elif command -v podman >/dev/null 2>&1; then
			podman rm -f mysql_source_net mysql_destination_net >/dev/null 2>&1 || true
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
