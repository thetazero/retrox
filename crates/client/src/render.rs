//! GPU setup and drawing.
//!
//! Floating origin: the camera is always at the origin. The world hands us
//! instances already positioned relative to the camera (computed in f64,
//! then narrowed), so precision only suffers far away, where nobody can see
//! it. Depth is reversed-Z with an infinite far plane, which covers
//! everything from a cockpit switch to a star several AU away.

use std::sync::Arc;

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};
use wgpu::util::DeviceExt;
use winit::window::Window;

use crate::meshes::{Mesh, Vertex};

pub const HDR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
pub const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// A point on a rock's mesh (within ±[`crate::mining::ROCK_BOUND`]) as
/// one float, to 1/255 of the bound on each axis: exact in an f32 (under
/// 2^24), and never 0, which means "none" (mesh.wgsl's `rock_point`).
fn pack_rock_point(p: Vec3) -> f32 {
    let bound = crate::mining::ROCK_BOUND as f32;
    let q = (p / bound * 0.5 + Vec3::splat(0.5)).clamp(Vec3::ZERO, Vec3::ONE) * 255.0;
    let [x, y, z] = [q.x, q.y, q.z].map(|c| c.round() as u32);
    (1 + x + y * 256 + z * 65_536) as f32
}

/// What a rock's veins are made of, for their colour; must match
/// `mesh.wgsl`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum VeinKind {
    None = 0,
    Ore = 1,
    Ice = 2,
    /// Pulses red: it bursts when it breaks.
    Volatile = 3,
}

/// Surface shading modes; must match `mesh.wgsl`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Surface {
    Plain = 0,
    Star = 1,
    Gas = 3,
    Ocean = 4,
    Ice = 5,
    Desert = 7,
    Barren = 8,
    /// Bare rock: mottled and bump-mapped (asteroids). `emissive` is the
    /// laser's heat.
    Rock = 9,
    /// A world's sky on a dome round the camera (city.rs): `color` the
    /// zenith, `aux` the horizon, `emissive` the sun's angular radius,
    /// `detail` cloud cover.
    SkyDome = 10,
    /// The ground round a city's pad, and its hills (city.rs).
    Ground = 11,
    /// Glows, drawn additively after everything solid (see `fs_glow`): a
    /// shield bubble lit round a hit (`light.xyz`: the hit's direction in
    /// object space, `light.w`: 0..1 through its life).
    Shield = 20,
    /// A fireball, `light.w` 0..1 through its life.
    Fire = 21,
    /// A rim-lit shell: shockwave rings, haloes.
    Ring = 22,
    /// A rock breaking: a soft hot puff that cools to `color`, `light.w`
    /// 0..1 through its life.
    Flash = 23,
    /// A star's corona, on a shell `detail` times the star's radius.
    Corona = 24,
    /// The dust of an asteroid belt, on a sphere round the camera: sunlit,
    /// brightest toward the sun, thickest along the belt (`aux`: the
    /// camera from the field's centre, and its radius).
    Dust = 25,
    /// A mote of space dust streaming past the ship (#211): a soft speck,
    /// `emissive` its brightness.
    Mote = 26,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct Instance {
    pub model: [[f32; 4]; 4],
    pub color: [f32; 4],
    pub params: [f32; 4],
    pub light: [f32; 4],
    /// Surface-specific extras. Rock: up to four vein points, packed
    /// (`Instance::with_veins`). Dust: the camera's place in the field.
    pub aux: [f32; 4],
}

impl Instance {
    pub fn new(
        model: Mat4,
        color: [f32; 3],
        surface: Surface,
        emissive: f32,
        seed: f32,
        light: [f32; 4],
    ) -> Self {
        Self {
            model: model.to_cols_array_2d(),
            // Alpha: how scorched a built thing is (0 clean, 1 a burnt-out
            // wreck with embers in its cracks).
            color: [color[0], color[1], color[2], 0.0],
            params: [surface as u32 as f32, emissive, seed, 0.0],
            light,
            aux: [0.0; 4],
        }
    }

