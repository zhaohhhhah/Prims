//! HTTP routes. Each submodule exposes a `router()` returning a
//! `Router<AppState>` to compose into the main app.

pub mod admin;
pub mod outlook_mail;
pub mod smtp_mail;
pub mod ai;
pub mod comments;
pub mod compiles;
pub mod exports;
pub mod files;
pub mod health;
pub mod invites;
pub mod lint;
pub mod members;
pub mod notifications;
pub mod projects;
pub mod shares;
pub mod versions;
pub mod voice;
pub mod whoami;
pub mod yjs;

pub mod ai_context;
pub mod project_actions;

pub mod ai_profiles;

pub mod version_summaries;

pub mod archives;

pub mod dashboard_summary;

pub mod avatars;

pub mod agent_library;

pub mod skill_import;
