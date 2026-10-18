use super::*;
/// Uses a controlled monotonic clock to verify strict timeout and busy protection.
#[test]
fn idle_deadline_and_reservation() -> Result<(), String> {
    let now = Instant::now();
    let mut resident = Residency {
        value: Some(42),
        last_used: now,
    };
    assert!(!resident.unload(now + IDLE_TIMEOUT, false));
    assert!(!resident.unload(now + IDLE_TIMEOUT + Duration::from_secs(1), true));
    assert!(resident.unload(now + IDLE_TIMEOUT + Duration::from_secs(1), false));
    assert!(resident.value.is_none());
    let state = EngineState::default();
    state.reserve()?;
    assert!(state.reserve().is_err());
    state.release();
    state.reserve()?;
    state.release();
    Ok(())
}
/// A fake loader proves reuse, failure recovery, and exactly one reload after expiry.
#[test]
fn offline_loader_reuses_and_reloads_once() -> Result<(), String> {
    let now = Instant::now();
    let mut resident = Residency {
        value: None,
        last_used: now,
    };
    let mut loads = 0;
    assert!(resident
        .get_or_load(|| Err("Missing model".into()))
        .is_err());
    assert!(resident.value.is_none());
    for _ in 0..3 {
        let value = resident.get_or_load(|| {
            loads += 1;
            Ok(42)
        })?;
        assert_eq!(*value, 42);
    }
    assert_eq!(loads, 1);
    assert!(resident.unload(now + Duration::from_secs(301), false));
    resident.get_or_load(|| {
        loads += 1;
        Ok(43)
    })?;
    assert_eq!(loads, 2);
    Ok(())
}
/// Simultaneous requests admit one owner for download or transcription.
#[test]
fn competing_model_operations_have_one_owner() -> anyhow::Result<()> {
    let state = std::sync::Arc::new(EngineState::default());
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
    let workers: Vec<_> = (0..8)
        .map(|_| {
            let state = state.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                state.reserve().is_ok()
            })
        })
        .collect();
    let mut admitted = 0;
    for worker in workers {
        if worker
            .join()
            .map_err(|_| anyhow::anyhow!("Worker failed"))?
        {
            admitted += 1;
        }
    }
    assert_eq!(admitted, 1);
    state.release();
    Ok(())
}

/// Tracks an old fake heavyweight model to prove replacement never overlaps allocations.
struct ResidentAsset(std::rc::Rc<std::cell::Cell<bool>>);
impl Drop for ResidentAsset {
    /// Marks the fake model as released before another loader may allocate weights.
    fn drop(&mut self) {
        self.0.set(false);
    }
}

/// A failed switch releases the old model and a later offline retry loads exactly once.
#[test]
fn replacement_drops_before_load_and_retains_failure() -> Result<(), String> {
    let live = std::rc::Rc::new(std::cell::Cell::new(true));
    let mut resident = Residency {
        value: Some(ResidentAsset(live.clone())),
        last_used: Instant::now(),
    };
    assert!(resident
        .replace(|| {
            assert!(!live.get());
            Err("GPU unavailable".into())
        })
        .is_err());
    assert!(resident.value.is_none());
    let mut loads = 0;
    resident.get_or_load(|| {
        loads += 1;
        live.set(true);
        Ok(ResidentAsset(live.clone()))
    })?;
    resident.get_or_load(|| Err("Must reuse resident weights".into()))?;
    assert_eq!(loads, 1);
    drop(resident);
    assert!(!live.get());
    Ok(())
}

/// Failed reloads preserve explicit CPU selection, clear active diagnostics, and publish newer revisions.
#[test]
fn selection_survives_failure_and_destination_reload() -> anyhow::Result<()> {
    let app = tauri::test::mock_app();
    let directory = tempfile::tempdir()?;
    app.manage(crate::model_storage::ModelStorage::new(directory.path()));
    app.manage(EngineState::default());
    let state = app.state::<EngineState>();
    state.reserve().map_err(anyhow::Error::msg)?;
    assert!(state.select(app.handle(), AsrSelection::Cpu {}).is_err());
    assert!(state.reserve().is_err());
    state.release();
    let failed = state.snapshot().map_err(anyhow::Error::msg)?;
    assert_eq!(failed.selection, AsrSelection::Cpu {});
    assert_eq!(failed.phase, "missing");
    assert!(failed.provider.is_none() && failed.device.is_none());
    state.reserve().map_err(anyhow::Error::msg)?;
    state
        .reload_destination(app.handle())
        .map_err(anyhow::Error::msg)?;
    state.release();
    let reloaded = state.snapshot().map_err(anyhow::Error::msg)?;
    assert_eq!(reloaded.selection, AsrSelection::Cpu {});
    assert!(reloaded.revision > failed.revision);
    assert!(reloaded.device.is_none());
    // Invalid IPC IDs cannot discard or replace an already accepted preference.
    state.reserve().map_err(anyhow::Error::msg)?;
    assert!(state
        .select(
            app.handle(),
            AsrSelection::Device {
                id: "forged".into()
            }
        )
        .is_err());
    state.release();
    assert_eq!(
        state.snapshot().map_err(anyhow::Error::msg)?.selection,
        AsrSelection::Cpu {}
    );
    assert_eq!(
        EngineState::default()
            .snapshot()
            .map_err(anyhow::Error::msg)?
            .selection,
        AsrSelection::Auto {}
    );
    Ok(())
}

