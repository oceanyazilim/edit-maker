//! SQLite index: profiles, recent projects, analysis cache entries, render
//! history, the shared media store and per-profile asset references.
//! Analysis payloads live as JSON files in the cache directory; the database
//! stores keys, versions and paths.
//!
//! Profile isolation is enforced here: every profile-owned query takes a
//! `profile_id` and filters / constrains on it.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use studio_core::{ErrorCode, StudioError, StudioResult};

use crate::profile::{Profile, DEFAULT_PROFILE_ID};

fn db_err(e: rusqlite::Error) -> StudioError {
    StudioError::new(
        ErrorCode::Storage,
        "The local library database reported an error.",
    )
    .with_details(e.to_string())
}

const MIGRATIONS: &[&str] = &[
    // v1
    "CREATE TABLE IF NOT EXISTS projects (id TEXT PRIMARY KEY, name TEXT NOT NULL, path TEXT NOT NULL, modified_at INTEGER NOT NULL);
     CREATE TABLE IF NOT EXISTS media (fingerprint TEXT PRIMARY KEY, path TEXT NOT NULL, kind TEXT NOT NULL, size INTEGER, duration_s REAL, last_seen INTEGER NOT NULL);
     CREATE TABLE IF NOT EXISTS analysis_cache (key TEXT PRIMARY KEY, kind TEXT NOT NULL, version INTEGER NOT NULL, path TEXT NOT NULL, created_at INTEGER NOT NULL);
     CREATE TABLE IF NOT EXISTS renders (id TEXT PRIMARY KEY, project_id TEXT, output TEXT NOT NULL, created_at INTEGER NOT NULL, ok INTEGER NOT NULL, duration_s REAL, encoder TEXT);",
    // v2 – multi-profile workspaces. Existing rows are assigned to the default profile.
    "CREATE TABLE profiles (
        id TEXT PRIMARY KEY,
        name TEXT NOT NULL,
        archived INTEGER NOT NULL DEFAULT 0,
        data TEXT NOT NULL,
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL);
     CREATE INDEX idx_profiles_archived ON profiles(archived, name);
     INSERT INTO profiles (id, name, archived, data, created_at, updated_at)
        VALUES ('profile_default', 'Default', 0, '{\"id\":\"profile_default\",\"name\":\"Default\"}', strftime('%s','now'), strftime('%s','now'));
     CREATE TABLE app_state (key TEXT PRIMARY KEY, value TEXT NOT NULL);
     INSERT INTO app_state (key, value) VALUES ('active_profile', 'profile_default');
     ALTER TABLE projects ADD COLUMN profile_id TEXT NOT NULL DEFAULT 'profile_default' REFERENCES profiles(id) ON DELETE CASCADE;
     CREATE INDEX idx_projects_profile ON projects(profile_id, modified_at);
     ALTER TABLE renders ADD COLUMN profile_id TEXT NOT NULL DEFAULT 'profile_default' REFERENCES profiles(id) ON DELETE CASCADE;
     CREATE INDEX idx_renders_profile ON renders(profile_id, created_at);
     CREATE TABLE profile_assets (
        profile_id TEXT NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
        fingerprint TEXT NOT NULL REFERENCES media(fingerprint) ON DELETE CASCADE,
        shared INTEGER NOT NULL DEFAULT 0,
        added_at INTEGER NOT NULL,
        PRIMARY KEY (profile_id, fingerprint));
     CREATE INDEX idx_profile_assets_fp ON profile_assets(fingerprint);
     INSERT INTO profile_assets (profile_id, fingerprint, shared, added_at) SELECT 'profile_default', fingerprint, 0, last_seen FROM media;
     CREATE TABLE profile_favorite_presets (
        profile_id TEXT NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
        preset_id TEXT NOT NULL,
        PRIMARY KEY (profile_id, preset_id));
     CREATE TABLE trend_research (
        id TEXT PRIMARY KEY,
        profile_id TEXT NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
        created_at INTEGER NOT NULL,
        query TEXT NOT NULL,
        result TEXT NOT NULL);
     CREATE INDEX idx_trend_research_profile ON trend_research(profile_id, created_at);",
];

