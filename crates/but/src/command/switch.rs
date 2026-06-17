use bstr::ByteSlice;
use gix::refs::{Category, FullName};

use crate::{CliId, CliResult, IdMap, args::atoms::CliIdArg, bad_input, utils::OutputChannel};

pub fn handle(
    ctx: &mut but_ctx::Context,
    out: &mut OutputChannel,
    target: Option<CliIdArg>,
    workspace: bool,
    new: bool,
) -> CliResult<()> {
    let mut guard = ctx.exclusive_worktree_access();

    if workspace {
        but_api::branch::workspace_checkout_with_perm(ctx, guard.write_permission())?;
        if let Some(out) = out.for_human() {
            writeln!(out, "Switched to workspace")?;
        }
        return Ok(());
    }

    if new {
        let requested_name = target.map(|target| target.0);
        but_api::branch::branch_checkout_new_with_perm(
            ctx,
            requested_name,
            guard.write_permission(),
        )?;
        let branch_name = current_head_short_name(ctx)?;
        if let Some(out) = out.for_human() {
            writeln!(out, "Created and switched to branch '{branch_name}'")?;
        }
        return Ok(());
    }

    let target = target
        .ok_or_else(|| anyhow::anyhow!("BUG: clap requires target, --workspace, or --new"))?;
    let branch = resolve_existing_local_branch(ctx, guard.read_permission(), &target)?;
    but_api::branch::branch_checkout_with_perm(ctx, branch.clone(), guard.write_permission())?;

    if let Some(out) = out.for_human() {
        writeln!(out, "Switched to branch '{}'", branch.shorten())?;
    }
    Ok(())
}

fn resolve_existing_local_branch(
    ctx: &but_ctx::Context,
    perm: &but_core::sync::RepoShared,
    target: &CliIdArg,
) -> CliResult<FullName> {
    let repo = ctx.repo.get()?;

    if target.0.starts_with("refs/heads/") {
        let full_name = FullName::try_from(target.0.as_str())
            .map_err(|_| bad_input(format!("Invalid branch ref '{}'", target.0)))?;
        ensure_existing_local_branch(&repo, &full_name)?;
        return Ok(full_name);
    }

    if target.0.starts_with("refs/remotes/") || looks_like_remote_branch(&repo, &target.0) {
        return Err(bad_input(format!(
            "Can only switch to local branches, got '{}'",
            target.0
        ))
        .into());
    }

    if let Ok(short_name) = Category::LocalBranch.to_full_name(target.0.as_str())
        && repo.try_find_reference(short_name.as_ref())?.is_some()
    {
        return Ok(short_name);
    }

    let id_map = IdMap::new_from_context(ctx, None, perm)?;
    let matches = id_map.parse_using_context(&target.0, ctx)?;
    if matches.is_empty() {
        return Err(bad_input(format!("Could not find branch: '{}'", target.0)).into());
    }
    if matches.len() > 1 {
        return Err(anyhow::anyhow!(
            "Branch '{}' is ambiguous. Try using more characters to disambiguate.",
            target.0
        )
        .into());
    }

    match &matches[0] {
        CliId::Branch { name, .. } => {
            let branch = Category::LocalBranch.to_full_name(name.as_str())?;
            ensure_existing_local_branch(&repo, &branch)?;
            Ok(branch)
        }
        other => {
            let kind = match other {
                CliId::Branch { .. } => unreachable!("handled above"),
                CliId::Commit { .. } => "a commit",
                CliId::Uncommitted(..) => "an uncommitted file",
                CliId::PathPrefix { .. } => "a path",
                CliId::CommittedFile { .. } => "a committed file",
                CliId::Unassigned { .. } => "unassigned changes",
                CliId::Stack { .. } => "a stack",
            };
            Err(bad_input(format!("Invalid branch. '{}' is {kind}", target.0)).into())
        }
    }
}

fn ensure_existing_local_branch(repo: &gix::Repository, branch: &FullName) -> CliResult<()> {
    if !branch.as_bstr().starts_with_str("refs/heads/") {
        return Err(bad_input(format!("Can only switch to local branches, got '{branch}'")).into());
    }
    if repo.try_find_reference(branch.as_ref())?.is_none() {
        return Err(bad_input(format!("Branch '{}' not found", branch.shorten())).into());
    }
    Ok(())
}

fn looks_like_remote_branch(repo: &gix::Repository, target: &str) -> bool {
    repo.remote_names().iter().any(|remote| {
        target
            .as_bytes()
            .strip_prefix(remote.as_bstr().as_bytes())
            .is_some_and(|rest| rest.starts_with(b"/"))
    })
}

fn current_head_short_name(ctx: &but_ctx::Context) -> CliResult<String> {
    let repo = ctx.repo.get()?;
    let head_name = repo
        .head_name()?
        .ok_or_else(|| anyhow::anyhow!("HEAD is detached after switching branches"))?;
    Ok(head_name.shorten().to_string())
}
