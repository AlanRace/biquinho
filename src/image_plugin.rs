use bevy::{
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
    sprite::Anchor,
    tasks::{AsyncComputeTaskPool, Task},
};
use futures_lite::future;
use image::{GenericImageView, RgbaImage};
use imc_rs::error::MCDError;
use nalgebra::Vector3;
use std::{collections::HashMap, time::Instant};

use crate::{
    camera::{Draggable, DraggedEvent, Selectable, SizedEntity},
    transform::AffineTransform,
    ui::UiLabel,
    Message,
};

/// ImagePlugin
///
/// This includes all events and systems required to load and visualise images. This also includes
/// features for handling tiled images (or tiling a large image) and channel images.
///
/// The easiest way to interact with this plugin is via an `ImageEvent`. This plugin will
/// consume all `ImageEvent`s present at each frame and issue the respective commands.
pub struct ImagePlugin;

impl Plugin for ImagePlugin {
    fn build(&self, app: &mut App) {
        app.add_event::<ImageEvent>()
            .add_system(marker_moved)
            .add_system(enable_registration)
            .add_system(update_loaded_image)
            .add_system(split_image_into_tiles)
            .add_system(spawn_tiles)
            .add_system(
                handle_image_events
                    .label("image_events")
                    .after(UiLabel::HandleUiEvent),
            );
    }
}

/// Image events define ways to interact with this plugin.
#[derive(Clone, Copy)]
pub enum ImageEvent {
    /// Set the visibility of the image with the given `Entity`. This is automatically propagated
    /// to all children entities.
    SetVisibility(Entity, bool),
    /// Set the opacity of the image with the given `Entity`.
    SetOpacity(Entity, f32),
    /// Set the ability to drag the image with the given `Entity`. If this is set to `true` then the `Selectable`
    /// component is added to this entity. Selecting and dragging is then handled by the `CameraPlugin`.
    SetDragging(Entity, bool),
    /// Set the colour domain of the image with the given `Entity`. This defines the minimum and maximum intensity
    /// values that are used to set the range for the colour scale.
    ///
    /// This is only relevant for images which have an `ImageControl` component. This is currently only channel
    /// images (e.g. from IMC data).
    SetColourDomain(Entity, (f32, f32)),
    /// Toggle the registration tool.
    ToggleRegistration(Entity, bool),
}

/// Handle image events
fn handle_image_events(
    mut commands: Commands,
    mut image_events: EventReader<ImageEvent>,
    mut q_image: Query<&mut ImageControl>,
    mut q_sprite: Query<&mut Sprite>,
    mut q_visibility: Query<&mut Visibility>,
    mut q_opacity: Query<&mut Opacity>,
) {
    for event in image_events.iter() {
        match event {
            ImageEvent::SetColourDomain(entity, colour_domain) => {
                if let Ok(mut image_control) = q_image.get_mut(*entity) {
                    image_control.colour_domain = *colour_domain;
                }
            }
            ImageEvent::SetOpacity(entity, opacity) => {
                if let Ok(mut opacity_component) = q_opacity.get_mut(*entity) {
                    opacity_component.0 = *opacity;
                }

                if let Ok(mut sprite) = q_sprite.get_mut(*entity) {
                    sprite.color.set_a(*opacity);
                }
            }
            ImageEvent::SetVisibility(entity, is_visible) => {
                if let Ok(mut visibility) = q_visibility.get_mut(*entity) {
                    visibility.is_visible = *is_visible;
                }
            }
            ImageEvent::SetDragging(entity, allow_dragging) => {
                // let sprite = q_sprite.get(*entity);
                // let tiled_image = q_tiled_image.get(*entity);

                // let (width, height) = if let Ok((sprite, image)) = sprite {
                //     if let Some(custom_size) = sprite.custom_size {
                //         (custom_size.x, custom_size.y)
                //     } else {
                //         let image = images.get(image).unwrap();
                //         (image.size().x, image.size().y)
                //     }
                // } else {
                //     let size = tiled_image.unwrap().0.size;

                //     (size.x, size.y)
                // };

                if *allow_dragging {
                    //commands.entity(*entity).insert(Draggable);
                    debug!("Allowing (ImageEvent) dragging: {:?}", entity);
                    commands.entity(*entity).insert(Selectable::default());
                } else {
                    //commands.entity(*entity).remove::<Draggable>();
                    commands.entity(*entity).remove::<Selectable>();
                }
            }
            ImageEvent::ToggleRegistration(entity, allow_registration) => {
                commands.entity(*entity).insert(EnableRegistration);
            }
        }
    }
}

