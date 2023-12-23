#!/bin/bash
export PGPASSWORD="${PG_PASSWORD}"
export DATABASE_URL="postgres://${PG_USER}:${PG_PASSWORD}@${PG_HOST}/zini"
DBNAME=zini

if ! psql -U ${PG_USER} --host=${PG_HOST} -tc "SELECT 1 FROM pg_database WHERE datname = '${DBNAME}';" | grep -q 1
then
    psql -U ${PG_USER} --host=${PG_HOST} -c "CREATE DATABASE ${DBNAME};"
fi
diesel migration run
