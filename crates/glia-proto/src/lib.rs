//! glia-proto — shared wire-frame types for the glia CLI/bridge ↔ Hub protocol.
//!
//! Frames are newline-delimited JSON (NDJSON). Each message is exactly one
//! UTF-8 JSON object followed by a newline. A UUID-style `id` field correlates
//! requests and responses so a single WebSocket connection can multiplex
//! concurrent actions.
//!
//! The bridge's existing line-pump already handles NDJSON framing; these types
//! plug directly into that transport.

use serde::{Deserialize, Serialize};

/// A frame sent from the CLI/bridge **to** the Hub.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientFrame {
    /// Submit an intent for the Hub to classify, discover tools, and execute.
    Action {
        /// Correlation ID (echoed in the matching `ServerFrame`).
        id: String,
        /// Natural-language intent query (e.g., "create a Linear issue for the login bug").
        intent: String,
        /// Optional stack filter (e.g., `"nextjs"`). `None` = no filter.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        stack: Option<String>,
        /// Working-directory path on the client device. Used when the Hub
        /// needs to send a `RunLocal` frame back to the device.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cwd: Option<String>,
    },
    /// Result of a `RunLocal` tool execution — sent back to the Hub after the
    /// device ran the requested command.
    LocalResult {
        /// Correlation ID matching the `RunLocal` frame.
        id: String,
        /// Standard output captured from the local command.
        stdout: String,
        /// Standard error captured from the local command.
        stderr: String,
        /// Exit code from the local command.
        exit_code: i32,
    },
}

/// A frame sent from the Hub **to** the CLI/bridge.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerFrame {
    /// The action completed (success or structured tool result).
    Result {
        /// Correlation ID matching the originating `ClientFrame::Action`.
        id: String,
        /// Execution outcome as a free-form JSON value.
        outcome: serde_json::Value,
    },
    /// The Hub requires one or more credentials before it can run the action.
    AuthRequired {
        /// Correlation ID.
        id: String,
        /// Missing credential dependencies. Client should start OAuth for each.
        deps: Vec<MissingDep>,
    },
    /// The Hub is requesting the device to run a local tool on its behalf.
    RunLocal {
        /// Correlation ID. Device must echo this in its `LocalResult`.
        id: String,
        /// Shell command to run on the device.
        command: String,
        /// Regex allow-list patterns passed to `glia-bash` for safety.
        allow_patterns: Vec<String>,
        /// Working-directory root under which the command is confined.
        root: String,
    },
    /// A required runtime is missing or too old on the executing host.
    RuntimeMissing {
        /// Correlation ID.
        id: String,
        /// Runtime binary name (e.g., `"uvx"`, `"npx"`, `"docker"`).
        runtime: String,
        /// Minimum required version string, if known (e.g., `"0.4.0"`).
        needed_version: Option<String>,
        /// Human-readable install hint for the agent or user.
        hint: String,
    },
    /// The Hub's skill/tool graph changed; the device should re-render its
    /// agent config files (`.cursor/rules`, `.claude/settings.json`).
    ConfigChanged {
        /// Human-readable reason for the change.
        reason: String,
        /// The skill or tool ID that changed, if applicable.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        skill_id: Option<String>,
    },
    /// An error occurred on the Hub side.
    Error {
        /// Correlation ID.
        id: String,
        /// Machine-readable error code (e.g., `"HELIX_ERR"`, `"EXEC_ERR"`).
        code: String,
        /// Human-readable error message.
        message: String,
    },
}

/// A missing credential dependency, paired with the tool that requires it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MissingDep {
    /// Name of the tool that needs the credential.
    pub tool: String,
    /// Credential (auth record) ID in HelixDB.
    pub cred: String,
}
