#/bin/bash

if [ $# -eq 0 ]; then
	echo "Usage: $0 [start|stop]"
fi

if [ $1 == "start" ]; then
	docker-compose -d  -f ./ops/mysql.compose.yaml up --build
fi

if [ $1 == "stop" ]; then
	docker-compose -f ./ops/mysql.compose.yaml down
fi

