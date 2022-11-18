use arboard::Clipboard;
use bevy::{
    core_pipeline::clear_color::ClearColorConfig,
    ecs::query::WorldQuery,
    input::mouse::{MouseScrollUnit, MouseWheel},
    prelude::*,
    render::{
        camera::{RenderTarget, Viewport},
        render_resource::{
            Extent3d, TextureDescriptor, TextureDimension, TextureFormat, TextureUsages,
        },
        renderer::RenderDevice,
        view::RenderLayers,
        RenderStage,
    },
    window::{WindowId, WindowResized},
};
use bevy_egui::EguiContext;
use pollster::FutureExt;
use wgpu::{BufferDescriptor, BufferUsages};

use crate::{
    image_copy::{ImageCopier, ImageCopyPlugin},
    ui::{UiLabel, UiSpace},
    Message, Severity,
};

/// CameraPlugin
///
/// This includes all events and systems for visualising and interacting with (e.g. selecting/dragging) data
/// through one or more cameras.
///
/// The easiest way to interact with this plugin is via an [`CameraEvent`]. This plugin will
/// consume all [`CameraEvent`]s present at each frame and issue the respective commands.
#[derive(Default)]
pub struct CameraPlugin {
    /// The initial [`CameraSetup`] which is copied over to be the resource describing the camera setup.
    pub camera_setup: CameraSetup,
}

impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(self.camera_setup.clone())
            .add_plugin(ImageCopyPlugin)
            .add_startup_system(setup)
            // .add_startup_system(set_camera_viewports.after("initial_setup"))
            .add_event::<CameraEvent>()
            .add_event::<DraggedEvent>()
            .add_system(
                window_resized, // .after("handle_camera_event")
                                // .after(UiLabel::Display),
            )
            .add_system(ui_changed)
            .add_system(copy_to_clipboard.before("handle_camera_event")) // This should be before handling camera events, to force it to be run on the next frame - otherwise the screenshot is empty
            .add_system_to_stage(CoreStage::Update, update_camera)
            .add_system(handle_camera_event.label("handle_camera_event"))
            .add_system(
                update_mouse_position
                    .label("mouse_update")
                    .after(UiLabel::Display),
            )
            .add_system_to_stage(CoreStage::PreUpdate, changed_camera_setup)
            .add_system(select_object.after("mouse_update"))
            .add_system(camera_zoom.after("mouse_update"))
            .add_system(selected.after("mouse_update"))
            .add_system(dragging.after("mouse_update"))
            .add_system(dragging_camera.after("mouse_update"));
    }
}

/// Camera events define ways to interact with this plugin.
#[derive(Clone)]
pub enum CameraEvent {
    /// Enables dragging on all cameras displaying data. This allows the user to drag the view by clicking and dragging.
    EnableDragging,
    /// Disables dragging on all cameras displaying data.
    DisableDragging,
    /// Set the number of cameras to display data and how they should be layed out in a rectangular grid.
    SetGrid((u32, u32)),
    /// Set the name of the camera with the given [`Entity`].
    SetName((Entity, String)),
    /// Set the position of the camera with the given [`Entity`]. This has the effect of setting the center of the camera's view
    /// to be at the given location.
    LookAt((Entity, Vec3)),
    /// Set the scale of all cameras displaying data to the given value. This has the effect of zooming in or out. All cameras are kept
    /// in-sync.
    Zoom(f32),

    CopyToClipboard,
}

