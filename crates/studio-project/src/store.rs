//! Project persistence: atomic saves, autosave, rotating backups, migrations,
//! crash recovery and missing-media detection.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;
use studio_core::{ErrorCode, StudioError, StudioResult};

use crate::model::{now_secs, Project, PROJECT_SCHEMA_VERSION};

pub const PROJECT_EXT: &str = "aesproj";
const MAX_BACKUPS: usize = 10;

/// Write bytes atomically: temp file in the same directory, flush, rename over the target.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> StudioResult<()> {
    let dir = path
        .parent()
        .ok_or_else(|| StudioError::invalid_input("Invalid project path."))?;
    fs::create_dir_all(dir)?;
    let tmp = dir.join(format!(
        ".{}.tmp-{}",
        path.file_name().unwrap_or_default().to_string_lossy(),
        std::process::id()
    ));
    {
        use std::io::Write;
        let mut f = fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    fs::rename(&tmp, path)?;
    Ok(())
}

fn autosave_path(file: &Path) -> PathBuf {
    file.with_extension(format!("{PROJECT_EXT}.autosave"))
}

fn backups_dir(file: &Path) -> PathBuf {
    file.parent().unwrap_or(Path::new(".")).join("backups")
}

/// Save to the project's file (manual save). Keeps a rotating backup of the previous version.
pub fn save(project: &mut Project) -> StudioResult<PathBuf> {
    let file = project
        .file
        .clone()
        .ok_or_else(|| StudioError::invalid_input("The project has no file location yet."))?;
    project.touch();
    if file.exists() {
        let bdir = backups_dir(&file);
        fs::create_dir_all(&bdir)?;
        let _ = fs::copy(
            &file,
            bdir.join(format!("project-{}.{PROJECT_EXT}", now_secs())),
        );
        let mut entries: Vec<_> = fs::read_dir(&bdir)?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .collect();
        entries.sort();
        while entries.len() > MAX_BACKUPS {
            let _ = fs::remove_file(entries.remove(0));
        }
    }
    let json = serde_json::to_vec_pretty(project)?;
    atomic_write(&file, &json)?;
    let _ = fs::remove_file(autosave_path(&file));
    Ok(file)
}

/// Autosave writes a sibling file and never touches the main file.
pub fn autosave(project: &Project) -> StudioResult<()> {
    let file = project
        .file
        .clone()
        .ok_or_else(|| StudioError::invalid_input("The project has no file location yet."))?;
    atomic_write(&autosave_path(&file), &serde_json::to_vec(project)?)
}

/// Upgrade older project JSON to the current schema.
pub fn migrate(mut v: Value) -> StudioResult<Value> {
    let ver = v
        .get("schema_version")
        .and_then(|x| x.as_u64())
        .unwrap_or(0) as u32;
    if ver > PROJECT_SCHEMA_VERSION {
        return Err(StudioError::new(
            ErrorCode::Validation,
            "This project was created by a newer version of the application.",
        ));
    }
    if ver == 0 {
        // v0 (pre-release drafts) had no renders/plan fields and stored `title` instead of `name`.
        if let Some(obj) = v.as_object_mut() {
            if !obj.contains_key("name") {
                let title = obj
                    .remove("title")
                    .unwrap_or(Value::String("Untitled".into()));
                obj.insert("name".into(), title);
            }
            obj.entry("renders").or_insert(Value::Array(vec![]));
            obj.insert("schema_version".into(), Value::from(1));
        }
    }
    Ok(v)
}

/// Whether an incomplete render (`<stem>.partial.mp4`) exists next to `output`.
pub fn partial_exists(output: &Path) -> bool {
    studio_render::pipeline::partial_path(output).exists()
}

pub struct LoadResult {
    pub project: Project,
    /// True when a newer autosave exists than the saved file (crash recovery available).
    pub autosave_available: bool,
    pub missing_assets: Vec<String>,
}

