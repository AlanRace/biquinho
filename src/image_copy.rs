// Using: https://github.com/DGriffin91/bevy/tree/frame_capture_tool

use std::sync::Arc;

use bevy::prelude::*;
use bevy::render::render_asset::RenderAssets;
use bevy::render::render_graph::{self, NodeRunError, RenderGraph, RenderGraphContext};
use bevy::render::renderer::{RenderContext, RenderDevice, RenderQueue};
use bevy::render::{Extract, RenderApp, RenderStage};

use bevy::render::render_resource::{
    Buffer, BufferDescriptor, BufferUsages, CommandEncoderDescriptor, Extent3d, ImageCopyBuffer,
    ImageDataLayout,
};
use pollster::FutureExt;

use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};

pub fn receive_images(
    image_copiers: Query<&ImageCopier>,
    mut images: ResMut<Assets<Image>>,
    render_device: Res<RenderDevice>,
) {
    for image_copier in image_copiers.iter() {
        if !image_copier.enabled() {
            continue;
        }
        // Derived from: https://sotrh.github.io/learn-wgpu/showcase/windowless/#a-triangle-without-a-window
        // We need to scope the mapping variables so that we can
        // unmap the buffer
        async {
            let buffer_slice = image_copier.buffer.slice(..);

            // NOTE: We have to create the mapping THEN device.poll() before await
            // the future. Otherwise the application will freeze.
            let (tx, rx) = futures_intrusive::channel::shared::oneshot_channel();
            buffer_slice.map_async(wgpu::MapMode::Read, move |result| {
                tx.send(result).unwrap();
            });
            render_device.poll(wgpu::Maintain::Wait);
            rx.receive().await.unwrap().unwrap();
            if let Some(mut image) = images.get_mut(&image_copier.dst_image) {
                image.data = buffer_slice.get_mapped_range().to_vec();
            }

            image_copier.buffer.unmap();
        }
        .block_on();

        image_copier.copied();
    }
}

pub const IMAGE_COPY: &str = "image_copy";

pub struct ImageCopyPlugin;
impl Plugin for ImageCopyPlugin {
    fn build(&self, app: &mut App) {
        let render_app = app.add_system(receive_images).sub_app_mut(RenderApp);

        render_app.add_system_to_stage(RenderStage::Extract, image_copy_extract);

        let mut graph = render_app.world.get_resource_mut::<RenderGraph>().unwrap();

        graph.add_node(IMAGE_COPY, ImageCopyDriver::default());

        graph
            .add_node_edge(IMAGE_COPY, bevy::render::main_graph::node::CAMERA_DRIVER)
            .unwrap();
    }
}

#[derive(Clone, Default, Resource, Deref, DerefMut)]
pub struct ImageCopiers(pub Vec<ImageCopier>);

#[derive(Clone, Component)]
pub struct ImageCopier {
    buffer: Buffer,
    enabled: Arc<AtomicBool>,
    src_image: Handle<Image>,
    dst_image: Handle<Image>,

    padded_bytes_per_row: usize,

    copied: Arc<AtomicI32>,
    rendered: Arc<AtomicI32>,
}

impl ImageCopier {
    pub fn new(
        src_image: Handle<Image>,
        dst_image: Handle<Image>,
        size: Extent3d,
        render_device: &RenderDevice,
    ) -> ImageCopier {
        let padded_bytes_per_row =
            RenderDevice::align_copy_bytes_per_row((size.width) as usize * 4);

        let cpu_buffer = render_device.create_buffer(&BufferDescriptor {
            label: None,
            size: padded_bytes_per_row as u64 * size.height as u64,
            usage: BufferUsages::MAP_READ | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        ImageCopier {
            buffer: cpu_buffer,
            src_image,
            dst_image,
            enabled: Arc::new(AtomicBool::new(true)),
            padded_bytes_per_row,

            copied: Arc::new(AtomicI32::new(0)),
            rendered: Arc::new(AtomicI32::new(0)),
        }
    }

    pub fn enable(&self) {
        self.enabled.store(true, Ordering::Relaxed);
    }

    pub fn disable(&self) {
        self.enabled.store(false, Ordering::Relaxed);
    }

    pub fn enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    pub fn padded_bytes_per_row(&self) -> usize {
        self.padded_bytes_per_row
    }

    pub fn copy_count(&self) -> i32 {
        self.copied.load(Ordering::Relaxed)
    }
    pub fn render_count(&self) -> i32 {
        self.rendered.load(Ordering::Relaxed)
    }

    pub fn copied(&self) {
        self.copied.fetch_add(1, Ordering::Relaxed);
    }

    pub fn rendered(&self) {
        self.rendered.fetch_add(1, Ordering::Relaxed);
    }
}

pub fn image_copy_extract(mut commands: Commands, image_copiers: Extract<Query<&ImageCopier>>) {
    commands.insert_resource(ImageCopiers(
        image_copiers.iter().cloned().collect::<Vec<ImageCopier>>(),
    ));
}

#[derive(Default)]
pub struct ImageCopyDriver;

impl render_graph::Node for ImageCopyDriver {
    fn run(
        &self,
        _graph: &mut RenderGraphContext,
        render_context: &mut RenderContext,
        world: &World,
    ) -> Result<(), NodeRunError> {
        let image_copiers = world.get_resource::<ImageCopiers>().unwrap();
        let gpu_images = world.get_resource::<RenderAssets<Image>>().unwrap();

        for image_copier in image_copiers.iter() {
            if !image_copier.enabled() {
                continue;
            }

            let src_image = gpu_images.get(&image_copier.src_image).unwrap();

            let mut encoder = render_context
                .render_device
                .create_command_encoder(&CommandEncoderDescriptor::default());

            let format = src_image.texture_format.describe();

            let padded_bytes_per_row = RenderDevice::align_copy_bytes_per_row(
                (src_image.size.x as usize / format.block_dimensions.0 as usize)
                    * format.block_size as usize,
            );

            let texture_extent = Extent3d {
                width: src_image.size.x as u32,
                height: src_image.size.y as u32,
                depth_or_array_layers: 1,
            };

            encoder.copy_texture_to_buffer(
                src_image.texture.as_image_copy(),
                ImageCopyBuffer {
                    buffer: &image_copier.buffer,
                    layout: ImageDataLayout {
                        offset: 0,
                        bytes_per_row: Some(
                            std::num::NonZeroU32::new(padded_bytes_per_row as u32).unwrap(),
                        ),
                        rows_per_image: None,
                    },
                },
                texture_extent,
            );

            let render_queue = world.get_resource::<RenderQueue>().unwrap();
            render_queue.submit(std::iter::once(encoder.finish()));

            image_copier.rendered();
        }

        Ok(())
    }
}