/// Handle all camera events
fn handle_camera_event(
    mut commands: Commands,
    mut ev_camera: EventReader<CameraEvent>,
    mut q_camera: Query<(Entity, &PanCamera, &mut Transform)>,
    mut q_text: Query<&mut Text>,
    mut windows: ResMut<Windows>,
    mut camera_setup: ResMut<CameraSetup>,
    images: Res<Assets<Image>>,
    render_device: Res<RenderDevice>,
) {
    let window = windows.primary_mut();

    for event in ev_camera.iter() {
        match event {
            CameraEvent::EnableDragging => {
                // Re-enable the camera
                for (camera, _, _) in q_camera.iter() {
                    println!("Enabling dragging {:?}", camera);
                    commands.entity(camera).insert(Selectable { priority: -1 });
                    println!("Dragging enabled for {:?}", camera);
                }

                window.set_cursor_icon(CursorIcon::Hand);
            }
            CameraEvent::DisableDragging => {
                // Disable the camera
                for (camera, _, _) in q_camera.iter() {
                    commands.entity(camera).remove::<Selectable>();
                }

                window.set_cursor_icon(CursorIcon::Default);
            }
            CameraEvent::SetGrid((x, y)) => {
                if *x != camera_setup.x || *y != camera_setup.y {
                    camera_setup.x = *x;
                    camera_setup.y = *y;
                }
            }
            CameraEvent::SetName((entity, name)) => {
                if let Ok((_, camera, _)) = q_camera.get(*entity) {
                    if let Ok(mut text) = q_text.get_mut(camera.camera_text) {
                        text.sections[0].value = name.clone();
                    }
                }
            }
            CameraEvent::LookAt((entity, position)) => {
                if let Ok((_, _, mut transform)) = q_camera.get_mut(*entity) {
                    transform.translation.x = position.x;
                    transform.translation.y = position.y;
                }
            }
            CameraEvent::Zoom(zoom) => {
                for (_camera, _, mut transform) in q_camera.iter_mut() {
                    transform.scale.x = *zoom;
                    transform.scale.y = *zoom;
                }
            }
            CameraEvent::CopyToClipboard => {
                if let Some(view_texture) = images.get(&camera_setup.cpu_target.as_ref().unwrap()) {
                    let size = view_texture.size().as_ivec2();
                    let size = Extent3d {
                        width: size.x as u32,
                        height: size.y as u32,
                        depth_or_array_layers: 4,
                    };

                    println!("Setting up screenshot: {:?}", size);

                    // TODO: update the size of cpu_target here to re

                    commands.spawn(ImageCopier::new(
                        camera_setup.target.as_ref().unwrap().clone(),
                        camera_setup.cpu_target.as_ref().unwrap().clone(),
                        size,
                        &render_device,
                    ));
                    // image_copier.disable();)

                    // let bytes = [
                    //     255, 100, 100, 255, 100, 255, 100, 100, 100, 100, 255, 100, 0, 0, 0, 255,
                    // ];
                    // let img_data = arboard::ImageData {
                    //     width: 2,
                    //     height: 2,
                    //     bytes: bytes.as_ref().into(),
                    // };
                    // ctx.set_image(img_data).unwrap();
                }
            }
        }
    }
}

fn copy_to_clipboard(
    mut commands: Commands,
    q_copier: Query<(Entity, &ImageCopier)>,
    camera_setup: Res<CameraSetup>,
    mut images: ResMut<Assets<Image>>,
) {
    for (entity, copier) in q_copier.iter() {
        let mut ctx = Clipboard::new().unwrap();

        let view_texture = images
            .get_mut(&camera_setup.cpu_target.as_ref().unwrap())
            .unwrap();

        let size = view_texture.size();

        let width_in_bytes = size.x as usize * 4;

        let pre_data_length = view_texture.data.len();

        let expected_length = (size.x * size.y) as usize * 4;

        // Due to the padding added (power of 2), we need to filter the data
        let data = view_texture
            .data
            .iter()
            .enumerate()
            .filter(|(index, _value)| index % copier.padded_bytes_per_row() < width_in_bytes)
            .map(|(_, value)| *value)
            .collect::<Vec<_>>();
        // let data = &view_texture.data;

        let data_length = data.len();

        if expected_length == data.len() {
            let img_data = arboard::ImageData {
                width: size.x as usize,
                height: size.y as usize,
                bytes: data.into(),
            };

            if let Err(error) = ctx.set_image(img_data) {
                commands.spawn(Message {
                severity: Severity::Error,
                message: format!("Error occured when trying to copy image to clipboard. This can happen if data is still loading in the background. Please try again.\n\nExpected size: {} x {}\nData size: {}\nData size(pre-filter): {}\n\nDescription: {:?}", 
                size.x, size.y, data_length, pre_data_length, error),
            });
            }

            commands.entity(entity).despawn_recursive();
        }
    }
}

// const MAX_CAMERA_WIDTH: f32 = 1e10;
// const MAX_CAMERA_HEIGHT: f32 = 1e10;

#[derive(Component)]
pub struct PanCamera {
    pub x: u32,
    pub y: u32,

    pub camera_text: Entity,

    // This is here to allow a change to be forced, to trigger redrawing of the camera
    pub(crate) force_change_toggle: bool,
}

