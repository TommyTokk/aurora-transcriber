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

/// Confirms GPU priority followed by CPU, including CPU-only platforms.
#[test]
fn auto_prefers_os_gpus_then_cpu() -> anyhow::Result<()> {
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
            vec![2, 4, 1, 0]
        );
    }
    assert_eq!(
        candidate_indices(
            &devices,
            &AsrSelection::Auto {},
            gpu_order("macos", "aarch64")
        )?,
        vec![3, 0]
    );
    for (os, arch) in [("macos", "x86_64"), ("unsupported", "aarch64")] {
        assert_eq!(
            candidate_indices(&devices, &AsrSelection::Auto {}, gpu_order(os, arch))?,
            vec![0]
        );
    }
    assert_eq!(
        candidate_indices(
            &devices[..1],
            &AsrSelection::Auto {},
            gpu_order("linux", "x86_64")
        )?,
        vec![0]
    );
    assert!(matches!(
        candidate_indices(&[], &AsrSelection::Auto {}, &[]),
        Err(EngineError::CpuFallbackUnavailable)
    ));
    Ok(())
}

/// Rejects forged IDs before recovery and keeps explicit CPU independent of GPUs.
#[test]
fn explicit_selection_is_exact_before_cpu_recovery() -> anyhow::Result<()> {
    let devices = vec![device("cpu", Provider::Cpu), device("gpu", Provider::Metal)];
    assert_eq!(
        candidate_indices(&devices, &AsrSelection::Cpu {}, &[])?,
        vec![0]
    );
    assert_eq!(
        candidate_indices(&devices, &AsrSelection::Device { id: "gpu".into() }, &[])?,
        vec![1, 0]
    );
    assert_eq!(
        candidate_indices(
            &devices[1..],
            &AsrSelection::Device { id: "gpu".into() },
            &[]
        )?,
        vec![0]
    );
    for id in ["cpu", "unknown", ""] {
        assert!(matches!(
            candidate_indices(&devices, &AsrSelection::Device { id: id.into() }, &[]),
            Err(EngineError::SelectionUnavailable)
        ));
    }
    assert!(candidate_indices(&[], &AsrSelection::Cpu {}, &[]).is_err());
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

fn requests(backends: &[Backend]) -> Vec<ModelOptions> {
    backends
        .iter()
        .map(|backend| ModelOptions {
            backend: *backend,
            device: None,
        })
        .collect()
}

/// Proves failed allocations are dropped before the next GPU or CPU attempt.
#[test]
fn attempts_release_failures_before_cpu() -> anyhow::Result<()> {
    let candidates = requests(&[Backend::Cuda, Backend::Vulkan, Backend::Cpu]);
    let live = std::cell::Cell::new(0);
    let mut tried = vec![];
    let (asset, reason) = try_candidates(&candidates, &AsrSelection::Auto {}, |options| {
        assert_eq!(live.get(), 0);
        live.set(1);
        let allocation = Allocation(&live);
        tried.push(options.backend);
        if options.backend == Backend::Cpu {
            Ok(allocation)
        } else {
            Err(EngineError::GpuUnavailable)
        }
    })?;
    assert_eq!(tried, vec![Backend::Cuda, Backend::Vulkan, Backend::Cpu]);
    assert_eq!(reason, Some(FallbackReason::GpuInitializationFailed));
    assert_eq!(live.get(), 1);
    drop(asset);
    assert_eq!(live.get(), 0);
    Ok(())
}

/// CPU success records why it was reached; a successful GPU stops further attempts.
#[test]
fn recovery_reason_and_success_short_circuit() -> anyhow::Result<()> {
    for error in [
        EngineError::GpuUnavailable,
        EngineError::ModelLoad,
        EngineError::OutOfMemory,
    ] {
        let expected = if matches!(error, EngineError::OutOfMemory) {
            FallbackReason::GpuOutOfMemory
        } else {
            FallbackReason::GpuInitializationFailed
        };
        let mut error = Some(error);
        let (_, reason) = try_candidates(
            &requests(&[Backend::Metal, Backend::Cpu]),
            &AsrSelection::Device { id: "gpu".into() },
            |options| {
                if options.backend == Backend::Cpu {
                    Ok(())
                } else {
                    Err(error.take().ok_or(EngineError::Unavailable)?)
                }
            },
        )?;
        assert_eq!(reason, Some(expected));
    }
    for (selection, expected) in [
        (AsrSelection::Auto {}, Some(FallbackReason::NoGpu)),
        (AsrSelection::Cpu {}, None),
    ] {
        let (_, reason) = try_candidates(&requests(&[Backend::Cpu]), &selection, |_| Ok(()))?;
        assert_eq!(reason, expected);
    }
    let mut tried = vec![];
    let (_, reason) = try_candidates(
        &requests(&[Backend::Cuda, Backend::Vulkan, Backend::Cpu]),
        &AsrSelection::Auto {},
        |options| {
            tried.push(options.backend);
            if options.backend == Backend::Cuda {
                Err(EngineError::GpuUnavailable)
            } else {
                Ok(())
            }
        },
    )?;
    assert_eq!(tried, vec![Backend::Cuda, Backend::Vulkan]);
    assert_eq!(reason, None);
    let mut calls = 0;
    try_candidates(
        &requests(&[Backend::Metal, Backend::Cpu]),
        &AsrSelection::Auto {},
        |_| {
            calls += 1;
            Ok(())
        },
    )?;
    assert_eq!(calls, 1);
    let (_, reason) = try_candidates(
        &requests(&[Backend::Cuda, Backend::Vulkan, Backend::Cpu]),
        &AsrSelection::Auto {},
        |options| match options.backend {
            Backend::Cuda => Err(EngineError::OutOfMemory),
            Backend::Vulkan => Err(EngineError::GpuUnavailable),
            _ => Ok(()),
        },
    )?;
    assert_eq!(reason, Some(FallbackReason::GpuOutOfMemory));
    Ok(())
}

/// Terminal errors never retry; missing or failing CPU is reported as recovery failure.
#[test]
fn terminal_errors_and_failed_recovery() {
    for error in [
        EngineError::Configuration,
        EngineError::Resources,
        EngineError::ModelContract,
        EngineError::NativeCompatibility,
        EngineError::SessionLimits,
        EngineError::Cancelled,
        EngineError::SelectionUnavailable,
        EngineError::Inference,
    ] {
        let expected = std::mem::discriminant(&error);
        let mut error = Some(error);
        let mut calls = 0;
        let result = try_candidates::<()>(
            &requests(&[Backend::Metal, Backend::Cpu]),
            &AsrSelection::Auto {},
            |_| {
                calls += 1;
                Err(error.take().unwrap_or(EngineError::Unavailable))
            },
        );
        assert_eq!(calls, 1);
        assert_eq!(
            result.err().map(|e| std::mem::discriminant(&e)),
            Some(expected)
        );
    }
    assert!(matches!(
        try_candidates::<()>(&requests(&[Backend::Metal]), &AsrSelection::Auto {}, |_| {
            Err(EngineError::GpuUnavailable)
        }),
        Err(EngineError::CpuFallbackUnavailable)
    ));
    let mut calls = 0;
    assert!(matches!(
        try_candidates::<()>(&[], &AsrSelection::Auto {}, |_| {
            calls += 1;
            Ok(())
        }),
        Err(EngineError::CpuFallbackUnavailable)
    ));
    assert_eq!(calls, 0);
    assert!(
        matches!(try_candidates::<()>(&requests(&[Backend::Metal, Backend::Cpu]), &AsrSelection::Auto {}, |_| Err(EngineError::OutOfMemory)), Err(EngineError::CpuFallbackFailed(error)) if matches!(*error, EngineError::OutOfMemory))
    );
    assert!(matches!(
        try_candidates::<()>(&requests(&[Backend::Cpu]), &AsrSelection::Cpu {}, |_| Err(
            EngineError::OutOfMemory
        )),
        Err(EngineError::OutOfMemory)
    ));
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
