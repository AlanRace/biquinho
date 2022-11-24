//#![warn(clippy::missing_docs_in_private_items)]
#![warn(missing_docs)]
#![warn(clippy::unwrap_used)]
//! Biquinho - visualisation of imaging mass cytometry data.
//!
//! Functionality is split out into separate plugins. This main file deals only with the setup.
//!
//! # Features
//!
//! - [x] Load .mcd files and visualise optical data
//! - [x] Select channels to form RGB composite with user-specified thresholds
//! - [x] View multiple IMC acquisitions in single .mcd file at once (multi-camera)
//! - [ ] View multiple .mcd files
//! - [ ] Save/load current view/project
//! - [x] Add annotatations and annotate data (e.g. with pencil tool)
//! - [ ] Classify IMC data based on annotations
//! - [ ] Segment cells based on classification data
//! - [x] Load and visualise cell segmentation results
//! - [ ] Calculate per-cell statistics (e.g. channel intensity)
//! - [ ] Cell phenotyping
//! - [ ] Network/neighbourhood analysis
//!
//!
//!
//! ## TODO: View multiple .mcd files
//! - [ ] Implement means of dragging entire dataset
//! - [ ] Provide option to load new dataset above loaded datasets
//!
//! ## TODO: Segment cells based on classification data
//! - [ ] Watershed on probability map (random forest)
//! - [ ] U-Net
//!
//! ## TODO: Calculate per-cell statistics (e.g. channel intensity)
//! - [ ] Link a cell to a specific acquisition
//! - [ ] Calculate cell area and shape(?)
//! - [ ] Calculate mean/std/median for each channel within cell
//! - [ ] Allow the selection of a single channel - colour each cell with mean/median of this channel data
//! - [ ] Export cell data to csv
//!
//! ## TODO: Cell phenotyping
//! - [ ] Manual thresholding / gating
//! - [ ] UMAP & density-based clustering
//! - [ ] Variational inference

use std::path::PathBuf;

/// AnnotationPlugin - handles everything related to drawing, saving and loading annotations.
mod annotation;
/// CameraPlugin - handles viewing data view one or more cameras and selecting and dragging objects viewed in the camera.
mod camera;
mod colour;
mod data;
// mod geometry;
mod image_copy;
/// ImagePlugin - handles loading and viewing image data (including channel images).
mod image_plugin;
/// IMCPlugin - handles specific loading and visualisation of imaging mass cytometry data.
mod imc;
/// Helper functions and structs for dealing with transformations (affine).
mod transform;
/// UiPlugin - handles everything related to the user interface (currently everything egui related).
mod ui;

use bevy::{
    diagnostic::LogDiagnosticsPlugin,
    prelude::*,
    window::WindowId,
    winit::{WinitSettings, WinitWindows},
};
use bevy_prototype_lyon::plugin::ShapePlugin;
use camera::CameraCommand;
use data::DataPlugin;
use imc::IMCEvent;

use transform::AffineTransform;

use crate::{imc::IMCPlugin, ui::UiPlugin};

fn main() {
    let mut app = App::new();
    let app = app
        .insert_resource(WinitSettings::desktop_app())
        .insert_resource(ClearColor(Color::rgb(0.3, 0.3, 0.3)))
        .add_plugins(
            DefaultPlugins
                .set(ImagePlugin::default_nearest())
                .set(WindowPlugin {
                    window: WindowDescriptor {
                        title: "Biquinho".to_string(),
                        //present_mode: PresentMode::Fifo,
                        // scale_factor_override: Some(1.0),
                        ..default()
                    },
                    ..default()
                }),
        )
        //.insert_resource(Msaa { samples: 4 })
        .add_plugin(UiPlugin)
        .add_plugin(image_plugin::ImagePlugin)
        .add_plugin(DataPlugin)
        .add_plugin(LogDiagnosticsPlugin::default())
        // .add_plugin(FrameTimeDiagnosticsPlugin::default())
        .add_plugin(ShapePlugin);

    #[cfg(feature = "imc")]
    let app = app.add_plugin(IMCPlugin);

    app.add_startup_system(load_test_data)
        .add_startup_system(setup)
        .add_startup_system(set_window_icon)
        //.add_system(print_messages)
        .run();
}

/// Set the window icon - startup system
fn set_window_icon(
    // we have to use `NonSend` here
    windows: NonSend<WinitWindows>,
) {
    let primary = windows.get_window(WindowId::primary()).unwrap();

    // here we use the `image` crate to load our icon data from a png file
    // this is not a very bevy-native solution, but it will do
    let (icon_rgba, icon_width, icon_height) = {
        let image = image::open("assets/biquihno_icon.png")
            .expect("Failed to open icon path")
            .into_rgba8();
        let (width, height) = image.dimensions();
        let rgba = image.into_raw();
        (rgba, width, height)
    };

    let icon = winit::window::Icon::from_rgba(icon_rgba, icon_width, icon_height).unwrap();

    primary.set_window_icon(Some(icon));
}

/// Severity of the error message
#[derive(Debug)]
pub enum Severity {
    /// Information - not an error
    Info,
    /// Warning - not an error
    Warning,
    /// Error - something has gone wrong
    Error,
    /// Fatal error - something has gone wrong and we can't recover
    Fatal,
}

/// Message component with message and severity.
#[derive(Component)]
pub struct Message {
    /// The severity of the message.
    pub severity: Severity,
    /// The message itself.
    pub message: String,
}

impl From<std::io::Error> for Message {
    fn from(error: std::io::Error) -> Self {
        Self {
            severity: Severity::Error,
            message: error.to_string(),
        }
    }
}

