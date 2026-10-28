//! Business-logic services. Each submodule owns one resource domain
//! (projects, files, comments, members, invites, versions, …) and is
//! the only place that builds SQL or talks to storage for that domain.
//! Route handlers translate HTTP into service calls and back.

pub mod ai;
pub mod inline_prompt;
pub mod project_context;
pub mod project_agent;
pub mod comments;
pub mod compiles;
pub mod exports;
pub mod files;
pub mod invites;
pub mod members;
pub mod membership;
pub mod notifications;
pub mod projects;
pub mod shares;
pub mod templates;
pub mod versions;

pub use comments::CommentService;
pub use files::FileService;
pub use invites::InviteService;
pub use members::MemberService;
pub use notifications::NotificationService;
pub use projects::ProjectService;
pub use shares::ShareService;
pub use versions::VersionService;

pub mod welcome;

pub mod ai_context;
pub mod project_actions;

pub mod ai_profiles;

pub mod project_progress;

pub mod version_diff;

pub mod version_summaries;

pub mod tex_proposal;

pub mod context_compaction;

pub mod agent_plan;

pub mod agent_loop_guard;

pub mod web_search;

pub mod archive_import;

pub mod agent_library;

pub mod agent_workbench;

pub mod search_connections;

pub mod skill_install;

pub mod skill_import;

pub mod builtin_skills;

pub mod agent_history;