    /// Scorch a built thing: 0 clean, up to 1 burnt out and glowing.
    pub fn with_scorch(mut self, scorch: f32) -> Self {
        self.color[3] = scorch;
        self
    }

    fn glows(&self) -> bool {
        self.params[0] >= 20.0
    }

    /// The pilot's own hull: it opens round the combat target where it
    /// would hide it (see `Frame::reveal`). Seeds are never negative
    /// otherwise.
    pub fn own_hull(mut self) -> Self {
        self.params[2] = -1.0;
        self
    }

    /// Rock: the veins a prospector has read, as points in the mesh's own
    /// space, drawn as ore seams in the rock in the colour of `kind`.
    pub fn with_veins(mut self, points: &[Vec3], kind: VeinKind) -> Self {
        for (slot, p) in self.aux.iter_mut().zip(points) {
            *slot = pack_rock_point(*p);
        }
        self.params[3] = kind as u32 as f32;
        self
    }

    /// Rock: where the laser is cutting, in the mesh's own space. The
    /// laser's heat glows round it rather than over the whole rock.
    pub fn with_cut(mut self, at: Vec3) -> Self {
        self.color[3] = pack_rock_point(at);
        self
    }

    /// Dust: the camera's place relative to the field's centre (metres)
    /// and the field's radius.
    pub fn with_aux(mut self, aux: [f32; 4]) -> Self {
        self.aux = aux;
        self
    }

