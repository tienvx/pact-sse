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

1. Run consumer test first (generates pact file to `examples/pacts/sseConsumer-sseProvider.json`)
2. Run provider test

The pact JSON file (`examples/pacts/sseConsumer-sseProvider.json`) is generated automatically by the consumer tests and should not be edited manually.
