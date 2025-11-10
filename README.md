# Tim is a command centric chat based assistant.

## General ideas

The document describes intentions and ideas rather than actual state. Style is imperative for simplicity.

UI is command line centric, AutoCAD like. States is evlovled through the backend initiated async updates. Any user action calls backend and leads to no state changes (excluding some optimizations). UI <-> backend protocol is gRPC-web. Many users collaboration on a single project is possible.

Three components: frontend: tim-web-ui, backend: tim-code, cloud: tim-cloud. tim-code contains tim-web-ui and is run locally. tim-cloud is used to sync between many tim-code nodes.

## Tooling & env

tim-web-ui:
  typescript
  svelte
  vite
  connect-web

tim-code:
  rust
  tonic
  buf

tim-cloud:
  java 21, spring, spring boot all latest versions
