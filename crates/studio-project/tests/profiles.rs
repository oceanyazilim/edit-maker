//! Profile workspaces at the engine level: isolation, defaults, ownership
//! enforcement, persistence across restarts. Uses real files and SQLite.

use studio_audio::synth;
use studio_core::CancelToken;
use studio_project::{AppDirs, AssetRole, Engine, Profile, ProfileDefaults, DEFAULT_PROFILE_ID};

fn root(name: &str) -> std::path::PathBuf {
    let r = std::env::temp_dir().join("studio-profile-tests").join(name);
    let _ = std::fs::remove_dir_all(&r);
    std::fs::create_dir_all(&r).unwrap();
    r
}

#[test]
fn profiles_isolate_projects_and_survive_restart() {
    let r = root("isolation");
    let dirs = AppDirs::under(&r);
    let anime_id;
    let phonk_project_path;
    {
        let e = Engine::new(dirs.clone()).unwrap();
        let anime = e
            .library
            .create_profile(Profile {
                name: "AnimeMotion".into(),
                primary_niche: Some("anime".into()),
                defaults: ProfileDefaults {
                    preset_id: "anime-impact".into(),
                    duration_secs: 15.0,
                    ..Default::default()
                },
                ..Default::default()
            })
            .unwrap();
        let phonk = e
            .library
            .create_profile(Profile {
                name: "PhonkVisuals".into(),
                primary_niche: Some("phonk".into()),
                defaults: ProfileDefaults {
                    preset_id: "aggressive-phonk".into(),
                    aspect: "1:1".into(),
                    ..Default::default()
                },
                ..Default::default()
            })
            .unwrap();
        anime_id = anime.id.clone();

        // New projects take the active profile's defaults.
        e.library.set_active_profile(&anime.id).unwrap();
        let s = e.default_project_settings().unwrap();
        assert_eq!(
            (s.preset_id.as_str(), s.duration_secs, s.niche_id.as_deref()),
            ("anime-impact", 15.0, Some("anime"))
        );
        let mut pa = e.create_project("Anime edit", s).unwrap();
        assert_eq!(pa.profile_id, anime.id);

        // Overriding a project setting does not modify the profile defaults.
        pa.settings.duration_secs = 42.0;
        e.save_project(&mut pa).unwrap();
        assert_eq!(
            e.library
                .get_profile(&anime.id)
                .unwrap()
                .unwrap()
                .defaults
                .duration_secs,
            15.0
        );

        e.library.set_active_profile(&phonk.id).unwrap();
        let s = e.default_project_settings().unwrap();
        assert_eq!((s.width, s.height), (1080, 1080));
        let pp = e.create_project("Phonk edit", s).unwrap();
        phonk_project_path = pp.file.clone().unwrap();

        // Each profile sees only its own projects.
        assert_eq!(
            e.library
                .recent_projects(&anime.id, 10)
                .unwrap()
                .iter()
                .map(|p| p.name.clone())
                .collect::<Vec<_>>(),
            vec!["Anime edit"]
        );
        assert_eq!(
            e.library
                .recent_projects(&phonk.id, 10)
                .unwrap()
                .iter()
                .map(|p| p.name.clone())
                .collect::<Vec<_>>(),
            vec!["Phonk edit"]
        );
        assert!(e
            .library
            .recent_projects(DEFAULT_PROFILE_ID, 10)
            .unwrap()
            .is_empty());

        // Opening another profile's project is refused by the backend.
        let err = e
            .open_project(pa.file.as_ref().unwrap())
            .err()
            .expect("must be rejected");
        assert!(err.message.contains("AnimeMotion"), "{}", err.message);
        // Saving it under the wrong active profile is refused too.
        let mut pa2 = pa.clone();
        assert!(e.save_project(&mut pa2).is_err());

        // Shared media: music imported by the phonk profile, shared, visible to anime.
        let tr = synth::render(&synth::edm_arrangement()[2..3], 140.0, 44100, 0.1, 4);
        let wav = r.join("track.wav");
        studio_audio::decode::write_wav_mono(&wav, &tr.samples, 44100).unwrap();
        let mut pp = e.open_project(&phonk_project_path).unwrap().project;
        let asset = e
            .import_media(&mut pp, &wav, AssetRole::Music, &CancelToken::new())
            .unwrap();
        assert!(e
            .library
            .profile_assets(&anime.id)
            .unwrap()
            .iter()
            .all(|a| a.fingerprint != asset.fingerprint));
        e.library
            .set_asset_shared(&phonk.id, &asset.fingerprint, true)
            .unwrap();
        assert!(e
            .library
            .profile_assets(&anime.id)
            .unwrap()
            .iter()
            .any(|a| a.fingerprint == asset.fingerprint));
        e.library.set_active_profile(&anime.id).unwrap();
    }
    // "Restart": a new engine on the same directories restores the last active profile.
    let e = Engine::new(dirs).unwrap();
    assert_eq!(e.active_profile().unwrap().id, anime_id);
    assert_eq!(e.library.list_profiles(false, "").unwrap().len(), 3);
    let recent = e.library.recent_projects(&anime_id, 10).unwrap();
    let reopened = e.open_project(&recent[0].path).unwrap();
    assert_eq!(reopened.project.settings.duration_secs, 42.0);
    assert!(e.open_project(&phonk_project_path).is_err());
}

#[test]
fn pre_profile_project_files_load_into_default_profile() {
    let r = root("legacy");
    let e = Engine::new(AppDirs::under(&r)).unwrap();
    let p = e
        .create_project("Legacy", e.default_project_settings().unwrap())
        .unwrap();
    let file = p.file.clone().unwrap();
    // Strip profile_id as a pre-profile build would have written it.
    let mut v: serde_json::Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
    v.as_object_mut().unwrap().remove("profile_id");
    std::fs::write(&file, serde_json::to_vec(&v).unwrap()).unwrap();
    let loaded = e.open_project(&file).unwrap();
    assert_eq!(loaded.project.profile_id, DEFAULT_PROFILE_ID);
}
