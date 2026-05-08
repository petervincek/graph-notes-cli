# Graph Notes CLI

Graph Notes CLI is a terminal application for managing graph notes with SQLite, supporting links, metadata, and robust configuration layering.

## Installation

1. **Build and install the binary:**
	 ```sh
	 cargo install --path .
	 # or manually:
	 cargo build --release
	 cp target/release/graph-notes-cli ~/.cargo/bin/
	 # or system-wide (requires sudo):
	 sudo cp target/release/graph-notes-cli /usr/local/bin/
	 ```

2. **Initialize the default configuration file:**
	 ```sh
	 graph-notes-cli init-config
	 # This creates a config file at $XDG_CONFIG_HOME/graph-notes-cli/config.toml or $HOME/.config/graph-notes-cli/config.toml
	 ```

## Configuration

The CLI supports layered configuration with the following priority (highest to lowest):

1. **Command-line options** (`--db-url`, `--log-level`, `--config`)
2. **Environment variables** (prefixed with `GRAPH_NOTES_`, e.g., `GRAPH_NOTES_DB_URL`)
3. **Configuration file** (TOML, discovered automatically or provided via `--config`)
4. **Built-in defaults**

### Providing a Configuration File

- By default, the CLI looks for a config file at:
	- `$XDG_CONFIG_HOME/graph-notes-cli/config.toml` (if `XDG_CONFIG_HOME` is set)
	- `$HOME/.config/graph-notes-cli/config.toml` (otherwise)
- You can specify a custom config file with `--config`:
	```sh
	graph-notes-cli --config /path/to/my-config.toml create "Title" "Content"
	```

### Example `config.toml`

```toml
db_url = "sqlite://notes.db"
log_level = "info"
```

### Environment Variables

You can override config values with environment variables:

- `GRAPH_NOTES_DB_URL` sets the database URL
- `GRAPH_NOTES_LOG_LEVEL` sets the log level

## Usage

```sh
graph-notes-cli [OPTIONS] <COMMAND>
```

### Global Options

- `--config <CONFIG>`: Optional config file path (overrides default config discovery)
- `--db-url <DB_URL>`: Optional database URL (overrides config/env)
- `--log-level <LOG_LEVEL>`: Optional log level (overrides config/env)

### Commands

- `create <TITLE> <CONTENT> [--metadata <JSON>]`  
	Create a new graph note. Optionally provide metadata as a JSON string.
  
	**Example:**
	```sh
	graph-notes-cli create "My Note" "Some content"
	graph-notes-cli create "Note with meta" "Content" --metadata '{"tags":["rust","cli"]}'
	```

- `read <ID>`  
	Read a graph note by its ID.
  
	**Example:**
	```sh
	graph-notes-cli read 1
	```

- `update <ID> <TITLE> <CONTENT>`  
	Update an existing graph note by ID.
  
	**Example:**
	```sh
	graph-notes-cli update 1 "Updated Title" "Updated Content"
	```

- `delete <ID>`  
	Delete a graph note by ID (also deletes related links).
  
	**Example:**
	```sh
	graph-notes-cli delete 1
	```

- `link <FROM_NOTE_ID> <TO_NOTE_ID> <LINK_TYPE>`  
	Link two notes together. `LINK_TYPE` can be `related`, `reference`, `parent`, or `child`.
  
	**Example:**
	```sh
	graph-notes-cli link 1 2 related
	graph-notes-cli link 2 3 reference
	```

- `init-config`  
	Initialize a default configuration file in the standard config directory.
  
	**Example:**
	```sh
	graph-notes-cli init-config
	# Output: Default config created at: /home/user/.config/graph-notes-cli/config.toml
	```

## Output

All commands print results as pretty-printed JSON for easy scripting and integration.

## Testing

Run all tests:

```sh
make test
# or
cargo test
```

## License

GPL-3.0