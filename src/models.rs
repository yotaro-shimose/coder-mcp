use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ExecuteBashRequest {
    pub command: String,
    pub cwd: Option<String>,
    pub timeout: Option<u64>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "kind")]
pub enum BashEvent {
    BashCommand(BashCommand),
    BashOutput(BashOutput),
}

impl BashEvent {
    pub fn timestamp(&self) -> DateTime<Utc> {
        match self {
            BashEvent::BashCommand(c) => c.timestamp,
            BashEvent::BashOutput(o) => o.timestamp,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct BashCommand {
    pub id: Uuid,
    pub timestamp: DateTime<Utc>,
    pub command: String,
    pub cwd: Option<String>,
    pub timeout: u64,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct BashOutput {
    pub id: Uuid,
    pub timestamp: DateTime<Utc>,
    pub command_id: Uuid,
    pub order: i32,
    pub exit_code: Option<i32>,
    pub stdout: Option<String>,
    pub stderr: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct BashEventPage {
    pub items: Vec<BashEvent>,
    pub next_page_id: Option<String>,
}