pub struct Library {
    conn: Mutex<Connection>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct RecentProject {
    pub id: String,
    pub name: String,
    pub path: PathBuf,
    pub modified_at: u64,
    pub profile_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct RenderHistoryEntry {
    pub id: String,
    pub project_id: Option<String>,
    pub output: PathBuf,
    pub created_at: u64,
    pub ok: bool,
    pub duration_s: f64,
    pub encoder: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ProfileAsset {
    pub fingerprint: String,
    pub path: PathBuf,
    pub kind: String,
    pub duration_s: f64,
    /// Visible to all profiles.
    pub shared: bool,
    /// Profile that holds the reference (may differ from the asking profile for shared assets).
    pub owner_profile: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ProfileStats {
    pub projects: u64,
    pub renders: u64,
    pub assets: u64,
}

/// What deleting a profile removes (shown to the user before confirming).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct DeleteReport {
    pub project_entries_removed: u64,
    pub render_entries_removed: u64,
    pub asset_references_removed: u64,
    /// Media index rows no longer referenced by any profile (files on disk are never deleted).
    pub orphaned_media_entries: u64,
}

fn now() -> u64 {
    crate::model::now_secs()
}

impl Library {
    pub fn open(path: &Path) -> StudioResult<Library> {
        if let Some(d) = path.parent() {
            std::fs::create_dir_all(d)?;
        }
        let conn = Connection::open(path).map_err(db_err)?;
        Self::init(conn)
    }
    pub fn open_in_memory() -> StudioResult<Library> {
        Self::init(Connection::open_in_memory().map_err(db_err)?)
    }
    fn init(mut conn: Connection) -> StudioResult<Library> {
        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(db_err)?;
        // ALTER TABLE ... REFERENCES with a non-NULL default requires FK enforcement off.
        conn.pragma_update(None, "foreign_keys", "OFF")
            .map_err(db_err)?;
        let ver: u32 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .map_err(db_err)?;
        for (i, m) in MIGRATIONS.iter().enumerate().skip(ver as usize) {
            let tx = conn.transaction().map_err(db_err)?;
            tx.execute_batch(m).map_err(db_err)?;
            tx.pragma_update(None, "user_version", (i + 1) as u32)
                .map_err(db_err)?;
            tx.commit().map_err(db_err)?;
        }
        conn.pragma_update(None, "foreign_keys", "ON")
            .map_err(db_err)?;
        let lib = Library {
            conn: Mutex::new(conn),
        };
        lib.normalize_default_profile()?;
        Ok(lib)
    }

    /// The v2 migration stores a minimal JSON document; expand it to a full profile.
    fn normalize_default_profile(&self) -> StudioResult<()> {
        if let Some(mut p) = self.get_profile(DEFAULT_PROFILE_ID)? {
            if p.created_at == 0 {
                p.created_at = now();
                p.updated_at = p.created_at;
                self.write_profile(&p)?;
            }
        }
        Ok(())
    }

    // ---------------- profiles ----------------

    fn write_profile(&self, p: &Profile) -> StudioResult<()> {
        let data = serde_json::to_string(p)?;
        self.conn
            .lock()
            .unwrap()
            .execute(
                "INSERT INTO profiles (id, name, archived, data, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(id) DO UPDATE SET name = excluded.name, archived = excluded.archived, data = excluded.data, updated_at = excluded.updated_at",
                params![p.id, p.name, p.archived as i32, data, p.created_at as i64, p.updated_at as i64],
            )
            .map_err(db_err)?;
        Ok(())
    }

    pub fn get_profile(&self, id: &str) -> StudioResult<Option<Profile>> {
        let c = self.conn.lock().unwrap();
        let row: Option<(String, i32)> = c
            .query_row(
                "SELECT data, archived FROM profiles WHERE id = ?1",
                params![id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(db_err)?;
        Ok(match row {
            Some((data, archived)) => {
                let mut p: Profile = serde_json::from_str(&data)?;
                p.id = id.to_string();
                p.archived = archived != 0;
                Some(p)
            }
            None => None,
        })
    }

    pub fn list_profiles(
        &self,
        include_archived: bool,
        search: &str,
    ) -> StudioResult<Vec<Profile>> {
        let ids: Vec<String> = {
            let c = self.conn.lock().unwrap();
            let mut st = c
                .prepare("SELECT id FROM profiles WHERE (?1 OR archived = 0) AND (?2 = '' OR name LIKE '%' || ?2 || '%' ESCAPE '\\') ORDER BY archived, name COLLATE NOCASE")
                .map_err(db_err)?;
            let escaped = search
                .replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_");
            let rows = st
                .query_map(params![include_archived, escaped], |r| {
                    r.get::<_, String>(0)
                })
                .map_err(db_err)?;
            rows.collect::<Result<Vec<_>, _>>().map_err(db_err)?
        };
        let mut out = Vec::with_capacity(ids.len());
        for id in ids {
            if let Some(p) = self.get_profile(&id)? {
                out.push(p);
            }
        }
        Ok(out)
    }

    pub fn create_profile(&self, mut p: Profile) -> StudioResult<Profile> {
        p.validate()?;
        p.id = studio_core::ids::new_id("profile");
        p.name = p.name.trim().to_string();
        p.archived = false;
        p.created_at = now();
        p.updated_at = p.created_at;
        self.write_profile(&p)?;
        Ok(p)
    }

    pub fn update_profile(&self, p: &Profile) -> StudioResult<Profile> {
        p.validate()?;
        let existing = self
            .get_profile(&p.id)?
            .ok_or_else(|| StudioError::not_found("Profile not found."))?;
        let mut q = p.clone();
        q.name = q.name.trim().to_string();
        q.created_at = existing.created_at;
        q.archived = existing.archived;
        q.updated_at = now();
        self.write_profile(&q)?;
        Ok(q)
    }

    /// Duplicate settings, favourite presets and asset references (not projects or history).
    pub fn duplicate_profile(&self, id: &str, new_name: &str) -> StudioResult<Profile> {
        let src = self
            .get_profile(id)?
            .ok_or_else(|| StudioError::not_found("Profile not found."))?;
        let mut p = src.clone();
        p.name = new_name.trim().to_string();
        p.validate()?;
        p.id = studio_core::ids::new_id("profile");
        p.archived = false;
        p.created_at = now();
        p.updated_at = p.created_at;
        let data = serde_json::to_string(&p)?;
        let mut c = self.conn.lock().unwrap();
        let tx = c.transaction().map_err(db_err)?;
        tx.execute(
            "INSERT INTO profiles (id, name, archived, data, created_at, updated_at) VALUES (?1, ?2, 0, ?3, ?4, ?4)",
            params![p.id, p.name, data, p.created_at as i64],
        )
        .map_err(db_err)?;
        tx.execute("INSERT INTO profile_favorite_presets (profile_id, preset_id) SELECT ?1, preset_id FROM profile_favorite_presets WHERE profile_id = ?2", params![p.id, id])
            .map_err(db_err)?;
        tx.execute(
            "INSERT INTO profile_assets (profile_id, fingerprint, shared, added_at) SELECT ?1, fingerprint, 0, ?3 FROM profile_assets WHERE profile_id = ?2",
            params![p.id, id, p.created_at as i64],
        )
        .map_err(db_err)?;
        tx.commit().map_err(db_err)?;
        Ok(p)
    }

    pub fn set_archived(&self, id: &str, archived: bool) -> StudioResult<()> {
        if archived && self.active_profile_id()? == id {
            return Err(StudioError::validation(
                "Switch to another profile before archiving the active one.",
            ));
        }
        let mut p = self
            .get_profile(id)?
            .ok_or_else(|| StudioError::not_found("Profile not found."))?;
        p.archived = archived;
        p.updated_at = now();
        self.write_profile(&p)
    }

    pub fn profile_stats(&self, id: &str) -> StudioResult<ProfileStats> {
        let c = self.conn.lock().unwrap();
        let count = |sql: &str| -> StudioResult<u64> {
            c.query_row(sql, params![id], |r| r.get::<_, i64>(0))
                .map(|v| v as u64)
                .map_err(db_err)
        };
        Ok(ProfileStats {
            projects: count("SELECT COUNT(*) FROM projects WHERE profile_id = ?1")?,
            renders: count("SELECT COUNT(*) FROM renders WHERE profile_id = ?1")?,
            assets: count("SELECT COUNT(*) FROM profile_assets WHERE profile_id = ?1")?,
        })
    }

    /// Permanently delete a profile's database records (projects index, render history,
    /// asset references, favourites, research) in one transaction. Project files and
    /// media files on disk are never deleted. The profile must be archived first, and
    /// the default profile cannot be deleted.
    pub fn delete_profile(&self, id: &str) -> StudioResult<DeleteReport> {
        if id == DEFAULT_PROFILE_ID {
            return Err(StudioError::validation(
                "The default profile cannot be deleted.",
            ));
        }
        let p = self
            .get_profile(id)?
            .ok_or_else(|| StudioError::not_found("Profile not found."))?;
        if !p.archived {
            return Err(StudioError::validation(
                "Archive the profile before deleting it.",
            ));
        }
        let stats = self.profile_stats(id)?;
        let mut c = self.conn.lock().unwrap();
        let tx = c.transaction().map_err(db_err)?;
        // ON DELETE CASCADE removes projects, renders, asset refs, favourites, research.
        tx.execute("DELETE FROM profiles WHERE id = ?1", params![id])
            .map_err(db_err)?;
        let orphans: i64 = tx
            .query_row("SELECT COUNT(*) FROM media WHERE fingerprint NOT IN (SELECT fingerprint FROM profile_assets)", [], |r| r.get(0))
            .map_err(db_err)?;
        tx.commit().map_err(db_err)?;
        Ok(DeleteReport {
            project_entries_removed: stats.projects,
            render_entries_removed: stats.renders,
            asset_references_removed: stats.assets,
            orphaned_media_entries: orphans as u64,
        })
    }

    pub fn active_profile_id(&self) -> StudioResult<String> {
        let c = self.conn.lock().unwrap();
        let v: Option<String> = c
            .query_row(
                "SELECT value FROM app_state WHERE key = 'active_profile'",
                [],
                |r| r.get(0),
            )
            .optional()
            .map_err(db_err)?;
        Ok(v.unwrap_or_else(|| DEFAULT_PROFILE_ID.to_string()))
    }

    /// Persist the active profile (restored on restart). Falls back to the default
    /// profile when the stored one no longer exists or is archived.
    pub fn active_profile(&self) -> StudioResult<Profile> {
        let id = self.active_profile_id()?;
        match self.get_profile(&id)? {
            Some(p) if !p.archived => Ok(p),
            _ => {
                self.set_active_profile(DEFAULT_PROFILE_ID)?;
                self.get_profile(DEFAULT_PROFILE_ID)?
                    .ok_or_else(|| StudioError::internal("Default profile missing."))
            }
        }
    }

    pub fn set_active_profile(&self, id: &str) -> StudioResult<()> {
        let p = self
            .get_profile(id)?
            .ok_or_else(|| StudioError::not_found("Profile not found."))?;
        if p.archived {
            return Err(StudioError::validation(
                "Restore the profile before switching to it.",
            ));
        }
        self.conn
            .lock()
            .unwrap()
            .execute("INSERT INTO app_state (key, value) VALUES ('active_profile', ?1) ON CONFLICT(key) DO UPDATE SET value = excluded.value", params![id])
            .map_err(db_err)?;
        Ok(())
    }

    pub fn favorite_presets(&self, profile_id: &str) -> StudioResult<Vec<String>> {
        let c = self.conn.lock().unwrap();
        let mut st = c.prepare("SELECT preset_id FROM profile_favorite_presets WHERE profile_id = ?1 ORDER BY preset_id").map_err(db_err)?;
        let rows = st
            .query_map(params![profile_id], |r| r.get(0))
            .map_err(db_err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(db_err)
    }

    pub fn set_favorite_preset(
        &self,
        profile_id: &str,
        preset_id: &str,
        favorite: bool,
    ) -> StudioResult<()> {
        let c = self.conn.lock().unwrap();
        if favorite {
            c.execute("INSERT OR IGNORE INTO profile_favorite_presets (profile_id, preset_id) VALUES (?1, ?2)", params![profile_id, preset_id])
        } else {
            c.execute("DELETE FROM profile_favorite_presets WHERE profile_id = ?1 AND preset_id = ?2", params![profile_id, preset_id])
        }
        .map_err(db_err)?;
        Ok(())
    }

    // ---------------- projects ----------------

    /// Register/update a project for a profile. A project id already owned by a
    /// different profile is rejected (ownership never changes silently).
    pub fn upsert_project(
        &self,
        profile_id: &str,
        id: &str,
        name: &str,
        path: &Path,
        modified_at: u64,
    ) -> StudioResult<()> {
        let c = self.conn.lock().unwrap();
        let owner: Option<String> = c
            .query_row(
                "SELECT profile_id FROM projects WHERE id = ?1",
                params![id],
                |r| r.get(0),
            )
            .optional()
            .map_err(db_err)?;
        if let Some(o) = owner {
            if o != profile_id {
                return Err(StudioError::validation(
                    "This project belongs to another profile.",
                ));
            }
        }
        c.execute(
            "INSERT INTO projects (id, name, path, modified_at, profile_id) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(id) DO UPDATE SET name = excluded.name, path = excluded.path, modified_at = excluded.modified_at",
            params![id, name, path.to_string_lossy(), modified_at as i64, profile_id],
        )
        .map_err(db_err)?;
        Ok(())
    }

    pub fn project_owner(&self, project_id: &str) -> StudioResult<Option<String>> {
        let c = self.conn.lock().unwrap();
        c.query_row(
            "SELECT profile_id FROM projects WHERE id = ?1",
            params![project_id],
            |r| r.get(0),
        )
        .optional()
        .map_err(db_err)
    }

    pub fn recent_projects(
        &self,
        profile_id: &str,
        limit: usize,
    ) -> StudioResult<Vec<RecentProject>> {
        let c = self.conn.lock().unwrap();
        let mut st = c.prepare("SELECT id, name, path, modified_at, profile_id FROM projects WHERE profile_id = ?1 ORDER BY modified_at DESC LIMIT ?2").map_err(db_err)?;
        let rows = st
            .query_map(params![profile_id, limit as i64], |r| {
                Ok(RecentProject {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    path: PathBuf::from(r.get::<_, String>(2)?),
                    modified_at: r.get::<_, i64>(3)? as u64,
                    profile_id: r.get(4)?,
                })
            })
            .map_err(db_err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(db_err)
    }

    pub fn remove_project(&self, profile_id: &str, id: &str) -> StudioResult<()> {
        self.conn
            .lock()
            .unwrap()
            .execute(
                "DELETE FROM projects WHERE id = ?1 AND profile_id = ?2",
                params![id, profile_id],
            )
            .map_err(db_err)?;
        Ok(())
    }

    // ---------------- media (shared store + per-profile references) ----------------

    /// Record a media file in the shared store and add a reference for `profile_id`.
    /// Returns the previously known path for this fingerprint (duplicate detection).
    pub fn note_media(
        &self,
        profile_id: &str,
        fingerprint: &str,
        path: &Path,
        kind: &str,
        size: u64,
        duration_s: f64,
        now: u64,
    ) -> StudioResult<Option<PathBuf>> {
        let mut c = self.conn.lock().unwrap();
        let tx = c.transaction().map_err(db_err)?;
        let prev: Option<String> = tx
            .query_row(
                "SELECT path FROM media WHERE fingerprint = ?1",
                params![fingerprint],
                |r| r.get(0),
            )
            .optional()
            .map_err(db_err)?;
        tx.execute(
            "INSERT INTO media (fingerprint, path, kind, size, duration_s, last_seen) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(fingerprint) DO UPDATE SET path = excluded.path, last_seen = excluded.last_seen",
            params![fingerprint, path.to_string_lossy(), kind, size as i64, duration_s, now as i64],
        )
        .map_err(db_err)?;
        tx.execute("INSERT OR IGNORE INTO profile_assets (profile_id, fingerprint, shared, added_at) VALUES (?1, ?2, 0, ?3)", params![profile_id, fingerprint, now as i64])
            .map_err(db_err)?;
        tx.commit().map_err(db_err)?;
        Ok(prev.map(PathBuf::from).filter(|p| p != path))
    }

    pub fn set_asset_shared(
        &self,
        profile_id: &str,
        fingerprint: &str,
        shared: bool,
    ) -> StudioResult<()> {
        let n = self
            .conn
            .lock()
            .unwrap()
            .execute(
                "UPDATE profile_assets SET shared = ?3 WHERE profile_id = ?1 AND fingerprint = ?2",
                params![profile_id, fingerprint, shared as i32],
            )
            .map_err(db_err)?;
        if n == 0 {
            return Err(StudioError::not_found(
                "This profile has no reference to that media.",
            ));
        }
        Ok(())
    }

    /// Remove this profile's reference. Returns how many other profiles still reference
    /// the media; the shared index row (and of course the file) is kept while referenced.
    pub fn remove_asset_reference(&self, profile_id: &str, fingerprint: &str) -> StudioResult<u64> {
        let mut c = self.conn.lock().unwrap();
        let tx = c.transaction().map_err(db_err)?;
        tx.execute(
            "DELETE FROM profile_assets WHERE profile_id = ?1 AND fingerprint = ?2",
            params![profile_id, fingerprint],
        )
        .map_err(db_err)?;
        let remaining: i64 = tx
            .query_row(
                "SELECT COUNT(*) FROM profile_assets WHERE fingerprint = ?1",
                params![fingerprint],
                |r| r.get(0),
            )
            .map_err(db_err)?;
        if remaining == 0 {
            // Index row only. Files on disk are user data and are never deleted.
            tx.execute(
                "DELETE FROM media WHERE fingerprint = ?1",
                params![fingerprint],
            )
            .map_err(db_err)?;
        }
        tx.commit().map_err(db_err)?;
        Ok(remaining as u64)
    }

    /// Media visible to a profile: its own references plus assets other profiles shared.
    pub fn profile_assets(&self, profile_id: &str) -> StudioResult<Vec<ProfileAsset>> {
        let c = self.conn.lock().unwrap();
        let mut st = c
            .prepare(
                "SELECT m.fingerprint, m.path, m.kind, COALESCE(m.duration_s, 0), pa.shared, pa.profile_id
                 FROM profile_assets pa JOIN media m ON m.fingerprint = pa.fingerprint
                 WHERE pa.profile_id = ?1
                    OR (pa.shared = 1 AND NOT EXISTS (SELECT 1 FROM profile_assets o WHERE o.profile_id = ?1 AND o.fingerprint = pa.fingerprint))
                 ORDER BY m.last_seen DESC",
            )
            .map_err(db_err)?;
        let rows = st
            .query_map(params![profile_id], |r| {
                Ok(ProfileAsset {
                    fingerprint: r.get(0)?,
                    path: PathBuf::from(r.get::<_, String>(1)?),
                    kind: r.get(2)?,
                    duration_s: r.get(3)?,
                    shared: r.get::<_, i32>(4)? != 0,
                    owner_profile: r.get(5)?,
                })
            })
            .map_err(db_err)?;
        let mut v = rows.collect::<Result<Vec<_>, _>>().map_err(db_err)?;
        v.dedup_by(|a, b| a.fingerprint == b.fingerprint);
        Ok(v)
    }

    // ---------------- analysis cache (global, content-addressed) ----------------

    pub fn cache_get(&self, key: &str, version: u32) -> StudioResult<Option<PathBuf>> {
        let c = self.conn.lock().unwrap();
        let r: Option<(String, u32)> = c
            .query_row(
                "SELECT path, version FROM analysis_cache WHERE key = ?1",
                params![key],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(db_err)?;
        Ok(r.filter(|(p, v)| *v == version && Path::new(p).is_file())
            .map(|(p, _)| PathBuf::from(p)))
    }

    pub fn cache_put(
        &self,
        key: &str,
        kind: &str,
        version: u32,
        path: &Path,
        now: u64,
    ) -> StudioResult<()> {
        self.conn
            .lock()
            .unwrap()
            .execute(
                "INSERT INTO analysis_cache (key, kind, version, path, created_at) VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(key) DO UPDATE SET version = excluded.version, path = excluded.path, created_at = excluded.created_at",
                params![key, kind, version, path.to_string_lossy(), now as i64],
            )
            .map_err(db_err)?;
        Ok(())
    }

    /// Delete cache rows (and cache files) whose payload is gone or whose version is stale.
    pub fn cache_cleanup(&self, current_versions: &[(&str, u32)]) -> StudioResult<usize> {
        let c = self.conn.lock().unwrap();
        let mut st = c
            .prepare("SELECT key, kind, version, path FROM analysis_cache")
            .map_err(db_err)?;
        let rows: Vec<(String, String, u32, String)> = st
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
            .map_err(db_err)?
            .filter_map(|r| r.ok())
            .collect();
        let mut n = 0;
        for (key, kind, ver, path) in rows {
            let stale = current_versions
                .iter()
                .any(|(k, v)| *k == kind && *v != ver)
                || !Path::new(&path).is_file();
            if stale {
                let _ = std::fs::remove_file(&path);
                c.execute("DELETE FROM analysis_cache WHERE key = ?1", params![key])
                    .map_err(db_err)?;
                n += 1;
            }
        }
        Ok(n)
    }

    // ---------------- renders ----------------

    pub fn add_render(&self, profile_id: &str, e: &RenderHistoryEntry) -> StudioResult<()> {
        self.conn
            .lock()
            .unwrap()
            .execute(
                "INSERT OR REPLACE INTO renders (id, project_id, output, created_at, ok, duration_s, encoder, profile_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![e.id, e.project_id, e.output.to_string_lossy(), e.created_at as i64, e.ok as i32, e.duration_s, e.encoder, profile_id],
            )
            .map_err(db_err)?;
        Ok(())
    }

    pub fn recent_renders(
        &self,
        profile_id: &str,
        limit: usize,
    ) -> StudioResult<Vec<RenderHistoryEntry>> {
        let c = self.conn.lock().unwrap();
        let mut st = c
            .prepare("SELECT id, project_id, output, created_at, ok, duration_s, encoder FROM renders WHERE profile_id = ?1 ORDER BY created_at DESC LIMIT ?2")
            .map_err(db_err)?;
        let rows = st
            .query_map(params![profile_id, limit as i64], |r| {
                Ok(RenderHistoryEntry {
                    id: r.get(0)?,
                    project_id: r.get(1)?,
                    output: PathBuf::from(r.get::<_, String>(2)?),
                    created_at: r.get::<_, i64>(3)? as u64,
                    ok: r.get::<_, i32>(4)? != 0,
                    duration_s: r.get::<_, Option<f64>>(5)?.unwrap_or(0.0),
                    encoder: r.get::<_, Option<String>>(6)?.unwrap_or_default(),
                })
            })
            .map_err(db_err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(db_err)
    }

    // ---------------- trend research (per profile) ----------------

    pub fn add_trend_research(
        &self,
        profile_id: &str,
        query: &serde_json::Value,
        result: &serde_json::Value,
    ) -> StudioResult<String> {
        let id = studio_core::ids::new_id("research");
        self.conn
            .lock()
            .unwrap()
            .execute(
                "INSERT INTO trend_research (id, profile_id, created_at, query, result) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![id, profile_id, now() as i64, query.to_string(), result.to_string()],
            )
            .map_err(db_err)?;
        Ok(id)
    }

    pub fn trend_research(
        &self,
        profile_id: &str,
        limit: usize,
    ) -> StudioResult<Vec<(String, u64, serde_json::Value, serde_json::Value)>> {
        let c = self.conn.lock().unwrap();
        let mut st = c.prepare("SELECT id, created_at, query, result FROM trend_research WHERE profile_id = ?1 ORDER BY created_at DESC LIMIT ?2").map_err(db_err)?;
        let rows = st
            .query_map(params![profile_id, limit as i64], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)? as u64,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                ))
            })
            .map_err(db_err)?;
        let mut out = vec![];
        for r in rows {
            let (id, t, q, res) = r.map_err(db_err)?;
            out.push((
                id,
                t,
                serde_json::from_str(&q).unwrap_or_default(),
                serde_json::from_str(&res).unwrap_or_default(),
            ));
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(lib: &Library, name: &str) -> Profile {
        lib.create_profile(Profile {
            name: name.into(),
            ..Default::default()
        })
        .unwrap()
    }

    #[test]
    fn projects_and_renders_are_isolated_per_profile() {
        let l = Library::open_in_memory().unwrap();
        let a = profile(&l, "AnimeMotion");
        let b = profile(&l, "PhonkVisuals");
        l.upsert_project(&a.id, "p1", "One", Path::new("C:/a.aesproj"), 10)
            .unwrap();
        l.upsert_project(&b.id, "p2", "Two", Path::new("C:/b.aesproj"), 20)
            .unwrap();
        assert_eq!(
            l.recent_projects(&a.id, 10)
                .unwrap()
                .iter()
                .map(|p| p.id.as_str())
                .collect::<Vec<_>>(),
            vec!["p1"]
        );
        assert_eq!(
            l.recent_projects(&b.id, 10)
                .unwrap()
                .iter()
                .map(|p| p.id.as_str())
                .collect::<Vec<_>>(),
            vec!["p2"]
        );
        // Ownership cannot be taken over by another profile.
        assert!(l
            .upsert_project(&b.id, "p1", "Hijack", Path::new("C:/x"), 30)
            .is_err());
        assert_eq!(
            l.project_owner("p1").unwrap().as_deref(),
            Some(a.id.as_str())
        );
        // Removing through the wrong profile is a no-op.
        l.remove_project(&b.id, "p1").unwrap();
        assert_eq!(l.recent_projects(&a.id, 10).unwrap().len(), 1);

        l.add_render(
            &a.id,
            &RenderHistoryEntry {
                id: "r".into(),
                project_id: Some("p1".into()),
                output: "C:/o.mp4".into(),
                created_at: 5,
                ok: true,
                duration_s: 20.0,
                encoder: "libx264".into(),
            },
        )
        .unwrap();
        assert_eq!(l.recent_renders(&a.id, 5).unwrap().len(), 1);
        assert_eq!(l.recent_renders(&b.id, 5).unwrap().len(), 0);
        l.add_trend_research(
            &a.id,
            &serde_json::json!({"niche":"anime"}),
            &serde_json::json!({}),
        )
        .unwrap();
        assert_eq!(l.trend_research(&a.id, 10).unwrap().len(), 1);
        assert_eq!(l.trend_research(&b.id, 10).unwrap().len(), 0);
    }

    #[test]
    fn shared_assets_survive_reference_removal() {
        let l = Library::open_in_memory().unwrap();
        let a = profile(&l, "A");
        let b = profile(&l, "B");
        assert_eq!(
            l.note_media(&a.id, "fp1", Path::new("C:/x.mp4"), "video", 1, 1.0, 1)
                .unwrap(),
            None
        );
        l.note_media(&b.id, "fp1", Path::new("C:/x.mp4"), "video", 1, 1.0, 2)
            .unwrap();
        l.note_media(
            &a.id,
            "fp2",
            Path::new("C:/private.mp4"),
            "video",
            1,
            1.0,
            3,
        )
        .unwrap();
        // fp2 is private to A.
        assert!(l
            .profile_assets(&b.id)
            .unwrap()
            .iter()
            .all(|x| x.fingerprint != "fp2"));
        l.set_asset_shared(&a.id, "fp2", true).unwrap();
        assert!(l
            .profile_assets(&b.id)
            .unwrap()
            .iter()
            .any(|x| x.fingerprint == "fp2" && x.shared));
        // Removing A's reference to fp1 keeps it for B.
        assert_eq!(l.remove_asset_reference(&a.id, "fp1").unwrap(), 1);
        assert!(l
            .profile_assets(&b.id)
            .unwrap()
            .iter()
            .any(|x| x.fingerprint == "fp1"));
        assert!(l
            .profile_assets(&a.id)
            .unwrap()
            .iter()
            .all(|x| x.fingerprint != "fp1"));
        // Last reference gone → index row removed.
        assert_eq!(l.remove_asset_reference(&b.id, "fp1").unwrap(), 0);
        assert!(l
            .profile_assets(&b.id)
            .unwrap()
            .iter()
            .all(|x| x.fingerprint != "fp1"));
    }

    #[test]
    fn profile_lifecycle() {
        let l = Library::open_in_memory().unwrap();
        assert_eq!(l.active_profile().unwrap().id, DEFAULT_PROFILE_ID);
        let a = profile(&l, "CinemaCuts");
        l.set_favorite_preset(&a.id, "cinematic-dark", true)
            .unwrap();
        l.note_media(&a.id, "fp", Path::new("C:/m.mp4"), "video", 1, 1.0, 1)
            .unwrap();
        l.upsert_project(&a.id, "p", "P", Path::new("C:/p"), 1)
            .unwrap();
        let d = l.duplicate_profile(&a.id, "CinemaCuts 2").unwrap();
        assert_ne!(d.id, a.id);
        assert_eq!(l.favorite_presets(&d.id).unwrap(), vec!["cinematic-dark"]);
        assert_eq!(
            l.profile_stats(&d.id).unwrap(),
            ProfileStats {
                projects: 0,
                renders: 0,
                assets: 1
            }
        );
        assert_eq!(l.list_profiles(false, "cinema").unwrap().len(), 2);
        assert_eq!(
            l.list_profiles(false, "%").unwrap().len(),
            0,
            "LIKE wildcards are escaped"
        );

        l.set_active_profile(&a.id).unwrap();
        assert!(
            l.set_archived(&a.id, true).is_err(),
            "cannot archive the active profile"
        );
        l.set_active_profile(DEFAULT_PROFILE_ID).unwrap();
        assert!(l.delete_profile(&a.id).is_err(), "must archive first");
        l.set_archived(&a.id, true).unwrap();
        assert!(l.set_active_profile(&a.id).is_err());
        assert_eq!(
            l.list_profiles(false, "")
                .unwrap()
                .iter()
                .filter(|p| p.id == a.id)
                .count(),
            0
        );
        assert_eq!(
            l.list_profiles(true, "")
                .unwrap()
                .iter()
                .filter(|p| p.id == a.id)
                .count(),
            1
        );
        let rep = l.delete_profile(&a.id).unwrap();
        assert_eq!(rep.project_entries_removed, 1);
        assert_eq!(rep.asset_references_removed, 1);
        assert_eq!(
            rep.orphaned_media_entries, 0,
            "duplicate still references the media"
        );
        assert!(l.get_profile(&a.id).unwrap().is_none());
        assert!(l.recent_projects(&a.id, 10).unwrap().is_empty());
        assert!(l.delete_profile(DEFAULT_PROFILE_ID).is_err());
        assert!(l
            .create_profile(Profile {
                name: "  ".into(),
                ..Default::default()
            })
            .is_err());
    }

    #[test]
    fn migrates_v1_database_into_default_profile() {
        let path = std::env::temp_dir().join(format!("studio-migrate-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        {
            let c = Connection::open(&path).unwrap();
            c.execute_batch(MIGRATIONS[0]).unwrap();
            c.pragma_update(None, "user_version", 1).unwrap();
            c.execute(
                "INSERT INTO projects VALUES ('old', 'Old project', 'C:/old.aesproj', 5)",
                [],
            )
            .unwrap();
            c.execute(
                "INSERT INTO media VALUES ('fpold', 'C:/old.mp4', 'video', 10, 3.0, 5)",
                [],
            )
            .unwrap();
            c.execute(
                "INSERT INTO renders VALUES ('r1', 'old', 'C:/o.mp4', 6, 1, 3.0, 'libx264')",
                [],
            )
            .unwrap();
        }
        let l = Library::open(&path).unwrap();
        let p = l.recent_projects(DEFAULT_PROFILE_ID, 10).unwrap();
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].name, "Old project");
        assert_eq!(l.recent_renders(DEFAULT_PROFILE_ID, 10).unwrap().len(), 1);
        assert_eq!(
            l.profile_assets(DEFAULT_PROFILE_ID).unwrap()[0].fingerprint,
            "fpold"
        );
        assert!(l.active_profile().unwrap().created_at > 0);
        drop(l);
        // Re-opening is idempotent.
        let l = Library::open(&path).unwrap();
        assert_eq!(l.list_profiles(true, "").unwrap().len(), 1);
        drop(l);
        let _ = std::fs::remove_file(&path);
    }
}