    /// Built things: panel cells per object unit (0 for none). Ocean
    /// worlds: sea level.
    pub fn with_detail(mut self, detail: f32) -> Self {
        self.params[3] = detail;
        self
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct Globals {
    view_proj: [[f32; 4]; 4],
    inv_view_proj: [[f32; 4]; 4],
    star_color: [f32; 4],
    misc: [f32; 4],
    /// Where the pilot's hull opens (mesh.wgsl): x, y in pixels, radius
    /// in pixels, strength 0..1.
    reveal: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct SkyVertex {
    pub dir: [f32; 3],
    pub color: [f32; 3],
}

/// A mesh uploaded into the shared buffers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct MeshId(usize);

struct MeshRange {
    first_index: u32,
    index_count: u32,
    base_vertex: i32,
}

/// Everything to draw this frame.
pub struct Frame<'a> {
    pub view_proj: Mat4,
    pub star_color: [f32; 3],
    pub time: f32,
    pub exposure: f32,
    pub instances: &'a [(MeshId, Instance)],
    /// What the pilot is shooting at, if their own hull could hide it:
    /// its position relative to the camera and its radius in metres.
    /// The hull (`Instance::own_hull`) opens round it.
    pub reveal: Option<(Vec3, f32)>,
    pub ui: Option<(&'a egui::Context, egui::FullOutput)>,
}

pub struct Renderer {
    pub window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,

    globals: wgpu::Buffer,
    globals_bind: wgpu::BindGroup,
    mesh_pipeline: wgpu::RenderPipeline,
    /// Additive glows over the solid scene: shields, fire, shockwaves.
    glow_pipeline: wgpu::RenderPipeline,
    sky_pipeline: wgpu::RenderPipeline,
    post_pipeline: wgpu::RenderPipeline,
    /// Bright-pass downsample, then blurs: horizontal and vertical, narrow
    /// then wide.
    bloom_pipelines: [wgpu::RenderPipeline; 5],
    post_layout: wgpu::BindGroupLayout,
    bloom_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    targets: Targets,

    meshes: Vec<MeshRange>,
    vertices: Vec<Vertex>,
    indices: Vec<u32>,
    vertex_buffer: Option<wgpu::Buffer>,
    index_buffer: Option<wgpu::Buffer>,
    dirty_meshes: bool,
    instance_buffer: wgpu::Buffer,
    instance_capacity: usize,
    sky_buffer: Option<(wgpu::Buffer, u32)>,

    pub egui: egui_wgpu::Renderer,
    /// Save the next frame here (as a PNG), then clear.
    pub capture: Option<std::path::PathBuf>,
}

impl Renderer {
    pub fn new(window: Arc<Window>) -> Self {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::PRIMARY,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let surface = instance
            .create_surface(window.clone())
            .expect("create surface");
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            ..Default::default()
        }))
        .expect("no suitable GPU adapter");
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("retrox"),
            ..Default::default()
        }))
        .expect("request device");

        let size = window.inner_size();
        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(caps.formats[0]);
        let config = wgpu::SurfaceConfiguration {
            // COPY_SRC (where supported) lets screenshots read the frame back.
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | (caps.usages & wgpu::TextureUsages::COPY_SRC),
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::AutoVsync,
            desired_maximum_frame_latency: 2,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            color_space: Default::default(),
        };
        surface.configure(&device, &config);

        let globals = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("globals"),
            size: std::mem::size_of::<Globals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("globals"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let globals_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("globals"),
            layout: &globals_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: globals.as_entire_binding(),
            }],
        });

        let post_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("post"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                texture_entry(2),
            ],
        });
        let bloom_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("bloom"),
            entries: &[
                texture_entry(0),
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let mesh_shader = device.create_shader_module(wgpu::include_wgsl!("shaders/mesh.wgsl"));
        let sky_shader = device.create_shader_module(wgpu::include_wgsl!("shaders/sky.wgsl"));
        let post_shader = device.create_shader_module(wgpu::include_wgsl!("shaders/post.wgsl"));
        let bloom_shader = device.create_shader_module(wgpu::include_wgsl!("shaders/bloom.wgsl"));

        let scene_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("scene"),
            bind_group_layouts: &[Some(&globals_layout)],
            immediate_size: 0,
        });
        let post_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("post"),
            bind_group_layouts: &[Some(&globals_layout), Some(&post_layout)],
            immediate_size: 0,
        });
        let bloom_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("bloom"),
                bind_group_layouts: &[Some(&globals_layout), Some(&bloom_layout)],
                immediate_size: 0,
            });

        let depth = |write: bool| {
            Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(write),
                // Reversed Z: nearer is larger.
                depth_compare: Some(wgpu::CompareFunction::GreaterEqual),
                stencil: Default::default(),
                bias: Default::default(),
            })
        };

        let vertex_attrs =
            wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32, 10 => Float32];
        let instance_attrs = wgpu::vertex_attr_array![
            3 => Float32x4, 4 => Float32x4, 5 => Float32x4, 6 => Float32x4,
            7 => Float32x4, 8 => Float32x4, 9 => Float32x4, 11 => Float32x4
        ];
        let mesh_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("mesh"),
            layout: Some(&scene_layout),
            vertex: wgpu::VertexState {
                module: &mesh_shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[
                    Some(wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<Vertex>() as u64,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &vertex_attrs,
                    }),
                    Some(wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<Instance>() as u64,
                        step_mode: wgpu::VertexStepMode::Instance,
                        attributes: &instance_attrs,
                    }),
                ],
            },
            // Two-sided: procedural kit meshes aren't all consistently wound.
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: depth(true),
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &mesh_shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(HDR_FORMAT.into())],
            }),
            multiview_mask: None,
            cache: None,
        });

        let glow_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("glow"),
            layout: Some(&scene_layout),
            vertex: wgpu::VertexState {
                module: &mesh_shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[
                    Some(wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<Vertex>() as u64,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &vertex_attrs,
                    }),
                    Some(wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<Instance>() as u64,
                        step_mode: wgpu::VertexStepMode::Instance,
                        attributes: &instance_attrs,
                    }),
                ],
            },
            primitive: wgpu::PrimitiveState::default(),
            // Tested against the solid scene, but not hiding each other.
            depth_stencil: depth(false),
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &mesh_shader,
                entry_point: Some("fs_glow"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: HDR_FORMAT,
                    blend: Some(wgpu::BlendState {
                        color: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::One,
                            dst_factor: wgpu::BlendFactor::One,
                            operation: wgpu::BlendOperation::Add,
                        },
                        alpha: wgpu::BlendComponent::REPLACE,
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });

        let sky_attrs = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3];
        let sky_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("sky"),
            layout: Some(&scene_layout),
            vertex: wgpu::VertexState {
                module: &sky_shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<SkyVertex>() as u64,
                    // One star per instance, drawn as a small quad.
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &sky_attrs,
                })],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: depth(false),
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &sky_shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                // Additive: stars too faint and close together to resolve
                // pile their light into the same pixels, which is all the
                // galaxy's glowing band is.
                targets: &[Some(wgpu::ColorTargetState {
                    format: HDR_FORMAT,
                    blend: Some(wgpu::BlendState {
                        color: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::One,
                            dst_factor: wgpu::BlendFactor::One,
                            operation: wgpu::BlendOperation::Add,
                        },
                        alpha: wgpu::BlendComponent::REPLACE,
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });

        // Fullscreen passes: a triangle from the vertex index, no buffers.
        let fullscreen = |label: &str,
                          layout: &wgpu::PipelineLayout,
                          module: &wgpu::ShaderModule,
                          fs: &str,
                          format: wgpu::TextureFormat,
                          depth_stencil: Option<wgpu::DepthStencilState>| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(layout),
                vertex: wgpu::VertexState {
                    module,
                    entry_point: Some("vs"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: Default::default(),
                depth_stencil,
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module,
                    entry_point: Some(fs),
                    compilation_options: Default::default(),
                    targets: &[Some(format.into())],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let bloom_pipelines = [
            "fs_down",
            "fs_blur_h1",
            "fs_blur_v1",
            "fs_blur_h2",
            "fs_blur_v2",
        ]
        .map(|fs| {
            fullscreen(
                fs,
                &bloom_pipeline_layout,
                &bloom_shader,
                fs,
                HDR_FORMAT,
                None,
            )
        });

        let post_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("post"),
            layout: Some(&post_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &post_shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &post_shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(format.into())],
            }),
            multiview_mask: None,
            cache: None,
        });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let targets = Targets::new(
            &device,
            &post_layout,
            &bloom_layout,
            &sampler,
            config.width,
            config.height,
        );

        let instance_capacity = 1024;
        let instance_buffer = instance_buffer(&device, instance_capacity);

        let egui = egui_wgpu::Renderer::new(&device, format, egui_wgpu::RendererOptions::default());

        Self {
            window,
            surface,
            device,
            queue,
            config,
            globals,
            globals_bind,
            mesh_pipeline,
            glow_pipeline,
            sky_pipeline,
            post_pipeline,
            bloom_pipelines,
            post_layout,
            bloom_layout,
            sampler,
            targets,
            meshes: Vec::new(),
            vertices: Vec::new(),
            indices: Vec::new(),
            vertex_buffer: None,
            index_buffer: None,
            dirty_meshes: false,
            instance_buffer,
            instance_capacity,
            sky_buffer: None,
            egui,
            capture: None,
        }
    }

    pub fn size(&self) -> (u32, u32) {
        (self.config.width, self.config.height)
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
        self.targets = Targets::new(
            &self.device,
            &self.post_layout,
            &self.bloom_layout,
            &self.sampler,
            width,
            height,
        );
    }

    /// Register a mesh; it's uploaded before the next frame.
    pub fn add_mesh(&mut self, mesh: &Mesh) -> MeshId {
        let id = MeshId(self.meshes.len());
        self.meshes.push(MeshRange {
            first_index: self.indices.len() as u32,
            index_count: mesh.indices.len() as u32,
            base_vertex: self.vertices.len() as i32,
        });
        self.vertices.extend_from_slice(&mesh.vertices);
        self.indices.extend_from_slice(&mesh.indices);
        self.dirty_meshes = true;
        id
    }

    /// The sky: every other star system, as points at infinity carrying
    /// the light that reaches us from each.
    pub fn set_sky(&mut self, stars: &[SkyVertex]) {
        let buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("sky"),
                contents: bytemuck::cast_slice(stars),
                usage: wgpu::BufferUsages::VERTEX,
            });
        self.sky_buffer = Some((buffer, stars.len() as u32));
    }

    pub fn render(&mut self, mut frame: Frame) {
        // Upload UI textures first, so a skipped frame doesn't lose them.
        if let Some((_, output)) = &mut frame.ui {
            for (id, deltas) in std::mem::take(&mut output.textures_delta.set) {
                for delta in &deltas {
                    self.egui
                        .update_texture(&self.device, &self.queue, id, delta);
                }
            }
        }
        if self.dirty_meshes {
            self.vertex_buffer = Some(self.device.create_buffer_init(
                &wgpu::util::BufferInitDescriptor {
                    label: Some("vertices"),
                    contents: bytemuck::cast_slice(&self.vertices),
                    usage: wgpu::BufferUsages::VERTEX,
                },
            ));
            self.index_buffer = Some(self.device.create_buffer_init(
                &wgpu::util::BufferInitDescriptor {
                    label: Some("indices"),
                    contents: bytemuck::cast_slice(&self.indices),
                    usage: wgpu::BufferUsages::INDEX,
                },
            ));
            self.dirty_meshes = false;
        }

        let surface_texture = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t)
            | wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
            _ => {
                self.surface.configure(&self.device, &self.config);
                return;
            }
        };
        let view = surface_texture.texture.create_view(&Default::default());

        let (w, h) = self.size();
        let globals = Globals {
            view_proj: frame.view_proj.to_cols_array_2d(),
            inv_view_proj: frame.view_proj.inverse().to_cols_array_2d(),
            star_color: [
                frame.star_color[0],
                frame.star_color[1],
                frame.star_color[2],
                1.0,
            ],
            misc: [frame.time, frame.exposure, w as f32, h as f32],
            reveal: frame
                .reveal
                .and_then(|(rel, radius)| reveal_window(frame.view_proj, rel, radius, w, h))
                .unwrap_or([0.0; 4]),
        };
        self.queue
            .write_buffer(&self.globals, 0, bytemuck::bytes_of(&globals));

        // Sort instances by mesh so each mesh is one instanced draw.
        let mut sorted: Vec<(MeshId, Instance)> = frame.instances.to_vec();
        // Solid things first, then glows over them.
        sorted.sort_by_key(|(m, i)| (i.glows(), m.0));
        if sorted.len() > self.instance_capacity {
            self.instance_capacity = sorted.len().next_power_of_two();
            self.instance_buffer = instance_buffer(&self.device, self.instance_capacity);
        }
        let raw: Vec<Instance> = sorted.iter().map(|(_, i)| *i).collect();
        self.queue
            .write_buffer(&self.instance_buffer, 0, bytemuck::cast_slice(&raw));

        let mut encoder = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("scene"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.targets.hdr,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        // Space is black; any light comes from the stars.
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.targets.depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(0.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_bind_group(0, &self.globals_bind, &[]);

            if let Some((buffer, count)) = &self.sky_buffer {
                pass.set_pipeline(&self.sky_pipeline);
                pass.set_vertex_buffer(0, buffer.slice(..));
                pass.draw(0..6, 0..*count);
            }

            if let (Some(vb), Some(ib)) = (&self.vertex_buffer, &self.index_buffer) {
                pass.set_pipeline(&self.mesh_pipeline);
                pass.set_vertex_buffer(0, vb.slice(..));
                pass.set_vertex_buffer(1, self.instance_buffer.slice(..));
                pass.set_index_buffer(ib.slice(..), wgpu::IndexFormat::Uint32);
                let mut start = 0;
                let mut glowing = false;
                while start < sorted.len() {
                    let (mesh, first) = sorted[start];
                    if first.glows() && !glowing {
                        glowing = true;
                        pass.set_pipeline(&self.glow_pipeline);
                    }
                    let end = start
                        + sorted[start..]
                            .iter()
                            .take_while(|(m, i)| *m == mesh && i.glows() == first.glows())
                            .count();
                    let range = &self.meshes[mesh.0];
                    pass.draw_indexed(
                        range.first_index..range.first_index + range.index_count,
                        range.base_vertex,
                        start as u32..end as u32,
                    );
                    start = end;
                }
            }
        }
        // Bloom: HDR -> A (bright pass), then ping-pong A <-> B.
        let t = &self.targets;
        let steps = [
            (&t.from_hdr, &t.bloom_a),
            (&t.from_a, &t.bloom_b),
            (&t.from_b, &t.bloom_a),
            (&t.from_a, &t.bloom_b),
            (&t.from_b, &t.bloom_a),
        ];
        for (pipeline, (src, dst)) in self.bloom_pipelines.iter().zip(steps) {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("bloom"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: dst,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &self.globals_bind, &[]);
            pass.set_bind_group(1, src, &[]);
            pass.draw(0..3, 0..1);
        }
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("post"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.post_pipeline);
            pass.set_bind_group(0, &self.globals_bind, &[]);
            pass.set_bind_group(1, &self.targets.post_bind, &[]);
            pass.draw(0..3, 0..1);
        }

        let mut extra = Vec::new();
        if let Some((ctx, output)) = frame.ui {
            let screen = egui_wgpu::ScreenDescriptor {
                size_in_pixels: [w, h],
                pixels_per_point: output.pixels_per_point,
            };
            let jobs = ctx.tessellate(output.shapes, output.pixels_per_point);
            extra =
                self.egui
                    .update_buffers(&self.device, &self.queue, &mut encoder, &jobs, &screen);
            {
                let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("ui"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                self.egui
                    .render(&mut pass.forget_lifetime(), &jobs, &screen);
            }
            for id in &output.textures_delta.free {
                self.egui.free_texture(id);
            }
        }

        let readback = self.capture.take().and_then(|path| {
            if !self.config.usage.contains(wgpu::TextureUsages::COPY_SRC) {
                eprintln!("screenshot: surface can't be read back");
                return None;
            }
            let row = (w * 4).div_ceil(256) * 256;
            let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("screenshot"),
                size: u64::from(row * h),
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            encoder.copy_texture_to_buffer(
                surface_texture.texture.as_image_copy(),
                wgpu::TexelCopyBufferInfo {
                    buffer: &buffer,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(row),
                        rows_per_image: None,
                    },
                },
                surface_texture.texture.size(),
            );
            Some((path, buffer, row))
        });

        self.queue
            .submit(extra.into_iter().chain([encoder.finish()]));
        if let Some((path, buffer, row)) = readback {
            self.save_screenshot(&path, &buffer, row, w, h);
        }
        self.window.pre_present_notify();
        self.queue.present(surface_texture);
    }
}

