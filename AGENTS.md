# Tim is a command centric chat based assistant.

## General ideas

The document describes intentions and ideas rather than actual state. Style is imperative for simplicity.

UI is command line centric, AutoCAD like. States is evlovled through the backend initiated async updates. Any user action calls backend and leads to no state changes (excluding some optimizations). UI <-> backend protocol is gRPC-web. Many users collaboration on a single project is possible.

Three components: frontend: tim-web-ui, backend: tim-code, cloud: tim-cloud. tim-code contains tim-web-ui and is run locally. tim-cloud is used to sync between many tim-code nodes.

## Tooling & env

tim-web-ui:
  typescript, svelte, vite, connect-web

tim-code:
  rust
  grpc

tim-cloud:
  java 21, spring, spring boot all latest versions


## Work style

Iterative, small step changes. Each patch must have high quality code just like it should pass review of the world top 100 tech lead (rust, java, typescript). Again, keep changes small. Offer to check the patch and re-iterate rather than create big patch of full functionality.
