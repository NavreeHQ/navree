# @navree/client

A typed client for Navree APIs. It calls an API with types generated from the API's own OpenAPI document, so a renamed field or a removed route is a compile error in the front end. It has no runtime dependencies and runs in browsers, on servers and in any JavaScript runtime.

## Install

```sh
npm install @navree/client
```

## Quick start

Generate the schema from your Navree app, then create the client:

```sh
navree openapi:client
```

```ts
import { createNavreeClient, unwrap } from "@navree/client";
import type { paths } from "./schema";

const api = createNavreeClient<paths>();

const deal = await unwrap(api.GET("/api/v1/deals/{id}", { params: { path: { id } } }));
```

With no `token` the client is in cookie mode: it calls the page's own origin, behind a backend-for-frontend such as `@navree/bff`. With a `token` it is in bearer mode and sends `Authorization: Bearer`.

## What is in it

- `createNavreeClient<paths>()`: the typed client, with the workspace header and request ids.
- `NavreeError` and `errorMessage`: Navree's error shape with its `code`.
- `listQuery()` and `checkListQuery()`: filters, sort, search and paging in the one list-query format Navree lists read.
- Pages, query keys, `followOperation` for long-running operations, text direction and error-code translation.
- Abilities (`@navree/client/abilities`, needs `@casl/ability`) and real time (`@navree/client/realtime`, needs `pusher-js`) are separate entry points.

Documentation: https://navree.com/docs/frontend/typed-client/

## License

MIT OR Apache-2.0