impl Renderer {
    fn save_screenshot(
        &self,
        path: &std::path::Path,
        buffer: &wgpu::Buffer,
        row: u32,
        w: u32,
        h: u32,
    ) {
        let slice = buffer.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
        let data = slice.get_mapped_range().expect("map screenshot");
        let bgra = matches!(
            self.config.format,
            wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb
        );
        let mut rgb = Vec::with_capacity((w * h * 3) as usize);
        for y in 0..h {
            let line = &data[(y * row) as usize..(y * row + w * 4) as usize];
            for px in line.as_chunks::<4>().0 {
                if bgra {
                    rgb.extend([px[2], px[1], px[0]]);
                } else {
                    rgb.extend(&px[..3]);
                }
            }
        }
        drop(data);
        buffer.unmap();
        let k = self.shot_scale();
        let (rgb, w, h) = shrink(&rgb, w, h, k);
        let path = png_path(path);
        match write_png(&path, &rgb, w, h) {
            Ok(()) => eprintln!("screenshot: saved {}", path.display()),
            Err(e) => eprintln!("screenshot: {}: {e}", path.display()),
        }
    }
}

/// Screenshots are at most this wide; a wider window is shrunk by a whole
/// factor. That keeps one under about a megabyte, and it is about as much
/// as an image viewer for a playtest shows anyway.
const SHOT_MAX_WIDTH: u32 = 1600;

