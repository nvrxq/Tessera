use std::sync::Arc;

/// Errors that can occur while initialising wgpu resources.
#[derive(Debug)]
pub enum ResourcesError {
    /// No GPU adapter matched the request (wgpu 29+: `RequestAdapterError`).
    NoAdapter(wgpu::RequestAdapterError),
    /// The adapter refused to create a logical device.
    DeviceRequest(wgpu::RequestDeviceError),
}

impl std::fmt::Display for ResourcesError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResourcesError::NoAdapter(e) => write!(f, "no suitable GPU adapter: {e}"),
            ResourcesError::DeviceRequest(e) => write!(f, "device request failed: {e}"),
        }
    }
}

impl std::error::Error for ResourcesError {}

impl From<wgpu::RequestAdapterError> for ResourcesError {
    fn from(e: wgpu::RequestAdapterError) -> Self {
        ResourcesError::NoAdapter(e)
    }
}

impl From<wgpu::RequestDeviceError> for ResourcesError {
    fn from(e: wgpu::RequestDeviceError) -> Self {
        ResourcesError::DeviceRequest(e)
    }
}

#[derive(Debug)]
pub struct Resources {
    pub instance: wgpu::Instance,
    pub adapter: wgpu::Adapter,
    pub device: Arc<wgpu::Device>,
    pub queue: Arc<wgpu::Queue>,
}

impl Resources {
    /// Create resources without a surface — used by tests and offscreen render.
    /// Picks the highest-power adapter available.
    ///
    /// # Divergences from plan (wgpu 0.20 → 29)
    /// - `InstanceDescriptor` no longer implements `Default`; all fields must be
    ///   specified explicitly (`flags`, `backend_options`, `display`,
    ///   `memory_budget_thresholds`).
    /// - `Instance::new` takes the descriptor by value (was by reference in 0.20).
    /// - `request_adapter` now returns `Result<Adapter, RequestAdapterError>`
    ///   instead of `Option<Adapter>`, so the `ok_or_else` pattern is replaced by
    ///   `?` with a `From` impl.
    /// - `DeviceDescriptor` gained two new required fields: `experimental_features`
    ///   and `trace`; both are satisfied by `Default::default()`.
    /// - `RequestDeviceError` has private fields and cannot be constructed with
    ///   `{}` literal syntax; a dedicated `ResourcesError` enum is used instead.
    pub fn new_headless() -> Result<Self, ResourcesError> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            flags: wgpu::InstanceFlags::default(),
            backend_options: wgpu::BackendOptions::default(),
            display: Default::default(),
            memory_budget_thresholds: Default::default(),
        });

        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: None,
            force_fallback_adapter: false,
        }))?;

        let (device, queue) = pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: Some("tessera-render headless device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                memory_hints: wgpu::MemoryHints::default(),
                experimental_features: Default::default(),
                trace: Default::default(),
            },
        ))?;

        Ok(Self {
            instance,
            adapter,
            device: Arc::new(device),
            queue: Arc::new(queue),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headless_resources_initialise() {
        let r = Resources::new_headless();
        match r {
            Ok(res) => {
                // Smoke check: device must be able to create a tiny buffer.
                let _ = res.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("smoke"),
                    size: 16,
                    usage: wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
            }
            Err(e) => eprintln!("no GPU adapter available — skipping headless test: {e}"),
        }
    }
}
