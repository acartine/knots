use std::path::PathBuf;

use crate::app::AppError;
use crate::cli::Cli;
use crate::project::{DistributionMode, ProjectContext, StorePaths};
use crate::write_queue::{self, QueuedWriteRequest, QueuedWriteResponse};

mod execute;
pub(crate) mod helpers;
mod operation_map;

pub(crate) use execute::execute_operation;
pub(crate) use operation_map::operation_from_command;

#[cfg(test)]
pub fn maybe_run_queued_command(cli: &Cli) -> Result<Option<String>, AppError> {
    let repo_root = cli.repo_root.clone().unwrap_or_else(|| PathBuf::from("."));
    let context = ProjectContext {
        project_id: None,
        repo_root: repo_root.clone(),
        store_paths: StorePaths {
            root: repo_root.join(".knots"),
        },
        distribution: DistributionMode::Git,
    };
    let db_path = cli
        .db
        .clone()
        .unwrap_or_else(|| ".knots/cache/state.sqlite".to_string());
    maybe_run_queued_command_with_context(cli, &context, &db_path)
}

pub fn maybe_run_queued_command_with_context(
    cli: &Cli,
    context: &ProjectContext,
    db_path: &str,
) -> Result<Option<String>, AppError> {
    let Some(operation) = operation_from_command(&cli.command) else {
        return Ok(None);
    };

    let response = write_queue::enqueue_and_wait_with_context(
        &context.repo_root,
        &context.store_paths,
        context.distribution,
        context.project_id.clone(),
        db_path,
        operation,
        execute_queued_request,
    )
    .map_err(|err| AppError::InvalidArgument(format!("write queue error: {}", err)))?;

    if response.success {
        Ok(Some(response.output))
    } else {
        Err(AppError::InvalidArgument(
            response
                .error
                .unwrap_or_else(|| "queued write failed".to_string()),
        ))
    }
}

fn execute_queued_request(request: &QueuedWriteRequest) -> QueuedWriteResponse {
    let context = ProjectContext {
        project_id: request.project_id.clone(),
        repo_root: PathBuf::from(&request.repo_root),
        store_paths: StorePaths {
            root: PathBuf::from(&request.store_root),
        },
        distribution: if request.distribution == "git" {
            DistributionMode::Git
        } else {
            DistributionMode::LocalOnly
        },
    };
    let app = match crate::app::App::open_with_context(&context, &request.db_path) {
        Ok(app) => app,
        Err(err) => return QueuedWriteResponse::failure(err.to_string()),
    };
    match execute_operation(&app, &request.operation) {
        Ok(output) => QueuedWriteResponse::success(output),
        Err(err) => QueuedWriteResponse::failure(err.to_string()),
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
#[path = "write_dispatch/tests_rollback_ext.rs"]
mod tests_rollback_ext;

#[cfg(test)]
#[path = "write_dispatch/tests_gate_ext.rs"]
mod tests_gate_ext;

#[cfg(test)]
#[path = "write_dispatch/tests_lease_ext.rs"]
mod tests_lease_ext;

#[cfg(test)]
#[path = "write_dispatch/tests_lease_ext2.rs"]
mod tests_lease_ext2;

#[cfg(test)]
#[path = "write_dispatch/tests_lease_ext3.rs"]
mod tests_lease_ext3;
