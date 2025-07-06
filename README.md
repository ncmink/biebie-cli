# biebie-cli

A command-line tool for scanning and uploading files.

## Quick Start

### 1. Build the Project

Build the release version for ARM64 macOS:

```bash
cargo build --release --target aarch64-apple-darwin
```

### 2. Run the CLI

Execute the CLI tool with a target folder:

```bash
target/aarch64-apple-darwin/release/biebie-cli /path/to/folder
```

### 3. Verify Installation (Optional)

Check that the binary is built for ARM64 architecture:

```bash
file target/aarch64-apple-darwin/release/biebie-cli
```

## Usage

Replace `/path/to/folder` with the actual path to the directory you want to process.

## Requirements

- Rust toolchain with ARM64 target support
- macOS (ARM64)