impl Renderer {
    /// How many window pixels make one screenshot pixel, each way (1 on a
    /// small window, 2 on a 2880 px one). Remote clicks are in screenshot
    /// pixels, so they scale by this too.
    pub fn shot_scale(&self) -> u32 {
        self.config.width.div_ceil(SHOT_MAX_WIDTH).max(1)
    }
}

/// Shrink an RGB frame by `k` each way, averaging each k×k block.
fn shrink(rgb: &[u8], w: u32, h: u32, k: u32) -> (Vec<u8>, u32, u32) {
    if k <= 1 {
        return (rgb.to_vec(), w, h);
    }
    let (sw, sh) = (w / k, h / k);
    let (w, k) = (w as usize, k as usize);
    let mut out = Vec::with_capacity(sw as usize * sh as usize * 3);
    for y in 0..sh as usize {
        for x in 0..sw as usize {
            let mut sum = [0u32; 3];
            for dy in 0..k {
                let row = (y * k + dy) * w;
                for dx in 0..k {
                    let i = (row + x * k + dx) * 3;
                    for c in 0..3 {
                        sum[c] += u32::from(rgb[i + c]);
                    }
                }
            }
            let n = (k * k) as u32;
            out.extend(sum.map(|v| ((v + n / 2) / n) as u8));
        }
    }
    (out, sw, sh)
}