#[derive(Debug, Default, Component, Clone, Copy)]
pub struct MousePosition {
    pub active_camera: Option<Entity>,

    pub just_changed_camera: bool,

    pub current_window: Vec2,
    pub current_world: Vec4,

    pub last_window: Vec2,
    pub last_world: Vec4,

    pub window_size: Vec2,
}

#[derive(Debug, Default, Component, Clone, Copy)]
pub struct FieldOfView {
    pub top_left: Vec4,
    pub bottom_right: Vec4,
}

#[derive(Resource, Clone)]
pub struct CameraSetup {
    pub x: u32,
    pub y: u32,

    pub margin: u32,

    pub target: Option<Handle<Image>>,
    pub cpu_target: Option<Handle<Image>>,

    pub names: Vec<String>,
}

impl Default for CameraSetup {
    fn default() -> Self {
        CameraSetup {
            x: 1,
            y: 1,
            margin: 10,
            names: Vec::new(),
            target: None,
            cpu_target: None,
        }
    }
}

// #[derive(Component)]
// struct CameraPosition {
//     x: u32,
//     y: u32,

//     camera_text: Entity,
// }

#[derive(Component)]
struct ViewTexture;

fn setup(
    mut commands: Commands,
    mut camera_setup: ResMut<CameraSetup>,
    asset_server: Res<AssetServer>,
    mut images: ResMut<Assets<Image>>,
    render_device: Res<RenderDevice>,
) {
    let size = Extent3d {
        width: 512,
        height: 512,
        ..default()
    };

    // This is the texture that will be rendered to.
    let mut image = Image {
        texture_descriptor: TextureDescriptor {
            label: None,
            size,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba8UnormSrgb, //Rgba8Unorm, //
            mip_level_count: 1,
            sample_count: 1,
            usage: TextureUsages::TEXTURE_BINDING
                | TextureUsages::COPY_SRC
                | TextureUsages::COPY_DST
                | TextureUsages::RENDER_ATTACHMENT,
        },
        ..default()
    };

    // fill image.data with zeroes
    image.resize(size);

    // This is the texture that will be rendered to.
    let mut cpu_image = Image {
        texture_descriptor: TextureDescriptor {
            label: None,
            size,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba8UnormSrgb, //Rgba8Unorm, //
            mip_level_count: 1,
            sample_count: 1,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
        },
        ..default()
    };

    // fill image.data with zeroes
    cpu_image.resize(size);

    let image_handle = images.add(image);
    camera_setup.target = Some(image_handle.clone());
    let cpu_image_handle = images.add(cpu_image);
    camera_setup.cpu_target = Some(cpu_image_handle.clone());

    let far = 1000.0;

    let mut camera_transform = Transform::from_xyz(0.0, 0.0, far - 0.1);
    camera_transform.scale.x *= 100.;
    camera_transform.scale.y *= 100.;

    // let camera_bundle = Camera2dBundle {
    //     transform: camera_transform,
    //     ..default()
    // };

    // commands.spawn().insert(CameraSetup {
    //     x: 2,
    //     y: 2,
    //     margin: 10,
    // });

    // This specifies the layer used for the ui and showing the texture
    let ui_layer = RenderLayers::layer(1);
    commands.spawn((
        SpriteBundle {
            texture: image_handle,
            ..default()
        },
        ui_layer,
        ViewTexture,
    ));

    // Spawn the UI camera
    // Transform the camera well away from the view - there should be a nicer way to do this, but otherwise
    // this camera displays the scene as well as the UI, causing issues for the multiview cameras
    commands.spawn((
        Camera2dBundle {
            camera: Camera {
                priority: 100,
                ..default()
            },
            camera_2d: Camera2d {
                // don't clear the color while rendering this camera
                clear_color: ClearColorConfig::None,
            },
            // transform: Transform::from_xyz(-1000000.0, 1000000.0, 0.0),
            ..default()
        },
        UiCameraConfig::default(),
        ui_layer,
    ));

    commands.spawn(MousePosition::default());

    create_cameras(commands, camera_setup.as_ref(), &asset_server);
}

fn changed_camera_setup(
    mut commands: Commands,
    camera_setup: Res<CameraSetup>,
    cameras: Query<(Entity, &PanCamera)>,
    asset_server: Res<AssetServer>,
) {
    if camera_setup.is_changed() {
        println!("Despawning cameras!!");
        for (entity, camera) in cameras.iter() {
            commands.entity(entity).despawn_recursive();
            commands.entity(camera.camera_text).despawn_recursive();
        }

        create_cameras(commands, camera_setup.as_ref(), &asset_server);
    }
}

