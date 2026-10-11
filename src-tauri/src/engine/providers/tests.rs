use super::*;

/// Constructs hardware-only catalog entries without touching the native registry.
fn device(id: &str, backend: Provider) -> DeviceSnapshot {
    DeviceSnapshot {
        id: id.into(),
        backend,
        name: id.into(),
        description: "Synthetic hardware".into(),
    }
}

/// Confirms OS priority, registry order within each backend, and exclusion of implicit CPU.
#[test]
fn auto_is_os_specific_and_gpu_only() -> anyhow::Result<()> {
    let devices = vec![
        device("cpu", Provider::Cpu),
        device("vulkan", Provider::Vulkan),
        device("cuda0", Provider::Cuda),
        device("metal", Provider::Metal),
        device("cuda1", Provider::Cuda),
    ];
    for os in ["windows", "linux"] {
        assert_eq!(
            candidate_indices(&devices, &AsrSelection::Auto {}, gpu_order(os, "x86_64"))?,
            vec![2, 4, 1]
        );
    }
    assert_eq!(
        candidate_indices(
            &devices,
            &AsrSelection::Auto {},
            gpu_order("macos", "aarch64")
        )?,
        vec![3]
    );
    for (os, arch) in [("macos", "x86_64"), ("unsupported", "aarch64")] {
        assert!(candidate_indices(&devices, &AsrSelection::Auto {}, gpu_order(os, arch)).is_err());
    }
    assert!(matches!(
        candidate_indices(
            &devices[..1],
            &AsrSelection::Auto {},
            gpu_order("linux", "x86_64")
        ),
        Err(EngineError::GpuUnavailable)
    ));
    Ok(())
}

/// Allows strict CPU opt-in while rejecting forged IDs and CPU disguised as an explicit GPU.
#[test]
fn explicit_selection_is_exact() -> anyhow::Result<()> {
    let devices = vec![device("cpu", Provider::Cpu), device("gpu", Provider::Metal)];
    assert_eq!(
        candidate_indices(&devices, &AsrSelection::Cpu {}, &[])?,
        vec![0]
    );
    assert_eq!(
        candidate_indices(&devices, &AsrSelection::Device { id: "gpu".into() }, &[])?,
        vec![1]
    );
    for id in ["cpu", "unknown", ""] {
        assert!(matches!(
            candidate_indices(&devices, &AsrSelection::Device { id: id.into() }, &[]),
            Err(EngineError::SelectionUnavailable)
        ));
    }
    assert!(candidate_indices(&[], &AsrSelection::Cpu {}, &[]).is_err());
    // Tagged IPC payloads cannot smuggle a handle or an ID into an Auto/CPU choice.
    for invalid in [
        r#"{"mode":"auto","id":"gpu"}"#,
        r#"{"mode":"cpu","handle":123}"#,
        r#"{"mode":"device"}"#,
        r#"{"mode":"unknown"}"#,
    ] {
        assert!(serde_json::from_str::<AsrSelection>(invalid).is_err());
    }
    Ok(())
}

/// Rejects misleading backend names, CPU fallback and unclassified accelerator devices.
#[test]
fn actual_backend_is_verified() -> anyhow::Result<()> {
    for (kind, provider, name) in [
        ("metal", Provider::Metal, "MTL0"),
        ("cuda", Provider::Cuda, "CUDA0"),
        ("vulkan", Provider::Vulkan, "Vulkan0"),
    ] {
        for class in [DeviceType::Gpu, DeviceType::Igpu] {
            assert_eq!(verify_backend(kind, name, kind, class)?, provider);
            assert_eq!(verify_backend(name, name, kind, class)?, provider);
        }
        for reported in ["cpu", "", "another GPU"] {
            assert!(verify_backend(reported, name, kind, DeviceType::Gpu).is_err());
        }
        assert!(verify_backend(kind, name, kind, DeviceType::Cpu).is_err());
        assert!(verify_backend(kind, name, kind, DeviceType::Unknown).is_err());
    }
    assert_eq!(
        verify_backend("cpu", "CPU", "cpu", DeviceType::Cpu)?,
        Provider::Cpu
    );
    assert!(classify("cpu", DeviceType::Accel).is_none());
    Ok(())
}

/// Tracks scoped fake native allocations to prove failure cleanup before the next GPU attempt.
struct Allocation<'a>(&'a std::cell::Cell<usize>);
impl Drop for Allocation<'_> {
    /// Mirrors native RAII ownership by releasing the tracked allocation on every exit.
    fn drop(&mut self) {
        self.0.set(self.0.get() - 1);
    }
}

/// Exercises GPU retries and final failure without constructing models or running CPU ASR.
#[test]
fn attempts_release_failures_and_never_add_cpu() -> anyhow::Result<()> {
    let requests = [Backend::Cuda, Backend::Cuda, Backend::Vulkan].map(|backend| ModelOptions {
        backend,
        device: None,
    });
    let live = std::cell::Cell::new(0);
    let mut tried = vec![];
    let result = try_candidates(&requests, |options| {
        assert_eq!(live.get(), 0);
        live.set(1);
        let allocation = Allocation(&live);
        tried.push(options.backend);
        if options.backend == Backend::Vulkan {
            Ok(allocation)
        } else {
            Err(EngineError::GpuUnavailable)
        }
    })?;
    assert_eq!(tried, vec![Backend::Cuda, Backend::Cuda, Backend::Vulkan]);
    assert_eq!(live.get(), 1);
    drop(result);
    assert_eq!(live.get(), 0);
    let mut calls = 0;
    assert!(try_candidates::<()>(&requests[..1], |_| {
        calls += 1;
        Err(EngineError::OutOfMemory)
    })
    .is_err());
    assert_eq!(calls, 1);
    let mut calls = 0;
    assert!(matches!(
        try_candidates::<()>(&requests, |_| {
            calls += 1;
            Err(EngineError::GpuUnavailable)
        }),
        Err(EngineError::GpuUnavailable)
    ));
    assert_eq!(calls, 3);
    assert!(
        try_candidates::<()>(&[], |_| panic!("Empty discovery must not load any model")).is_err()
    );
    Ok(())
}

/// Ensures same-vendor GPU substitution and GPU-to-CPU fallback cannot pass readiness.
#[test]
fn placement_matches_the_requested_identity() -> anyhow::Result<()> {
    verify_placement(Provider::Metal, Backend::Metal, true)?;
    verify_placement(Provider::Cpu, Backend::Cpu, true)?;
    assert!(verify_placement(Provider::Metal, Backend::Metal, false).is_err());
    assert!(verify_placement(Provider::Cpu, Backend::Metal, true).is_err());
    assert!(verify_placement(Provider::Vulkan, Backend::Cuda, true).is_err());
    assert!(verify_placement(Provider::Cpu, Backend::Auto, true).is_err());
    Ok(())
}