/// Where a screenshot goes: always a `.png`. A path asked for with another
/// extension (old playtest scripts say `shot:x.ppm`) gets `.png` instead, so
/// the file's name says what it holds.
pub fn png_path(path: &std::path::Path) -> std::path::PathBuf {
    if path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("png"))
    {
        path.to_path_buf()
    } else {
        path.with_extension("png")
    }
}

/// Save an RGB frame as a compressed PNG. Screenshots land in tmpfs
/// scratchpads (RAM), so they must be small: a raw 2880 px frame is 13 MB,
/// the PNG of a game frame (mostly black space) well under one.
fn write_png(path: &std::path::Path, rgb: &[u8], w: u32, h: u32) -> Result<(), String> {
    let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w, h);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    enc.set_compression(png::Compression::High);
    enc.set_filter(png::Filter::Adaptive);
    let mut writer = enc.write_header().map_err(|e| e.to_string())?;
    writer.write_image_data(rgb).map_err(|e| e.to_string())?;
    writer.finish().map_err(|e| e.to_string())
}

fn texture_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

/// The offscreen render targets, sized to the window, and the bind groups
/// that read them.
struct Targets {
    hdr: wgpu::TextureView,
    depth: wgpu::TextureView,
    /// Quarter-resolution bloom ping-pong pair.
    bloom_a: wgpu::TextureView,
    bloom_b: wgpu::TextureView,
    from_hdr: wgpu::BindGroup,
    from_a: wgpu::BindGroup,
    from_b: wgpu::BindGroup,
    post_bind: wgpu::BindGroup,
}

