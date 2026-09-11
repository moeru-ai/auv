# @auv-js/api-client

Generated Fetch bindings for the daemon-owned AUV REST API.

<!-- START doctoc generated TOC please keep comment here to allow auto update -->
<!-- DON'T EDIT THIS SECTION, INSTEAD RE-RUN doctoc TO UPDATE -->
## Table of Contents

- [Installation](#installation)
- [Usage](#usage)
- [Scope](#scope)
- [Development](#development)

<!-- END doctoc generated TOC please keep comment here to allow auto update -->

## Installation

```sh
npm install @auv-js/api-client
```

```sh
pnpm add @auv-js/api-client
```

## Usage

Create a client for an AUV daemon. Pass the client to each generated operation.

```ts
import {
  createClient,
  deviceServiceListDevices,
} from '@auv-js/api-client'

const client = createClient({
  baseUrl: 'http://127.0.0.1:9847',
  headers: {
    Authorization: 'Bearer <credential>',
  },
})

const { data, error } = await deviceServiceListDevices({ client })
if (error !== undefined)
  throw error

console.log(data.devices)
```

The current OpenAPI document does not describe the listener bearer policy.
Callers must supply `Authorization: Bearer <credential>` for protected remote
operations. Caller-local owner endpoints do not require this header.

## Scope

Dynamic Runner invocation and WebSocket streaming are runtime-described
protocols and are intentionally owned by `@auv-js/sdk`, not this package.

`@auv-js/sdk` owns pairing, transport selection, Runner routing, streamed
invocation, and the higher-level Device and Run interfaces.

## Development

After a Protobuf or HTTP annotation change, run the root generation command:

```sh
pnpm generate:proto
```

This command generates Protobuf-ES and OpenAPI artifacts together. The package
build then generates the Fetch client from the OpenAPI document with Hey API.
Do not edit files under `src/gen` by hand.
