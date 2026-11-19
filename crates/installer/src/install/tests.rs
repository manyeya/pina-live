use super::*;

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("pina-live-installer-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

/// A Pina Live.app with only what the installer reads: its version and its program.
fn fake_app(at: &Path, v: &str) {
    fs::create_dir_all(at.join("Contents/MacOS")).unwrap();
    fs::write(
        at.join("Contents/Info.plist"),
        format!(
            "<plist><dict><key>CFBundleShortVersionString</key><string>{v}</string></dict></plist>"
        ),
    )
    .unwrap();
    fs::write(at.join(PROGRAM), v).unwrap();
}

#[test]
fn versions_compare_number_by_number() {
    assert_eq!(over(None, "0.2.0"), Over::Nothing);
    assert_eq!(
        over(Some("0.9.7".into()), "0.10.2"),
        Over::Older("0.9.7".into())
    );
    assert_eq!(over(Some("0.2.0".into()), "0.2.0"), Over::Same);
    assert_eq!(
        over(Some("0.3.0".into()), "0.2.9"),
        Over::Newer("0.3.0".into())
    );
    assert_eq!(
        over(Some("0.2.0-dev".into()), "0.2.0"),
        Over::Older("0.2.0-dev".into())
    );
}

#[test]
fn installing_puts_it_there_whole_and_an_update_replaces_it() {
    let root = scratch("install");
    let payload = root.join("payload").join(APP);
    fake_app(&payload, "0.2.0");
    let plan = Plan {
        payload: payload.clone(),
        apps: root.join("Applications"),
    };
    assert_eq!(version(&plan.app()), None);
    let app = copy_app(&plan).unwrap();
    assert_eq!(version(&app).as_deref(), Some("0.2.0"));
    // A newer one over it: replaced, nothing left beside it.
    fake_app(&payload, "0.3.0");
    copy_app(&plan).unwrap();
    assert_eq!(version(&app).as_deref(), Some("0.3.0"));
    let left: Vec<_> = fs::read_dir(&plan.apps)
        .unwrap()
        .flatten()
        .map(|e| e.file_name())
        .collect();
    assert_eq!(left, vec![std::ffi::OsString::from(APP)]);
    // Out again: the app gone.
    uninstall(&plan, false).unwrap();
    assert!(!app.exists());
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_missing_payload_says_so_and_leaves_the_old_one() {
    let root = scratch("missing");
    let plan = Plan {
        payload: root.join("nowhere").join(APP),
        apps: root.join("Applications"),
    };
    fake_app(&plan.app(), "0.1.0");
    assert!(copy_app(&plan).is_err());
    assert_eq!(version(&plan.app()).as_deref(), Some("0.1.0"));
    let _ = fs::remove_dir_all(&root);
}
