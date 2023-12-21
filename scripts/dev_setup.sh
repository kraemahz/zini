#!/bin/bash -e
diesel migration --database-url postgres://postgres:development@localhost/zini run
