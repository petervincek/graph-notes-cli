use config::{Config, ConfigError, Environment, File};
use serde::Deserialize;

/// Command-line options for the CLI application.
#[derive(Debug)]
pub struct CliOptions {
    /// Database URL override from CLI.
    pub db_url: Option<String>,
    /// Log level override from CLI.
    pub log_level: Option<String>,
}

/// Application configuration combining defaults, files, environment, and CLI options.
#[derive(Debug, Deserialize)]
pub struct AppConfig {
    /// The database URL to connect to.
    pub db_url: String,
    /// The logging level (trace, debug, info, warn, error).
    pub log_level: Option<String>,
}

impl AppConfig {
    /// Builds AppConfig from multiple sources with the following priority:
    /// 1. CLI options (highest priority)
    /// 2. Environment variables (with GRAPH_NOTES prefix)
    /// 3. Configuration file (if provided)
    /// 4. Defaults (lowest priority)
    ///
    /// # Arguments
    /// * `config_path` - Optional path to a TOML configuration file
    /// * `cli_options` - Command-line options that override all other sources
    ///
    /// # Errors
    /// Returns `ConfigError` if configuration building or deserialization fails.
    pub fn from_sources(
        config_path: Option<&str>,
        cli_options: CliOptions,
    ) -> Result<Self, ConfigError> {
        // let's start with the default options
        let mut config_builder = Config::builder()
            .set_default("db_url", "graph-notes-dev.db")?
            .set_default("log_level", "info")?;

        // optional config file (TOML)
        if let Some(path) = config_path {
            config_builder = config_builder.add_source(File::with_name(path).required(false));
        }

        // check for environment variables
        config_builder =
            config_builder.add_source(Environment::with_prefix("GRAPH_NOTES").separator("_"));

        // check for CLI overrides
        if let Some(db_url) = cli_options.db_url {
            config_builder = config_builder.set_override("db_url", db_url)?;
        }
        if let Some(log_level) = cli_options.log_level {
            config_builder = config_builder.set_override("log_level", log_level)?;
        }
        config_builder.build()?.try_deserialize()
    }
}
