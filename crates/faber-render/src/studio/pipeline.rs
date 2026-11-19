use super::*;
use wgpu::util::DeviceExt;
pub(crate) struct Pipelines {
    pub material: wgpu::RenderPipeline,
    pub shadow: wgpu::RenderPipeline,
    pub floor: wgpu::RenderPipeline,
    pub cap: wgpu::RenderPipeline,
    pub uniform: wgpu::Buffer,
    pub scene: wgpu::BindGroup,
    pub mask: wgpu::BindGroup,
    pub mask_view: wgpu::TextureView,
}
impl Pipelines {
    pub fn new(device: &wgpu::Device) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Studio lighting"),
            source: wgpu::ShaderSource::Wgsl(include_str!("studio.wgsl").into()),
        });
        let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Studio camera"),
            contents: bytemuck::bytes_of(&Uniform::zeroed()),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
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
        let scene = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            }],
        });
        let mask_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
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
            ],
        });
        let mask_view = device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("Studio ground shadow"),
                size: wgpu::Extent3d {
                    width: 1024,
                    height: 1024,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::R8Unorm,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
            .create_view(&Default::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let mask = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &mask_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&mask_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });
        let surface_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let ground_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&layout), Some(&mask_layout)],
            immediate_size: 0,
        });
        let verts = wgpu::vertex_attr_array![0=>Float32x3,1=>Float32x3,2=>Float32x3,3=>Float32x3];
        let transforms = wgpu::vertex_attr_array![4=>Float32x4,5=>Float32x4,6=>Float32x4,7=>Float32x4,8=>Float32x4];
        let mesh = [
            Some(wgpu::VertexBufferLayout {
                array_stride: 48,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &verts,
            }),
            Some(wgpu::VertexBufferLayout {
                array_stride: 80,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &transforms,
            }),
        ];
        let caps = wgpu::vertex_attr_array![0=>Float32x3,1=>Float32x3,2=>Float32x3,3=>Float32];
        let cap_mesh = [Some(wgpu::VertexBufferLayout {
            array_stride: 40,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &caps,
        })];
        let make = |vs: &str,
                    fs: &str,
                    layout: &wgpu::PipelineLayout,
                    buffers: &[Option<wgpu::VertexBufferLayout<'_>>],
                    shadow: bool| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(vs),
                layout: Some(layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some(vs),
                    compilation_options: Default::default(),
                    buffers,
                },
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(fs),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: if shadow {
                            wgpu::TextureFormat::R8Unorm
                        } else {
                            crate::FORMAT
                        },
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: Default::default(),
                depth_stencil: (!shadow).then_some(wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth32Float,
                    depth_write_enabled: Some(true),
                    depth_compare: Some(wgpu::CompareFunction::LessEqual),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: wgpu::MultisampleState {
                    count: if shadow { 1 } else { 4 },
                    ..Default::default()
                },
                multiview_mask: None,
                cache: None,
            })
        };
        Self {
            material: make("vs", "fs", &surface_layout, &mesh, false),
            shadow: make("shadow_vs", "shadow_fs", &surface_layout, &mesh, true),
            floor: make("floor_vs", "floor_fs", &ground_layout, &[], false),
            cap: make("cap_vs", "cap_fs", &surface_layout, &cap_mesh, false),
            mask,
            mask_view,
            uniform,
            scene,
        }
    }
}
