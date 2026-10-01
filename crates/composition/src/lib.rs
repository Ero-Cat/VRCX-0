mod composition;
mod context;
mod error;
mod event_sink;
mod group_order;
mod profile;
mod state;

pub use composition::{RuntimeHostComposition, RuntimeHostProfileExtension};
pub(crate) use context::RuntimeHostContext;
pub use context::RuntimeHostDesktopAssemblyDeps;
pub use error::{Error, Result};
pub use event_sink::RuntimeHostEventSink;
pub use group_order::{GroupOrderSource, UnavailableGroupOrderSource};
pub use profile::RuntimeHostProfile;
pub use state::{
    BackendRuntimeCombinedSnapshot, CliLoginPrompt, CliTwoFactorChoice, RemoteSyncHost,
    RemoteSyncSettings, RuntimeHostOptions, RuntimeHostState, RuntimeHostStateBuilder,
};
