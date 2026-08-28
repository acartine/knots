use std::collections::BTreeMap;

use super::super::{ControlRecord, GenerationArtifacts};
use super::git_adapter::GitObjectProvider;
use super::object_loader::valid_oid;
use super::{GenerationObjectWriter, RuntimeError, CONTROL_OBJECT_PATH, MANIFEST_OBJECT_PATH};

impl GitObjectProvider {
    pub(crate) fn write_commit(
        &self,
        files: BTreeMap<String, Vec<u8>>,
        parent: Option<&str>,
        message: &str,
    ) -> Result<String, RuntimeError> {
        let mut tree = WriteTree::default();
        for (path, bytes) in files {
            let oid = parse_oid(self.git_input(&["hash-object", "-w", "--stdin"], &bytes)?)?;
            tree.insert(&path, oid)?;
        }
        let tree_oid = tree.write(self)?;
        let mut args = vec!["commit-tree", tree_oid.as_str()];
        if let Some(parent) = parent {
            if !valid_oid(parent) {
                return Err(RuntimeError::InvalidGeneration("invalid commit parent"));
            }
            args.extend(["-p", parent]);
        }
        parse_oid(self.git_input(&args, message.as_bytes())?)
    }
}

impl GenerationObjectWriter for GitObjectProvider {
    fn write_generation(
        &mut self,
        artifacts: &GenerationArtifacts,
        parent_oid: Option<&str>,
    ) -> Result<String, RuntimeError> {
        let mut files = BTreeMap::new();
        insert_json(&mut files, MANIFEST_OBJECT_PATH, &artifacts.manifest)?;
        files.insert(
            artifacts.manifest.snapshots.active.path.clone(),
            artifacts.active_snapshot.clone(),
        );
        files.insert(
            artifacts.manifest.snapshots.cold.path.clone(),
            artifacts.cold_snapshot.clone(),
        );
        files.insert(
            artifacts.manifest.projections.path.clone(),
            artifacts.projections.clone(),
        );
        files.insert(
            artifacts.pack.descriptor.path.clone(),
            artifacts.pack.compressed.clone(),
        );
        self.write_commit(files, parent_oid, "knots: build compacted generation\n")
    }

    fn write_control(&mut self, control: &ControlRecord) -> Result<String, RuntimeError> {
        let mut files = BTreeMap::new();
        insert_json(&mut files, CONTROL_OBJECT_PATH, control)?;
        self.write_commit(
            files,
            control.previous_control_head.as_deref(),
            "knots: activate compacted generation\n",
        )
    }
}

#[derive(Default)]
struct WriteTree {
    blobs: BTreeMap<String, String>,
    trees: BTreeMap<String, WriteTree>,
}

impl WriteTree {
    fn insert(&mut self, path: &str, oid: String) -> Result<(), RuntimeError> {
        let mut parts = path.split('/');
        let Some(first) = parts.next() else {
            return Err(RuntimeError::InvalidGeneration("empty generation path"));
        };
        if first.is_empty() || first == "." || first == ".." || first.contains(['\n', '\t']) {
            return Err(RuntimeError::InvalidGeneration("invalid generation path"));
        }
        let remainder = parts.collect::<Vec<_>>().join("/");
        if remainder.is_empty() {
            if self.trees.contains_key(first) || self.blobs.insert(first.to_string(), oid).is_some()
            {
                return Err(RuntimeError::InvalidGeneration("duplicate generation path"));
            }
        } else {
            if self.blobs.contains_key(first) {
                return Err(RuntimeError::InvalidGeneration("generation path collision"));
            }
            self.trees
                .entry(first.to_string())
                .or_default()
                .insert(&remainder, oid)?;
        }
        Ok(())
    }

    fn write(&self, provider: &GitObjectProvider) -> Result<String, RuntimeError> {
        let mut entries = Vec::new();
        for (name, oid) in &self.blobs {
            entries.extend_from_slice(format!("100644 blob {oid}\t{name}\n").as_bytes());
        }
        for (name, tree) in &self.trees {
            let oid = tree.write(provider)?;
            entries.extend_from_slice(format!("040000 tree {oid}\t{name}\n").as_bytes());
        }
        parse_oid(provider.git_input(&["mktree"], &entries)?)
    }
}

fn insert_json(
    files: &mut BTreeMap<String, Vec<u8>>,
    path: &str,
    value: &impl serde::Serialize,
) -> Result<(), RuntimeError> {
    let bytes = serde_json::to_vec(value)
        .map_err(|_| RuntimeError::InvalidGeneration("generation object encoding failed"))?;
    files.insert(path.to_string(), bytes);
    Ok(())
}

fn parse_oid(bytes: Vec<u8>) -> Result<String, RuntimeError> {
    let oid = String::from_utf8(bytes)
        .map_err(|_| RuntimeError::Provider("git returned a non-UTF-8 OID".to_string()))?
        .trim()
        .to_string();
    valid_oid(&oid)
        .then_some(oid)
        .ok_or_else(|| RuntimeError::Provider("git returned an invalid OID".to_string()))
}

#[cfg(test)]
mod tests {
    use super::{parse_oid, RuntimeError, WriteTree};

    const OID: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    #[test]
    fn write_tree_rejects_invalid_duplicate_and_colliding_paths() {
        for path in ["", "/root", "./file", "../file", "bad\nname", "bad\tname"] {
            let mut tree = WriteTree::default();
            assert!(matches!(
                tree.insert(path, OID.to_string()),
                Err(RuntimeError::InvalidGeneration(_))
            ));
        }

        let mut duplicate = WriteTree::default();
        duplicate.insert("file", OID.to_string()).unwrap();
        assert!(duplicate.insert("file", OID.to_string()).is_err());

        let mut collision = WriteTree::default();
        collision.insert("directory", OID.to_string()).unwrap();
        assert!(collision.insert("directory/file", OID.to_string()).is_err());
    }

    #[test]
    fn oid_parser_accepts_trimmed_hex_and_rejects_bad_output() {
        assert_eq!(parse_oid(format!("{OID}\n").into_bytes()).unwrap(), OID);
        assert!(parse_oid(b"not-an-oid".to_vec()).is_err());
        assert!(parse_oid(vec![0xff]).is_err());
    }
}
