use std::collections::HashSet;

use crate::db;
use crate::hierarchy_alias::{build_alias_maps, AliasMaps};
use crate::knot_id::{generate_knot_id, generate_knot_id_from_slug};
use crate::workflow_runtime;

use super::error::AppError;
use super::types::KnotView;
use super::App;

impl App {
    pub(super) fn known_knot_ids(&self) -> Result<HashSet<String>, AppError> {
        let mut ids = HashSet::new();
        for r in crate::trace::measure("alias_scan_hot", || db::list_knot_hot(&self.conn))? {
            ids.insert(r.id);
        }
        for r in crate::trace::measure("alias_scan_warm", || db::list_knot_warm(&self.conn))? {
            ids.insert(r.id);
        }
        for r in crate::trace::measure("alias_scan_cold", || db::list_cold_catalog(&self.conn))? {
            ids.insert(r.id);
        }
        Ok(ids)
    }

    pub(super) fn alias_maps(&self) -> Result<AliasMaps, AppError> {
        crate::trace::measure("alias_resolve", || {
            let mut ids = self.known_knot_ids()?;
            let parent_edges = crate::trace::measure("alias_scan_edges", || {
                db::list_edges_by_kind(&self.conn, "parent_of")
            })?;
            let mut edges = Vec::new();
            for edge in parent_edges {
                ids.insert(edge.src.clone());
                ids.insert(edge.dst.clone());
                edges.push((edge.src, edge.dst));
            }
            Ok(build_alias_maps(ids.into_iter().collect(), &edges))
        })
    }

    pub(super) fn resolve_knot_token(&self, token: &str) -> Result<String, AppError> {
        if token.trim().is_empty() {
            return Ok(token.to_string());
        }
        let maps = self.alias_maps()?;
        if let Some(id) = maps.alias_to_id.get(token) {
            return Ok(id.clone());
        }
        resolve_knot_suffix(token, &maps)
    }

    pub(super) fn with_alias_maps(knot: KnotView, maps: &AliasMaps) -> KnotView {
        let mut knot = knot;
        let alias = maps.id_to_alias.get(&knot.id).cloned();
        knot.alias = alias.filter(|value| value != &knot.id);
        knot
    }

    pub(super) fn apply_aliases_to_knots(
        &self,
        knots: Vec<KnotView>,
    ) -> Result<Vec<KnotView>, AppError> {
        let maps = self.alias_maps()?;
        Ok(knots
            .into_iter()
            .map(|k| Self::with_alias_maps(k, &maps))
            .collect())
    }

    pub(super) fn apply_alias_to_knot(&self, knot: KnotView) -> Result<KnotView, AppError> {
        let maps = self.alias_maps()?;
        Ok(Self::with_alias_maps(knot, &maps))
    }

    pub(super) fn apply_alias_and_enrich_knot(&self, knot: KnotView) -> Result<KnotView, AppError> {
        let maps = self.alias_maps()?;
        let mut knot = Self::with_alias_maps(knot, &maps);
        workflow_runtime::enrich_step_metadata(&mut knot, &self.profile_registry);
        Ok(knot)
    }

    pub(super) fn next_knot_id(&self) -> Result<String, AppError> {
        let existing = self.known_knot_ids()?;
        Ok(match self.project_id.as_deref() {
            Some(pid) => generate_knot_id_from_slug(pid, |c| existing.contains(c)),
            None => generate_knot_id(&self.repo_root, |c| existing.contains(c)),
        })
    }
}

fn resolve_knot_suffix(token: &str, maps: &AliasMaps) -> Result<String, AppError> {
    let suffix_part = token.split('.').next().unwrap_or(token);
    let mut suffix_matches = maps
        .id_to_alias
        .keys()
        .filter_map(|id| {
            id.rsplit_once('-')
                .filter(|(_, s)| *s == suffix_part)
                .map(|_| id.clone())
        })
        .collect::<Vec<_>>();
    if !token.contains('.') {
        return match suffix_matches.len() {
            0 => Ok(token.to_string()),
            1 => Ok(suffix_matches.remove(0)),
            _ => {
                suffix_matches.sort();
                Err(AppError::InvalidArgument(format!(
                    "ambiguous knot id '{}'; matches: {}",
                    token,
                    suffix_matches.join(", ")
                )))
            }
        };
    }
    resolve_hierarchical_alias(token, suffix_part, &suffix_matches, maps)
}

fn resolve_hierarchical_alias(
    token: &str,
    suffix_part: &str,
    suffix_matches: &[String],
    maps: &AliasMaps,
) -> Result<String, AppError> {
    let dot_tail = &token[suffix_part.len()..];
    if suffix_matches.is_empty() {
        return Ok(token.to_string());
    }
    let mut resolved: Vec<String> = suffix_matches
        .iter()
        .filter_map(|pfx| {
            let full = format!("{}{}", pfx, dot_tail);
            maps.alias_to_id.get(&full).cloned()
        })
        .collect();
    resolved.sort();
    resolved.dedup();
    match resolved.len() {
        0 => Err(AppError::NotFound(token.to_string())),
        1 => Ok(resolved.remove(0)),
        _ => Err(AppError::InvalidArgument(format!(
            "ambiguous knot alias '{}'; matches: {}",
            token,
            resolved.join(", ")
        ))),
    }
}
