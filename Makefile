.phony: build
build:
	fastly compute build --verbose

.phony: deploy
deploy:
	fastly compute deploy

.phony: try
try:
	http --headers https://broadly-super-bear.edgecompute.app/2020/12/31/22208287/duet-proxy-magic
	http --header https://broadly-super-bear.edgecompute.app/packs/js/50-39d45b60161a3cc59ddf.chunk.js