#[derive(Component)]
struct CameraText;

fn create_cameras(mut commands: Commands, camera_setup: &CameraSetup, asset_server: &AssetServer) {
    for y in 0..camera_setup.y {
        for x in 0..camera_setup.x {
            let mut transform = Transform::from_scale(Vec3::new(40.0, 40.0, 1.0));
            transform.translation.x = 15000.0;
            transform.translation.y = 10000.0;
            transform.translation.z = 800.0;

            let camera_bundle = if x == 0 && y == 0 {
                Camera2dBundle {
                    transform,
                    camera: Camera {
                        target: RenderTarget::Image(camera_setup.target.as_ref().unwrap().clone()),
                        ..default()
                    },

                    ..default()
                }
            } else {
                Camera2dBundle {
                    transform,
                    camera: Camera {
                        priority: ((y * camera_setup.x) + x) as isize,

                        target: RenderTarget::Image(camera_setup.target.as_ref().unwrap().clone()),
                        ..default()
                    },
                    camera_2d: Camera2d {
                        clear_color: ClearColorConfig::None,
                    },
                    ..default()
                }
            };

            let camera_text = commands
                .spawn(
                    // Create a TextBundle that has a Text with a single section.
                    TextBundle::from_section(
                        // Accepts a `String` or any type that converts into a `String`, such as `&str`
                        "",
                        TextStyle {
                            font: asset_server.load("fonts/lato/Lato-Regular.ttf"),
                            font_size: 20.0,
                            color: Color::WHITE,
                        },
                    ) // Set the alignment of the Text
                    .with_text_alignment(TextAlignment::BOTTOM_LEFT)
                    // Set the style of the TextBundle itself.
                    .with_style(Style {
                        align_self: AlignSelf::FlexEnd,
                        position_type: PositionType::Absolute,
                        position: UiRect {
                            bottom: Val::Px(15.0),
                            left: Val::Px(50.0),
                            ..default()
                        },
                        ..default()
                    }),
                )
                .insert(CameraText)
                .id();

            commands
                .spawn(camera_bundle)
                .insert(PanCamera {
                    x,
                    y,
                    camera_text,
                    force_change_toggle: false,
                })
                .insert(FieldOfView::default())
                .insert(Selectable::with_priority(-1))
                .insert(UiCameraConfig { show_ui: false })
                // .insert(BoundingBox {
                //     x: 0.0,
                //     y: 0.0,
                //     width: MAX_CAMERA_WIDTH,
                //     height: MAX_CAMERA_HEIGHT,
                // })
                .insert(Draggable);

            // println!("Creating cameras: {} {} => {:?}", x, y, entity);
        }
    }
}

/// Check whether the UI has changed (for example that the panel size has changed)
/// If so, then we need to redraw the viewports
fn ui_changed(ui_space: Res<UiSpace>, mut cameras: Query<&mut PanCamera>) {
    if ui_space.is_changed() {
        // Change the camera, hence forcing a change to be detected and then the camera redrawn
        for mut camera in cameras.iter_mut() {
            camera.force_change_toggle = !camera.force_change_toggle;
        }
    }
}

/// Check whether the window has been resized, if so then need to redraw the viewports
fn window_resized(
    mut resize_events: EventReader<WindowResized>,
    mut cameras: Query<&mut PanCamera>,
) {
    // We need to dynamically resize the camera's viewports whenever the window size changes
    // A resize_event is sent when the window is first created, allowing us to reuse this system for initial setup.
    for resize_event in resize_events.iter() {
        if resize_event.id == WindowId::primary() {
            // Change the camera, hence forcing a change to be detected and then the camera redrawn
            for mut camera in cameras.iter_mut() {
                camera.force_change_toggle = !camera.force_change_toggle;
            }
        }
    }
}