pub fn load(file: &Path) -> StudioResult<LoadResult> {
    let bytes = fs::read(file).map_err(|_| {
        StudioError::not_found("The project file could not be opened.")
            .with_details(file.display().to_string())
    })?;
    let v: Value = serde_json::from_slice(&bytes).map_err(|e| {
        StudioError::new(ErrorCode::Validation, "The project file is damaged.")
            .with_details(e.to_string())
    })?;
    let v = migrate(v)?;
    let mut project: Project = serde_json::from_value(v).map_err(|e| {
        StudioError::new(ErrorCode::Validation, "The project file is damaged.")
            .with_details(e.to_string())
    })?;
    project.file = Some(file.to_path_buf());
    let auto = autosave_path(file);
    let autosave_available = auto.exists()
        && fs::metadata(&auto).and_then(|m| m.modified()).ok()
            > fs::metadata(file).and_then(|m| m.modified()).ok();
    let missing_assets = refresh_missing(&mut project);
    Ok(LoadResult {
        project,
        autosave_available,
        missing_assets,
    })
}

/// Load the autosave (crash recovery).
pub fn load_autosave(file: &Path) -> StudioResult<Project> {
    let r = load(&autosave_path(file))?;
    let mut p = r.project;
    p.file = Some(file.to_path_buf());
    Ok(p)
}

/// Mark assets whose files are gone; returns their ids.
pub fn refresh_missing(project: &mut Project) -> Vec<String> {
    let mut missing = vec![];
    for a in project.assets.iter_mut() {
        a.missing = !a.path.is_file();
        if a.missing {
            missing.push(a.id.clone());
        }
    }
    missing
}

/// Relink a missing asset to a new path after verifying it is the same content.
pub fn relink(
    project: &mut Project,
    asset_id: &str,
    new_path: &Path,
    allow_different: bool,
) -> StudioResult<()> {
    let path = studio_media::validate_input_path(new_path)?;
    let fp = studio_media::fingerprint(&path)?;
    let a = project
        .assets
        .iter_mut()
        .find(|a| a.id == asset_id)
        .ok_or_else(|| StudioError::not_found("Asset not found."))?;
    if fp != a.fingerprint && !allow_different {
        return Err(StudioError::validation(
            "The selected file has different content than the original media.",
        ));
    }
    a.path = path;
    a.fingerprint = fp;
    a.missing = false;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Project, ProjectSettings};

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join("studio-project-tests").join(name);
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn save_load_roundtrip_and_backups() {
        let d = dir("roundtrip");
        let mut p = Project::new("Test", ProjectSettings::default());
        p.file = Some(d.join("p.aesproj"));
        save(&mut p).unwrap();
        p.name = "Renamed".into();
        save(&mut p).unwrap();
        let r = load(&d.join("p.aesproj")).unwrap();
        assert_eq!(r.project.name, "Renamed");
        assert_eq!(r.project.timeline, p.timeline);
        assert!(fs::read_dir(d.join("backups")).unwrap().count() >= 1);
        assert!(!r.autosave_available);
    }

    #[test]
    fn autosave_recovery() {
        let d = dir("autosave");
        let mut p = Project::new("A", ProjectSettings::default());
        p.file = Some(d.join("a.aesproj"));
        save(&mut p).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1100));
        p.name = "Unsaved change".into();
        autosave(&p).unwrap();
        let r = load(&d.join("a.aesproj")).unwrap();
        assert!(r.autosave_available);
        assert_eq!(
            load_autosave(&d.join("a.aesproj")).unwrap().name,
            "Unsaved change"
        );
    }

    #[test]
    fn migrates_v0_and_rejects_future() {
        let p = Project::new("x", ProjectSettings::default());
        let mut v = serde_json::to_value(&p).unwrap();
        let o = v.as_object_mut().unwrap();
        o.remove("name");
        o.remove("renders");
        o.insert("title".into(), "Old".into());
        o.insert("schema_version".into(), 0.into());
        let m = migrate(v).unwrap();
        let back: Project = serde_json::from_value(m).unwrap();
        assert_eq!(back.name, "Old");
        let mut fut = serde_json::to_value(&p).unwrap();
        fut["schema_version"] = 99.into();
        assert!(migrate(fut).is_err());
    }

    #[test]
    fn damaged_file() {
        let d = dir("damaged");
        fs::write(d.join("bad.aesproj"), b"{not json").unwrap();
        assert_eq!(
            load(&d.join("bad.aesproj")).err().unwrap().code,
            ErrorCode::Validation
        );
    }
}
