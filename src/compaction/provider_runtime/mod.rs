mod checkpoint_loader;
mod evidence;
mod generation_builder;
mod git_adapter;
mod git_writer;
mod object_loader;
mod orchestrator;

pub(crate) use checkpoint_loader::{load_checkpoint_objects, LoadedCheckpointObjects};
pub(crate) use evidence::{
    validate_provider_evidence, CanonicalMode, ControlMode, ProviderEvidenceInput, ProviderKind,
    ProviderRefLayout, SealedProviderEvidence, SubmissionMode,
};
pub(crate) use generation_builder::{
    GenerationObjectWriter, IntegratingGenerationPlanBuilder, CONTROL_OBJECT_PATH,
    MANIFEST_OBJECT_PATH,
};
pub(crate) use git_adapter::GitObjectProvider;
pub(crate) use object_loader::{
    load_untrusted_inbox, ExactObjectReader, LoadedInbox, ObjectKind, TreeObject,
};
pub(crate) use orchestrator::{
    orchestrate_generation, CasOutcome, ControlState, GenerationCandidate, GenerationPlanBuilder,
    GenerationProvider, GenerationRefKind,
};

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) enum RuntimeError {
    InvalidEvidence(&'static str),
    InvalidObject(&'static str),
    RefDrift,
    Provider(String),
    InvalidGeneration(&'static str),
    ConcurrentControlUpdate,
}

impl std::fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "protocol-v2 provider runtime error: {self:?}")
    }
}

impl std::error::Error for RuntimeError {}

#[cfg(test)]
#[path = "checkpoint_loader_tests.rs"]
mod checkpoint_loader_tests;

#[cfg(test)]
mod tests;

#[cfg(test)]
#[path = "generation_builder_tests.rs"]
mod generation_builder_tests;