#[derive(Debug)]
pub enum ImageUpdateType {
    Red,
    Green,
    Blue,
    All,
}
#[derive(Debug, Component)]
struct EnableRegistration;

fn enable_registration(
    mut commands: Commands,
    q_sized_entity: Query<SizedEntity, With<EnableRegistration>>,
    images: Res<Assets<Image>>,
) {
    for sized in q_sized_entity.iter() {
        commands.entity(sized.entity).remove::<EnableRegistration>();

        let top_left = sized.top_left(&images).unwrap();
        let bottom_right = sized.bottom_right(&images).unwrap();

        let marker_size_fixed = 1000.0;
        let marker_size_moving = (bottom_right.x - top_left.x) / 100.0;

        let marker_1 = commands
            .spawn_bundle(SpriteBundle {
                sprite: Sprite {
                    color: Color::RED,
                    custom_size: Some(Vec2::new(marker_size_fixed, marker_size_fixed)),
                    ..default()
                },
                transform: Transform::from_xyz(0.0, 0.0, 10.0),
                ..default()
            })
            .insert(Selectable::default())
            .insert(Draggable)
            .insert(WorldMarker)
            .id();

        let marker_2 = commands
            .spawn_bundle(SpriteBundle {
                sprite: Sprite {
                    color: Color::GREEN,
                    custom_size: Some(Vec2::new(marker_size_fixed, marker_size_fixed)),
                    ..default()
                },
                transform: Transform::from_xyz(0.0, 25000.0, 10.0),
                ..default()
            })
            .insert(Selectable::default())
            .insert(Draggable)
            .insert(WorldMarker)
            .id();

        let marker_3 = commands
            .spawn_bundle(SpriteBundle {
                sprite: Sprite {
                    color: Color::BLUE,
                    custom_size: Some(Vec2::new(marker_size_fixed, marker_size_fixed)),
                    ..default()
                },
                transform: Transform::from_xyz(75000.0, 25000.0, 10.0),
                ..default()
            })
            .insert(Selectable::default())
            .insert(Draggable)
            .insert(WorldMarker)
            .id();

        commands.entity(sized.entity).with_children(|parent| {
            parent
                .spawn_bundle(SpriteBundle {
                    sprite: Sprite {
                        color: Color::RED,
                        custom_size: Some(Vec2::new(marker_size_moving, marker_size_moving)),
                        ..default()
                    },
                    transform: Transform::from_xyz(top_left.x, top_left.y, 10.0),
                    ..default()
                })
                .insert(Selectable::default())
                .insert(Draggable)
                .insert(ImageMarker {
                    world_marker: marker_1,
                });

            parent
                .spawn_bundle(SpriteBundle {
                    sprite: Sprite {
                        color: Color::GREEN,
                        custom_size: Some(Vec2::new(marker_size_moving, marker_size_moving)),
                        ..default()
                    },
                    transform: Transform::from_xyz(top_left.x, bottom_right.y, 10.0),
                    ..default()
                })
                .insert(Selectable::default())
                .insert(Draggable)
                .insert(ImageMarker {
                    world_marker: marker_2,
                });

            parent
                .spawn_bundle(SpriteBundle {
                    sprite: Sprite {
                        color: Color::BLUE,
                        custom_size: Some(Vec2::new(marker_size_moving, marker_size_moving)),
                        ..default()
                    },
                    transform: Transform::from_xyz(bottom_right.x, bottom_right.y, 10.0),
                    ..default()
                })
                .insert(Selectable::default())
                .insert(Draggable)
                .insert(ImageMarker {
                    world_marker: marker_3,
                });
        });
    }
}

#[derive(Debug, Component)]
struct WorldMarker;

#[derive(Debug, Component)]
struct ImageMarker {
    world_marker: Entity,
}

