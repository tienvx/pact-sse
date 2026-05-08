# Build and Test Commands

## Build the plugin
```bash
cargo build --release
```

## Install the plugin
```bash
mkdir -p ~/.pact/plugins/sse-0.1.0
cp target/release/pact-sse-plugin ~/.pact/plugins/sse-0.1.0/
cp pact-plugin.json ~/.pact/plugins/sse-0.1.0/
```

## Run the plugin
```bash
cargo run --release
```

## Run tests

### Plugin unit tests
```bash
cargo test
```

### Consumer test

Note: Tests may encounter runtime issues due to tokio constraints with plugins.
```bash
cd examples/sse-consumer
PACT_DO_NOT_TRACK=true cargo test
```

### Provider verification test
```bash
cd examples/sse-provider
PACT_DO_NOT_TRACK=true cargo test
```

## Test execution order

1. Build and install the plugin first
2. Run consumer test (generates pact file to `examples/pacts/sseConsumer-sseProvider.json`)
3. Run provider test

The pact JSON file (`examples/pacts/sseConsumer-sseProvider.json`) is generated automatically by the consumer tests and should not be edited manually.

**Important:** The consumer and provider examples use the installed plugin binary at `~/.pact/plugins/sse-0.1.0/`. Always rebuild and reinstall after code changes before running the examples.

## SSE Plugin Notes

- **Matching rules:** SSE data values are strings, so never rely on `type` matcher alone -- it only checks if both values are the same type (string), meaning `'aa'` would match `'100'`. Always use stricter matchers like `integer`, `number`, `regex`, etc.
- **`id` and `retry`:** These are global SSE fields applied to all events, not per-event data. On consumer side, they're placed in the first event, all events should have the same `id`/`retry`. On provider side, matching rules (number, integer) must be enforced.