fn update_camera(
    windows: Res<Windows>,
    ui_space: Res<UiSpace>,
    camera_setup: Res<CameraSetup>,
    mut view_texture: Query<&mut Transform, With<ViewTexture>>,
    mut images: ResMut<Assets<Image>>,
    mut cameras: Query<(&mut Camera, &PanCamera), Changed<PanCamera>>,
    mut camera_text: Query<(&mut Text, &mut Style), With<CameraText>>,
) {
    // If no camera needs to be updated, then don't bother proceeding
    if cameras.is_empty() {
        return;
    }

    let window = windows.primary();

    let panel_width = ui_space.right() * window.scale_factor() as f32;
    let top_panel_height = ui_space.top() * window.scale_factor() as f32;
    let bottom_panel_height = ui_space.bottom() * window.scale_factor() as f32;

    let physical_view_width = window.physical_width() - panel_width as u32;
    let physical_view_height =
        window.physical_height() - top_panel_height as u32 - bottom_panel_height as u32;

    let physical_camera_width =
        (physical_view_width - (camera_setup.margin * (camera_setup.x - 1))) / camera_setup.x;
    let physical_camera_height =
        (physical_view_height - (camera_setup.margin * (camera_setup.y - 1))) / camera_setup.y;

    let width = physical_camera_width as f32 / window.scale_factor() as f32;
    let height = physical_camera_height as f32 / window.scale_factor() as f32;

    info!("Window resized - resizing all textures");

    // TODO: Do we really want to resize these images every time?
    let image_width = physical_view_width as f32;// / window.scale_factor() as f32;
    let image_height = (window.physical_height() - top_panel_height as u32) as f32;// / window.scale_factor() as f32;

    images
        .get_mut(&camera_setup.target.as_ref().unwrap())
        .unwrap()
        .resize(Extent3d {
            width: image_width as u32,
            height: image_height as u32,
            ..default()
        });
    images
        .get_mut(&camera_setup.cpu_target.as_ref().unwrap())
        .unwrap()
        .resize(Extent3d {
            width: image_width as u32,
            height: image_height as u32,
            ..default()
        });

    if let Ok(mut view_texture_transform) = view_texture.get_single_mut() {
        view_texture_transform.translation.x = -panel_width / 2.0 / window.scale_factor() as f32;
        view_texture_transform.translation.y = top_panel_height / 2.0 / window.scale_factor() as f32;

        view_texture_transform.scale.x =  1.0 / window.scale_factor() as f32;
        view_texture_transform.scale.y =  1.0 / window.scale_factor() as f32;
    }

    // println!("{:?}", window);
    // println!("{:?}", ui_space);

    // UI pixel coordinates start at (0, 0) at bottom left

    for (mut camera, position) in cameras.iter_mut() {
        camera.viewport = Some(Viewport {
            physical_position: UVec2::new(
                (physical_camera_width + camera_setup.margin) * position.x,
                (physical_camera_height + camera_setup.margin) * (position.y)
                    + top_panel_height as u32,
            ),
            physical_size: UVec2::new(physical_camera_width, physical_camera_height),
            ..default()
        });

        // println!("{:?}", camera.viewport);

        if let Ok((mut text, mut style)) = camera_text.get_mut(position.camera_text) {
            let index = (position.y * camera_setup.x) + position.x;

            if let Some(name) = camera_setup.names.get(index as usize) {
                if name != &text.sections[0].value {
                    text.sections[0].value = name.clone();
                }
            }

            let pos_y = ((height as u32 + camera_setup.margin) * (camera_setup.y - position.y - 1))
                as f32
                + ui_space.bottom();

            style.position = UiRect {
                left: Val::Px(((width as u32 + camera_setup.margin) * position.x) as f32 + 10.0),
                // right: Val::Px(((width + camera_setup.margin) * (position.x + 1)) as f32),
                //top: Val::Px(pos_y + 100.0),
                bottom: Val::Px(pos_y + 10.0), // Need to take into account the bottom info bar from egui!
                ..default()
            };
        }
    }
}

#[derive(Component, Default)]
pub struct Selectable {
    //pub bounding_box: Option<Rectangle<f32>>,
    pub priority: i64,
}