/// Publication preserves policy/catalog while clearing the active device during non-ready phases.
#[test]
fn unload_snapshot_preserves_selection_and_orders_updates() -> Result<(), String> {
    let app = tauri::test::mock_app();
    let state = EngineState::default();
    let device = DeviceSnapshot {
        id: "gpu".into(),
        backend: Provider::Metal,
        name: "MTL0".into(),
        description: "Synthetic".into(),
    };
    state.update(app.handle(), |s| {
        s.selection = AsrSelection::Device {
            id: device.id.clone(),
        };
        s.devices = vec![device.clone()];
    });
    state.publish(
        app.handle(),
        "ready",
        Some(device.clone()),
        None,
        vec![],
        None,
        None,
    );
    let ready = state.snapshot()?;
    state.publish(app.handle(), "unloaded", None, None, vec![], None, None);
    let unloaded = state.snapshot()?;
    assert!(unloaded.revision > ready.revision);
    assert_eq!(unloaded.selection, ready.selection);
    assert_eq!(unloaded.devices, ready.devices);
    assert_eq!(unloaded.models, ready.models);
    assert_eq!(unloaded.models, super::super::manifest::descriptors());
    assert!(unloaded.device.is_none() && unloaded.provider.is_none());
    Ok(())
}

/// The full IPC snapshot includes only display metadata and retains the manifest before loading.
#[test]
fn snapshot_serializes_public_model_identities() -> anyhow::Result<()> {
    let snapshot = EngineState::default()
        .snapshot()
        .map_err(anyhow::Error::msg)?;
    let value = serde_json::to_value(snapshot)?;
    assert_eq!(
        value["models"],
        serde_json::to_value(super::super::manifest::descriptors())?
    );
    for descriptor in value["models"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("Expected models array"))?
    {
        let fields = descriptor
            .as_object()
            .ok_or_else(|| anyhow::anyhow!("Expected descriptor"))?;
        assert_eq!(fields.len(), 7);
        for private in [
            "repository",
            "revision",
            "remoteFile",
            "directory",
            "filename",
            "sha256",
            "path",
        ] {
            assert!(!fields.contains_key(private));
        }
    }
    Ok(())
}

/// Recovery metadata follows resident weights and only appears in ready snapshots.
#[test]
fn recovery_snapshot_is_atomic_cleared_and_restored() -> anyhow::Result<()> {
    let app = tauri::test::mock_app();
    let state = EngineState::default();
    let device = DeviceSnapshot {
        id: "cpu".into(),
        backend: Provider::Cpu,
        name: "CPU".into(),
        description: "Synthetic".into(),
    };
    state.update(app.handle(), |s| {
        s.selection = AsrSelection::Device { id: "gpu".into() };
        s.devices = vec![device.clone()];
    });
    let mut resident = Residency {
        value: Some((device, Some(FallbackReason::GpuOutOfMemory))),
        last_used: Instant::now(),
    };
    for phase in [
        "loading",
        "unloaded",
        "downloading",
        "missing",
        "unavailable",
    ] {
        let (device, reason) = resident
            .get_or_load(|| Err("Must reuse resident diagnostics".into()))
            .map_err(anyhow::Error::msg)?;
        state.publish_ready(app.handle(), device.clone(), *reason);
        let ready = state.snapshot().map_err(anyhow::Error::msg)?;
        assert_eq!(ready.phase, "ready");
        assert_eq!(ready.provider, Some(Provider::Cpu));
        assert_eq!(ready.fallback_reason, Some(FallbackReason::GpuOutOfMemory));
        assert!(ready.error.is_none());
        assert_eq!(
            serde_json::to_value(&ready)?["fallbackReason"],
            "gpu_out_of_memory"
        );
        state.publish(app.handle(), phase, None, None, vec![], None, None);
        let cleared = state.snapshot().map_err(anyhow::Error::msg)?;
        assert_eq!(cleared.revision, ready.revision + 1);
        assert_eq!(cleared.selection, ready.selection);
        assert!(cleared.fallback_reason.is_none());
        assert!(cleared.device.is_none() && cleared.provider.is_none());
    }
    for reason in [
        FallbackReason::NoGpu,
        FallbackReason::GpuInitializationFailed,
    ] {
        let (device, _) = resident
            .get_or_load(|| Err("Must reuse weights".into()))
            .map_err(anyhow::Error::msg)?;
        state.publish_ready(app.handle(), device.clone(), Some(reason));
        assert_eq!(
            state
                .snapshot()
                .map_err(anyhow::Error::msg)?
                .fallback_reason,
            Some(reason)
        );
        state.publish_ready(app.handle(), device.clone(), None);
        assert!(state
            .snapshot()
            .map_err(anyhow::Error::msg)?
            .fallback_reason
            .is_none());
    }
    Ok(())
}