/// Load in a dataset for testing automatically - if the dataset isn't found, then just skip loading data
fn load_test_data(mut imc_events: EventWriter<IMCEvent>) {
    let path = PathBuf::from(
        // "/home/alan/Documents/Work/Nicole/Salmonella/2019-10-25_Salmonella_final_VS+WT.mcd",
        "/home/alan/Documents/Work/20200609_ARDS_1921.mcd",
    );

    if std::path::Path::new(&path).exists() {
        imc_events.send(IMCEvent::Load(path));
    }
}
// fn load_test_data(mut commands: Commands) {
//     let thread_pool = AsyncComputeTaskPool::get();

//     let path = PathBuf::from(
//         // "/home/alan/Documents/Work/Nicole/Salmonella/2019-10-25_Salmonella_final_VS+WT.mcd",
//         "/home/alan/Documents/Work/20200609_ARDS_1921.mcd",
//     );

//     let file = match File::open(&path) {
//         Ok(file) => file,
//         Err(_error) => {
//             println!("Not opening test data.");
//             return;
//         }
//     };

//     let load_task = thread_pool.spawn(async move {
//         let mcd = MCD::from_path(path)?.with_dcm()?;

//         // let xml = mcd.xml()?;
//         // std::fs::write("mcd.xml", xml);

//         Ok(mcd)
//     });
//     commands.spawn(LoadIMC(load_task));
// }

/// Convert an `AffineTransform` into a bevy `Transform`.
///
/// We have to modify the transform to take into account that Bevy relies on the centre of image and upsidedown
fn create_transform(
    transform: &AffineTransform,
    width: f32,
    height: f32,
    flip_required: bool,
) -> Transform {
    let translate_transform = Transform::from_xyz(width / 2.0, height / 2.0, 0.0);
    //let scale_transform = Transform::from_scale(Vec3::new(1.0, 1.0, 0.0));
    let transform = Transform::from_matrix(transform.into());

    if flip_required {
        let transform = transform.mul_transform(translate_transform);
        println!("Trans: {:?}", transform);

        transform.with_translation(Vec3::new(
            transform.translation.x,
            25000.0 - transform.translation.y,
            0.0,
        ))
        //.mul_transform(Transform::from_scale(Vec3::new(1.0, -1.0, 1.0)))
        //.mul_transform()
    } else {
        transform.mul_transform(translate_transform)
    }
    //    .mul_transform(scale_transform)
}

/// This setup function generates a grid with a spacing of 2000 um
fn setup(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut camera_events: EventWriter<CameraCommand>,
) {
    // Send an event to enable dragging of camera
    camera_events.send(CameraCommand::EnableDragging);

    // Draw grid lines
    // TODO: Move the grid generation to its own plugin?
    let grid_length = 1000000.0;
    let grid_spacing = 2000.0;
    let grid_thickness = 100.0;

    let num_gridlines = 100;

    let grid_alpha = 0.25;

    for y in 0..=num_gridlines {
        let y_value = (-(num_gridlines / 2) + y) as f32 * grid_spacing;

        commands.spawn(SpriteBundle {
            transform: Transform::from_xyz(0.0, y_value, 0.0),
            sprite: Sprite {
                custom_size: Some(Vec2::new(grid_length, grid_thickness)),
                color: Color::rgba(0.75, 0.75, 0.75, grid_alpha),
                ..Default::default()
            },
            ..Default::default()
        });

        if y_value >= 0.0 {
            commands.spawn(Text2dBundle {
                // Use `Text` directly
                text: Text {
                    // Construct a `Vec` of `TextSection`s
                    sections: vec![TextSection {
                        value: format!("{}", y_value),
                        style: TextStyle {
                            font: asset_server.load("fonts/lato/Lato-Bold.ttf"),
                            font_size: 60.0,
                            color: Color::WHITE,
                        },
                    }],
                    alignment: TextAlignment {
                        vertical: VerticalAlign::Center,
                        horizontal: HorizontalAlign::Right,
                    },
                },
                transform: Transform::from_xyz(-300.0, y_value, 1.0)
                    .mul_transform(Transform::from_scale(Vec3::new(10.0, 10.0, 1.0))),
                ..default()
            });
        }
    }

    for x in 0..=num_gridlines {
        let x_value = (-(num_gridlines / 2) + x) as f32 * grid_spacing;

        commands.spawn(SpriteBundle {
            //material: materials.add(Color::rgb(0.5, 0.5, 1.0).into()),
            transform: Transform::from_xyz(x_value, 0.0, 0.0),
            sprite: Sprite {
                custom_size: Some(Vec2::new(grid_thickness, grid_length)),
                color: Color::rgba(0.75, 0.75, 0.75, grid_alpha),
                ..Default::default()
            },
            ..Default::default()
        });

        if x_value >= 0.0 {
            commands
            .spawn(Text2dBundle {
                // Use `Text` directly
                text: Text {
                    // Construct a `Vec` of `TextSection`s
                    sections: vec![
                        TextSection {
                            value: format!("{}", x_value),
                            style: TextStyle {
                                font: asset_server.load("fonts/lato/Lato-Bold.ttf"),
                                font_size: 60.0,
                                color: Color::WHITE,
                            },
                        },
                    ],
                    alignment: TextAlignment {
                        vertical: VerticalAlign::Center,
                        horizontal: HorizontalAlign::Center,
                    },
                },
                transform: Transform::from_xyz(
                    (-(num_gridlines / 2) + x) as f32 * grid_spacing,
                    -300.0,
                    1.0,
                ).mul_transform(Transform::from_scale(Vec3::new(10.0, 10.0, 1.0))),
                ..default()
            })
            //.insert(ColorText)
            ;
        }
    }
}

/// Print any messages to the console.
fn print_messages(q_errors: Query<&Message>) {
    for error in q_errors.iter() {
        println!("{}", error.message);
    }
}