#[derive(Component, Default)]
pub struct BoundingBox {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Selectable {
    pub fn with_priority(priority: i64) -> Self {
        Selectable { priority }
    }
}

// impl Default for Selectable {
//     fn default() -> Self {
//         Selectable {
//             bounding_box: None,
//             priority: 0,
//         }
//     }
// }

pub fn get_primary_window_size(windows: &Res<Windows>) -> Vec2 {
    let window = windows.get_primary().unwrap();

    Vec2::new(window.width() as f32, window.height() as f32)
}

pub fn camera_to_world(
    camera: &Camera,
    pos: Vec2,
    window_size: Vec2,
    camera_transform: &Transform,
) -> Vec4 {
    // Undo default orthographic projection (pixels from centre)
    let p = match &camera.viewport {
        Some(viewport) => {
            let pos = Vec2::new(pos.x, window_size.y - pos.y);
            // println!(
            //     "{:?} | {:?} | {:?} | {:?}",
            //     pos,
            //     pos - viewport.physical_position.as_vec2(),
            //     viewport.physical_position,
            //     viewport.physical_size
            // );

            let mut pos =
                pos - viewport.physical_position.as_vec2() - viewport.physical_size.as_vec2() / 2.0;
            pos.y *= -1.0;

            pos
        }
        None => pos - window_size / 2.0,
    };

    camera_transform.compute_matrix() * p.extend(0.0).extend(1.0)
}

pub fn world_to_camera(pos: Vec4, window_size: Vec2, camera_transform: &Transform) -> Vec2 {
    let p = camera_transform.compute_matrix().inverse() * pos;
    let p = Vec2::new(p.x, p.y);

    // Undo default orthographic projection (pixels from centre)
    p + window_size / 2.0
}

#[derive(Component)]
pub struct Selected;

#[derive(WorldQuery)]
pub struct SizedEntity {
    pub entity: Entity,
    pub transform: &'static GlobalTransform,
    pub sprite: Option<&'static Sprite>,
    pub image_handle: Option<&'static Handle<Image>>,
    pub bounding_box: Option<&'static BoundingBox>,
}

impl<'w> SizedEntityItem<'w> {
    pub fn top_left(&self, images: &Res<Assets<Image>>) -> Option<Vec3> {
        if let Some(bounding_box) = self.bounding_box {
            Some(Vec3::new(
                bounding_box.x - bounding_box.width / 2.0,
                bounding_box.y - bounding_box.height / 2.0,
                0.0,
            ))
        } else if let (Some(sprite), Some(image_handle)) = (self.sprite, self.image_handle) {
            let (width, height) = if let Some(custom_size) = sprite.custom_size {
                (custom_size.x, custom_size.y)
            } else {
                let image = images.get(image_handle).unwrap();
                (image.size().x, image.size().y)
            };

            Some(Vec3::new(-width / 2.0, -height / 2.0, 0.0))
        } else {
            None
        }
    }

