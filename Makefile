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

serve: dotenv
	fastly compute serve --addr 127.0.0.1:4000

test:
	cargo wasi test