fn marker_moved(
    mut events: ResMut<Events<DraggedEvent>>,
    q_image_markers: Query<(&ImageMarker, &Transform, &Parent)>,
    //q_changed_image_marker: Query<&ImageMarker, Changed<GlobalTransform>>,
    q_world_markers: Query<(&WorldMarker, &GlobalTransform)>,
    //q_changed_world_marker: Query<&WorldMarker, Changed<GlobalTransform>>,
    mut q_transforms: Query<&mut Transform, (Without<ImageMarker>, Without<WorldMarker>)>,
) {
    // Process events related to dragging an Image/World marker for alignment
    // Any other events we should leave alone, so add them back to the event list once we are finished
    let mut unprocessed_events = Vec::new();

    for event in events.drain() {
        let image_marker = q_image_markers.get(event.0);
        let world_marker = q_world_markers.get(event.0);

        if image_marker.is_err() && world_marker.is_err() {
            unprocessed_events.push(event);
            continue;
        }

        // One of the markers was dragged, so lets update the transform

        let mut fixed_points = Vec::with_capacity(3);
        let mut moving_points = Vec::with_capacity(3);

        let mut to_transform = None;

        for (image_marker, image_transform, parent) in q_image_markers.iter() {
            let (world_marker, world_transform) =
                q_world_markers.get(image_marker.world_marker).unwrap();

            let world_translation = world_transform.translation();

            fixed_points.push(Vector3::new(
                world_translation.x as f64,
                world_translation.y as f64,
                0.0,
            ));
            moving_points.push(Vector3::new(
                image_transform.translation.x as f64,
                image_transform.translation.y as f64,
                0.0,
            ));

            to_transform = Some(parent);
        }

        println!("{:?}", fixed_points);
        println!("{:?}", moving_points);

        if let Some(parent) = to_transform {
            let transform = AffineTransform::from_points(
                "affine_transform".to_string(),
                fixed_points,
                moving_points,
            );

            let mut parent = q_transforms.get_mut(**parent).unwrap();

            let z = parent.translation.z;

            parent
                .set(Box::new(
                    Transform::from_xyz(0.0, 0.0, z)
                        .mul_transform(Transform::from_matrix(transform.into())),
                ))
                .unwrap();
        }
    }

    events.extend(unprocessed_events.drain(..));
}

#[derive(Component, Debug)]
pub struct ImageControl {
    // List of all entities that are controlled by this control
    pub description: String,

    pub entities: HashMap<u16, Entity>,
    pub image_update_type: ImageUpdateType,
    pub intensity_range: (f32, f32),
    pub histogram: Vec<usize>,

    pub colour_domain: (f32, f32),
}

#[derive(Component)]
pub struct Opacity(pub f32);

#[derive(Default, Component)]
pub struct TiledImage {
    // colour: Color,
    pub size: Vec2,
}

fn div_ceil(x: u32, y: u32) -> u32 {
    1 + ((x - 1) / y)
}

#[derive(Component)]
pub struct ComputeImage(pub Task<Image>);

fn update_loaded_image(
    mut commands: Commands,
    mut textures: ResMut<Assets<Image>>,
    mut q_sprite: Query<(Entity, &mut ComputeImage)>,
) {
    for (entity, mut task) in q_sprite.iter_mut() {
        if let Some(image) = future::block_on(future::poll_once(&mut task.0)) {
            let image_handle = textures.add(image);

            // Task is complete, so remove task component from entity
            commands
                .entity(entity)
                .remove::<ComputeImage>()
                .insert(image_handle);
        }
    }
}

pub struct ToTileImage {
    pub image: RgbaImage,

    pub tile_width: u32,
    pub tile_height: u32,

    pub image_width: f32,
    pub image_height: f32,
}

pub struct Tile {
    pub image: Image,
    pub transform: Transform,
    pub sprite: Sprite,
}

#[derive(Component)]
pub struct ComputeTileImage(pub Task<Result<ToTileImage, MCDError>>);

#[derive(Component)]
pub struct SpawnTiles(pub Task<Vec<Tile>>);

fn spawn_tiles(
    mut commands: Commands,
    mut textures: ResMut<Assets<Image>>,
    mut q_spawn: Query<(Entity, &mut SpawnTiles)>,
) {
    for (entity, mut task) in q_spawn.iter_mut() {
        if let Some(tiles) = future::block_on(future::poll_once(&mut task.0)) {
            for tile in tiles {
                let texture_handle = textures.add(tile.image);

                let tile_entity = commands
                    .spawn(SpriteBundle {
                        texture: texture_handle,
                        transform: tile.transform,
                        sprite: tile.sprite,
                        ..Default::default()
                    })
                    .id();

                commands.entity(entity).add_child(tile_entity);
            }

            // Task is complete, so remove task component from entity
            commands.entity(entity).remove::<SpawnTiles>();
        }
    }
}