impl Targets {
    fn new(
        device: &wgpu::Device,
        post_layout: &wgpu::BindGroupLayout,
        bloom_layout: &wgpu::BindGroupLayout,
        sampler: &wgpu::Sampler,
        width: u32,
        height: u32,
    ) -> Self {
        let make = |format, label, width: u32, height: u32| {
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: wgpu::Extent3d {
                        width: width.max(1),
                        height: height.max(1),
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                        | wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                })
                .create_view(&Default::default())
        };
        let hdr = make(HDR_FORMAT, "hdr", width, height);
        let depth = make(DEPTH_FORMAT, "depth", width, height);
        let bloom_a = make(HDR_FORMAT, "bloom a", width / 4, height / 4);
        let bloom_b = make(HDR_FORMAT, "bloom b", width / 4, height / 4);
        let read = |view: &wgpu::TextureView| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("bloom"),
                layout: bloom_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(sampler),
                    },
                ],
            })
        };
        let post_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("post"),
            layout: post_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&hdr),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&bloom_a),
                },
            ],
        });
        Self {
            from_hdr: read(&hdr),
            from_a: read(&bloom_a),
            from_b: read(&bloom_b),
            post_bind,
            hdr,
            depth,
            bloom_a,
            bloom_b,
        }
    }
}

