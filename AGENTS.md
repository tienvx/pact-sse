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

### Consumer example

Note: Tests may encounter runtime issues due to tokio constraints with plugins.
```bash
cd examples/sse-consumer
PACT_DO_NOT_TRACK=true cargo test
```

### Provider example

The provider is a binary server that can be run directly:
```bash
cd examples/sse-provider
cargo run
```
