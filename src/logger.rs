use chrono::Local;
use colored::*;
use std::env;
use tracing_subscriber::EnvFilter;

/// Initializes the global logging system with colorized output and environment-based level filtering.
///
/// The `RUST_LOG` environment variable can be used to control the log level (default: info).
/// Example: `RUST_LOG=debug cargo run --example remote_test`
pub fn init_logging() {
    if env::var("RUST_LOG").is_err() {
        unsafe { env::set_var("RUST_LOG", "info") };
    }

    // Force colored output even if not a TTY
    colored::control::set_override(true);

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_ansi(true)
        .with_writer(std::io::stdout)
        .event_format(CustomFormatter)
        .init();
}

struct CustomFormatter;

impl<S, N> tracing_subscriber::fmt::FormatEvent<S, N> for CustomFormatter
where
    S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
    N: for<'a> tracing_subscriber::fmt::FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        _ctx: &tracing_subscriber::fmt::FmtContext<'_, S, N>,
        mut writer: tracing_subscriber::fmt::format::Writer<'_>,
        event: &tracing::Event<'_>,
    ) -> std::fmt::Result {
        let now = Local::now().format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string();
        let level = *event.metadata().level();

        let level_str = match level {
            tracing::Level::ERROR => "ERROR".red().bold().to_string(),
            tracing::Level::WARN => "WARN".yellow().bold().to_string(),
            tracing::Level::INFO => "INFO".green().bold().to_string(),
            tracing::Level::DEBUG => "DEBUG".blue().bold().to_string(),
            tracing::Level::TRACE => "TRACE".magenta().bold().to_string(),
        };

        write!(writer, "{} {} ", now.dimmed(), level_str)?;

        // Visit the message field without escaping
        let mut message = String::new();
        let mut visitor = MessageVisitor {
            message: &mut message,
        };
        event.record(&mut visitor);

        write!(writer, "{}", message)?;
        writeln!(writer)
    }
}

struct MessageVisitor<'a> {
    message: &'a mut String,
}

impl<'a> tracing::field::Visit for MessageVisitor<'a> {
    fn record_debug(&mut self, _field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        use std::fmt::Write;
        let _ = write!(self.message, "{:?}", value);
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        use std::fmt::Write;
        if field.name() == "message" {
            let _ = write!(self.message, "{}", value);
        } else {
            let _ = write!(self.message, " {}={}", field.name(), value);
        }
    }
}