/// Split up an image into tiles to ensure that we have small enough textures when displaying.
/// TODO: This code is relatively slow - taking up to 0.5 s to split up an image - presumably due to the copying of
/// data. Not sure whether it is possible to avoid so many copies to improve performance here.
fn split_image_into_tiles(
    mut commands: Commands,
    mut q_totile: Query<(Entity, &mut ComputeTileImage)>,
) {
    let thread_pool = AsyncComputeTaskPool::get();

    for (entity, mut task) in q_totile.iter_mut() {
        if let Some(to_tile) = future::block_on(future::poll_once(&mut task.0)) {
            match to_tile {
                Ok(to_tile) => {
                    let image_task = thread_pool.spawn(async move {
                        let mut tiles: Vec<Tile> = Vec::new();

                        let tiles_x = div_ceil(to_tile.image.width(), to_tile.tile_width);
                        let tiles_y = div_ceil(to_tile.image.height(), to_tile.tile_height);

                        let start = Instant::now();

                        let image_width_pixels = to_tile.image.width();
                        let image_height_pixels = to_tile.image.height();

                        for tile_y in 0..tiles_y {
                            for tile_x in 0..tiles_x {
                                let start_x = tile_x * to_tile.tile_width;
                                let start_y = tile_y * to_tile.tile_height;
                                let tile_width = image_width_pixels
                                    .min((tile_x + 1) * to_tile.tile_width)
                                    - start_x;
                                let tile_height = image_height_pixels
                                    .min((tile_y + 1) * to_tile.tile_height)
                                    - start_y;

                                let tile = to_tile
                                    .image
                                    .view(start_x, start_y, tile_width, tile_height)
                                    .to_image();

                                let image_texture = Image::from_dynamic(tile.into(), false);

                                // let data: Vec<u8> = tile.to_image().into_vec();

                                // let image_texture = Image::new(
                                //     Extent3d {
                                //         width: tile_width,
                                //         height: tile_height,
                                //         depth_or_array_layers: 1,
                                //     },
                                //     TextureDimension::D2,
                                //     data,
                                //     TextureFormat::Rgba8Unorm,
                                // );

                                let tile_width_um = (tile_width as f32 / image_width_pixels as f32)
                                    * to_tile.image_width;

                                let tile_height_um = (tile_height as f32
                                    / image_height_pixels as f32)
                                    * to_tile.image_height;

                                // Transformation is defined with respect to slide, so make sure to offset properly
                                let transform = Transform::from_xyz(
                                    (start_x as f32 / image_width_pixels as f32)
                                        * to_tile.image_width
                                        - (to_tile.image_width / 2.0),
                                    ((image_height_pixels - start_y) as f32
                                        / image_height_pixels as f32)
                                        * to_tile.image_height
                                        - (to_tile.image_height / 2.0),
                                    0.0,
                                );

                                let sprite = Sprite {
                                    custom_size: Some(Vec2::new(tile_width_um, tile_height_um)),
                                    anchor: Anchor::TopLeft,
                                    ..Default::default()
                                };

                                // (entity, image_texture, transform, sprite)

                                // let texture_handle = textures.add(image_texture);

                                // let tile_entity = commands
                                //     .spawn(SpriteBundle {
                                //         texture: texture_handle,
                                //         transform,
                                //         sprite: Sprite {
                                //             custom_size: Some(Vec2::new(
                                //                 tile_width_um,
                                //                 tile_height_um,
                                //             )),
                                //             anchor: Anchor::TopLeft,
                                //             ..Default::default()
                                //         },
                                //         ..Default::default()
                                //     })
                                //     .id();

                                // commands.entity(entity).add_child(tile_entity);

                                tiles.push(Tile {
                                    image: image_texture,
                                    transform,
                                    sprite,
                                })
                            }
                        }

                        info!("Took {:?} to generate tiles!", start.elapsed());

                        tiles
                    });

                    commands.entity(entity).insert(SpawnTiles(image_task));
                }
                Err(error) => {
                    commands.spawn(Message {
                        severity: crate::Severity::Error,
                        message: error.to_string(),
                    });
                }
            }

            // Task is complete, so remove task component from entity
            commands.entity(entity).remove::<ComputeTileImage>();
        }
    }
}