    pub fn bottom_right(&self, images: &Res<Assets<Image>>) -> Option<Vec3> {
        if let Some(bounding_box) = self.bounding_box {
            Some(Vec3::new(
                bounding_box.x + bounding_box.width / 2.0,
                bounding_box.y + bounding_box.height / 2.0,
                0.0,
            ))
        } else if let (Some(sprite), Some(image_handle)) = (self.sprite, self.image_handle) {
            let (width, height) = if let Some(custom_size) = sprite.custom_size {
                (custom_size.x, custom_size.y)
            } else {
                let image = images.get(image_handle).unwrap();
                (image.size().x, image.size().y)
            };

            Some(Vec3::new(width / 2.0, height / 2.0, 0.0))
        } else {
            None
        }
    }
}

fn select_object(
    mut egui_ctx: ResMut<EguiContext>,
    mut commands: Commands,
    mouse_input: Res<Input<MouseButton>>,

    q_mouse_position: Query<&MousePosition>,
    q_selectable: Query<(&Selectable, SizedEntity)>,
    images: Res<Assets<Image>>,
) {
    // Check position is not in the menu or side panel
    //egui_ctx.ctx_mut().wants_keyboard_input()
    // || egui_ctx.ctx_mut().is_pointer_over_area()
    if egui_ctx.ctx_mut().wants_pointer_input() || egui_ctx.ctx_mut().is_using_pointer() {
        return;
    }

    if mouse_input.just_pressed(MouseButton::Left) {
        let mouse_position = q_mouse_position
            .get_single()
            .expect("There should be only one MousePosition");
        let pos_world = mouse_position.current_world;

        let mut possible_selections: Vec<(Entity, &Selectable)> = q_selectable
            .iter()
            .filter_map(|(selectable, sized)| {
                // Get the top left and bottom right points
                let top_left = sized.top_left(&images)?;
                let bottom_right = sized.bottom_right(&images)?;

                // Transform top left and bottom right points to world coordinates
                let top_left = sized.transform.transform_point(top_left);
                let bottom_right = sized.transform.transform_point(bottom_right);

                // Perform check whether point in bounding box
                if pos_world.x >= top_left.x.min(bottom_right.x)
                    && pos_world.x <= top_left.x.max(bottom_right.x)
                    && pos_world.y >= top_left.y.min(bottom_right.y)
                    && pos_world.y <= top_left.y.max(bottom_right.y)
                {
                    Some((sized.entity, selectable))
                } else {
                    None
                }
            })
            .collect();

        // Add the active camera as a possible selection
        if let Some(camera) = mouse_position.active_camera {
            if let Ok(camera_selection) = q_selectable.get(camera) {
                possible_selections.push((camera, camera_selection.0));
            }
        }

        let to_select = possible_selections
            .iter()
            .max_by_key(|(_entity, selectable)| selectable.priority);

        if let Some((entity, _sized)) = to_select {
            // println!("Selecting {:?}!!!", entity);
            commands.entity(*entity).insert(Selected);
        }
    }
}

fn selected(
    mut commands: Commands,
    mouse_input: Res<Input<MouseButton>>,
    q_selected: Query<Entity, With<Selected>>,
    q_camera: Query<Entity, (With<Selected>, With<PanCamera>)>,
) {
    if mouse_input.just_pressed(MouseButton::Right) {
        println!("Testing..");
        for entity in q_selected.iter() {
            println!("Selected {:?}", entity)
        }
    }

    // Remove the camera from being selected, if one has been
    if mouse_input.just_released(MouseButton::Left) {
        for entity in q_camera.iter() {
            commands.entity(entity).remove::<Selected>();
        }
    }
}

fn camera_zoom(
    mut egui_ctx: ResMut<EguiContext>,
    mut q_camera: Query<(Entity, &Camera, &mut Transform, &mut FieldOfView), With<PanCamera>>,
    mut scroll_events: EventReader<MouseWheel>,

    windows: Res<Windows>,
    q_mouse_position: Query<&MousePosition>,
) {
    // Check position is not in the menu or side panel
    if egui_ctx.ctx_mut().is_pointer_over_area() {
        return;
    }

    let pixels_per_line = 100.; // Maybe make configurable?
    let scroll = scroll_events
        .iter()
        .map(|ev| match ev.unit {
            MouseScrollUnit::Pixel => ev.y,
            MouseScrollUnit::Line => ev.y * pixels_per_line,
        })
        .sum::<f32>();

    if scroll == 0. {
        return;
    }

    let mouse_position = q_mouse_position.single();
    let window_size = get_primary_window_size(&windows);

    if let Some(active_camera) = mouse_position.active_camera {
        for (camera_entity, camera, mut projection, mut field_of_view) in q_camera.iter_mut() {
            projection.scale.x *= 1. + -scroll * 0.001; //.max(0.00001);
            projection.scale.y *= 1. + -scroll * 0.001;

            // println!("Camera Transform: {:?}", projection);

            if active_camera == camera_entity {
                let current_pos_world = camera_to_world(
                    camera,
                    mouse_position.current_window,
                    window_size,
                    &projection,
                );

                projection.translation.x -= current_pos_world.x - mouse_position.current_world.x;
                projection.translation.y -= current_pos_world.y - mouse_position.current_world.y;

                field_of_view.top_left = camera_to_world(
                    camera,
                    Vec2::new(0.0, window_size.y),
                    window_size,
                    &projection,
                );
                field_of_view.bottom_right = camera_to_world(
                    camera,
                    Vec2::new(window_size.x, 0.0),
                    window_size,
                    &projection,
                );
            }
        }
    }
}

#[derive(Default, Component)]
pub struct Draggable;

#[derive(Default, Component)]
struct Dragging;

#[derive(Debug)]
pub struct DraggedEvent(pub Entity);

fn update_mouse_position(
    windows: Res<Windows>,
    q_camera: Query<(Entity, &Camera, &Transform), With<PanCamera>>,
    mut q_mouse_position: Query<&mut MousePosition>,
) {
    let window = windows.get_primary().unwrap();
    let window_size = get_primary_window_size(&windows);

    // Use position instead of MouseMotion, otherwise we don't get acceleration movement
    let current_pos = match window.cursor_position() {
        Some(current_pos) => current_pos,
        None => return,
    };

    // Reset to being no active camera - this stops buggy behaviour if we have multiple cameras active
    if let Ok(mut mouse_position) = q_mouse_position.get_single_mut() {
        mouse_position.active_camera = None;
    }

    for (entity, camera, transform) in q_camera.iter() {
        // Check whether the ca
        let active_camera = match &camera.viewport {
            Some(viewport) => {
                // Flip the y-axis
                let current_pos_y = window_size.y - current_pos.y;

                // Have to take into account the scale factor
                let viewport_pos = Vec2::new(
                    viewport.physical_position.x as f32 / window.scale_factor() as f32,
                    viewport.physical_position.y as f32 / window.scale_factor() as f32,
                );

                let viewport_size = Vec2::new(
                    viewport.physical_size.x as f32 / window.scale_factor() as f32,
                    viewport.physical_size.y as f32 / window.scale_factor() as f32,
                );

                current_pos.x >= viewport_pos.x as f32
                    && current_pos.x < (viewport_pos.x + viewport_size.x) as f32
                    && current_pos_y >= viewport_pos.y as f32
                    && current_pos_y < (viewport_pos.y + viewport_size.y) as f32
            }
            None => true,
        };

        if active_camera {
            //if let Ok(camera_transform) = q_camera.get_single() {

            let current_pos_world = camera_to_world(camera, current_pos, window_size, transform);

            if let Ok(mut mouse_position) = q_mouse_position.get_single_mut() {
                // Update last position with the new transform (if changed)
                // TODO: Might be possible to reduce computation if we can somehow check for a change...e.g. add
                // in new system before update_mouse_position()
                mouse_position.just_changed_camera = match mouse_position.active_camera {
                    Some(previous_camera) => previous_camera != entity,
                    None => true,
                };

                mouse_position.active_camera = Some(entity);

                mouse_position.last_window = mouse_position.current_window;
                mouse_position.last_world = camera_to_world(
                    camera,
                    mouse_position.current_window,
                    window_size,
                    transform,
                );

                mouse_position.current_window = current_pos;
                mouse_position.current_world = current_pos_world;

                mouse_position.window_size = window_size;

                // println!("{:?}", mouse_position);
            }
        }
    }
}

fn dragging_camera(
    windows: Res<Windows>,
    q_mouse_position: Query<&MousePosition>,
    mut q_camera: Query<
        (&Camera, &mut Transform, &mut FieldOfView),
        (With<Selected>, With<PanCamera>),
    >,
) {
    if let Ok(mouse_position) = q_mouse_position.get_single() {
        if let Some(camera_entity) = mouse_position.active_camera {
            if let Ok((camera, mut camera_transform, mut field_of_view)) =
                q_camera.get_mut(camera_entity)
            {
                // println!("Trying to update camera {:?} ", camera_entity);
                let delta = mouse_position.current_world - mouse_position.last_world;

                camera_transform.translation.x -= delta.x;
                camera_transform.translation.y -= delta.y;

                let window_size = get_primary_window_size(&windows);

                field_of_view.top_left = camera_to_world(
                    camera,
                    Vec2::new(0.0, window_size.y),
                    window_size,
                    &camera_transform,
                );
                field_of_view.bottom_right = camera_to_world(
                    camera,
                    Vec2::new(window_size.x, 0.0),
                    window_size,
                    &camera_transform,
                );
            }
        }
    }
}

fn dragging(
    mut commands: Commands,
    mouse_input: Res<Input<MouseButton>>,
    q_mouse_position: Query<&MousePosition>,
    mut q_selected: Query<
        (Entity, &mut Transform),
        (With<Draggable>, With<Selected>, Without<PanCamera>),
    >,
    mut ev_dragged: EventWriter<DraggedEvent>,
) {
    if let Ok(mouse_position) = q_mouse_position.get_single() {
        let delta = mouse_position.current_world - mouse_position.last_world;

        if delta.length_squared() > 0.0 {
            for (entity, mut transform) in q_selected.iter_mut() {
                transform.translation.x += delta.x;
                transform.translation.y += delta.y;

                ev_dragged.send(DraggedEvent(entity));
            }
        }
    }

    if mouse_input.just_released(MouseButton::Left) {
        for (entity, _transform) in q_selected.iter_mut() {
            commands.entity(entity).remove::<Selected>();
        }
    }
}