/// The window through the pilot's hull round a target at `rel` (camera
/// relative) of `radius` metres: its centre and radius in pixels, at full
/// strength. Never smaller than the HUD's bracket round the target, so
/// the bracket frames open space, and wider than the target itself, so
/// its whole silhouette shows (DESIGN §7.2.1).
pub fn reveal_window(view_proj: Mat4, rel: Vec3, radius: f32, w: u32, h: u32) -> Option<[f32; 4]> {
    let clip = view_proj * rel.extend(1.0);
    if clip.w <= 0.0 {
        return None;
    }
    let ndc = clip.truncate() / clip.w;
    let (w, h) = (w as f32, h as f32);
    // The lens's vertical scale (the length of clip y's row: the view's
    // rotation keeps it), in pixels per metre at a metre's distance.
    let row_y = Vec3::new(view_proj.x_axis.y, view_proj.y_axis.y, view_proj.z_axis.y);
    let focal = row_y.length() * 0.5 * h;
    let target_px = radius / clip.w * focal;
    let r = (target_px * 1.4 + 0.01 * h).max(0.035 * h);
    Some([(ndc.x + 1.0) * 0.5 * w, (1.0 - ndc.y) * 0.5 * h, r, 1.0])
}

fn instance_buffer(device: &wgpu::Device, capacity: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("instances"),
        size: (capacity * std::mem::size_of::<Instance>()) as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lens() -> Mat4 {
        // The game's own lens (main.rs).
        glam::camera::rh::proj::directx::perspective_infinite_reverse(
            70f32.to_radians(),
            1920.0 / 1080.0,
            0.5,
        )
    }

    #[test]
    fn the_window_is_centred_on_the_target() {
        let [x, y, r, strength] =
            reveal_window(lens(), Vec3::new(0.0, 0.0, -600.0), 8.0, 1920, 1080).unwrap();
        assert!((x - 960.0).abs() < 0.5 && (y - 540.0).abs() < 0.5);
        assert_eq!(strength, 1.0);
        // A distant fighter is a few pixels: the window is the bracket's
        // size, not the ship's.
        assert!((r - 0.035 * 1080.0).abs() < 0.5, "{r}");
    }

    #[test]
    fn a_close_target_gets_a_window_round_all_of_it() {
        let near = Vec3::new(0.0, -20.0, -60.0);
        let [_, y, r, _] = reveal_window(lens(), near, 12.0, 1920, 1080).unwrap();
        assert!(y > 540.0, "below the centre");
        // The window is wider than the target's own radius on screen.
        let focal = 0.5 * 1080.0 / (35f32.to_radians()).tan();
        let size = 12.0 / 60.0 * focal;
        assert!(r > size, "{r} vs {size}");
    }

    #[test]
    fn nothing_opens_for_a_target_behind_the_camera() {
        assert!(reveal_window(lens(), Vec3::new(0.0, 0.0, 50.0), 8.0, 1920, 1080).is_none());
    }

    #[test]
    fn shrink_averages_blocks() {
        // 2×2 → 1×1: the average of four pixels.
        let rgb = [0, 10, 200, 2, 10, 200, 4, 10, 200, 6, 11, 201];
        assert_eq!(super::shrink(&rgb, 2, 2, 2), (vec![3, 10, 200], 1, 1));
        assert_eq!(super::shrink(&rgb, 2, 2, 1).0, rgb.to_vec());
    }
}
