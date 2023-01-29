define setup_env
    $(eval ENV_FILE := .env)
    @echo " - setup env $(ENV_FILE)"
    $(eval include .env)
    $(eval export)
endef

dotenv:
	$(call setup_env)

.phony: all

build:
	fastly compute build --verbose

deploy: dotenv
	fastly compute deploy

tail: dotenv
	fastly log-tail

try:
	http --headers https://broadly-super-bear.edgecompute.app/2020/12/31/22208287/duet-proxy-magic
	http --header https://broadly-super-bear.edgecompute.app/packs/js/50-39d45b60161a3cc59ddf.chunk.js
